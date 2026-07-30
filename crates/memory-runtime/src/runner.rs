//! Runtime run 创建与事务化转换。

use std::{fmt, str::FromStr};

use chrono::{DateTime, Utc};
use jc_memory_protocol::{
    AccessContext, Decision, DenyReason, EvidenceRef, MemoryScope, Operation, TraceId,
    VerificationMethod,
};
use jc_memory_security::AclEngine;
use jc_memory_storage::SqliteStore;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest as _, Sha256};
use sqlx::{Row as _, Sqlite, SqlitePool, Transaction};
use uuid::Uuid;

use crate::{
    RuntimeState, TransitionError, recovery::load_validated_snapshot, state::RuntimeEvent,
};

/// 一个 Runtime run 的强类型标识。
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct RunId(Uuid);

impl RunId {
    /// 创建 UUID v7 Runtime run 标识。
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl Default for RunId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for RunId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl FromStr for RunId {
    type Err = uuid::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Uuid::parse_str(value).map(Self)
    }
}

/// 最近一次已提交转换之后的可恢复快照。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeSnapshot {
    /// Runtime run 标识。
    pub run_id: RunId,
    /// Run 所属的精确记忆 Scope。
    pub scope: MemoryScope,
    /// 最近一次已提交状态。
    pub state: RuntimeState,
    /// 下一次成功转换应使用的单调递增步骤号。
    pub next_step: u64,
    /// 用于拒绝陈旧快照写入的乐观并发版本。
    pub version: u64,
}

/// 一次 Runtime 转换必须持久化的上下文。
#[derive(Clone, Debug, PartialEq)]
pub struct TransitionMetadata {
    /// 转换读取的权威证据引用。
    pub input_refs: Vec<EvidenceRef>,
    /// 转换产生或确认的权威证据引用。
    pub output_refs: Vec<EvidenceRef>,
    /// 当前动作尝试次数，从一开始计数。
    pub attempt: u32,
    /// 以 UTC 表示的截止时间，持久化为 Unix 微秒。
    pub deadline: DateTime<Utc>,
    /// 分布式 Trace 标识。
    pub trace_id: TraceId,
    /// 与转换相关的结构化载荷。
    pub payload: Value,
}

/// Runtime 状态机与持久化错误。
#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    /// 当前状态拒绝该事件。
    #[error(transparent)]
    Transition(#[from] TransitionError),
    /// `SQLite` 操作失败。
    #[error("runtime database failure: {0}")]
    Storage(#[from] sqlx::Error),
    /// JSON 编解码失败。
    #[error("runtime serialization failure: {0}")]
    Serialization(#[from] serde_json::Error),
    /// 调用方不具备 Runtime Scope 的精确 `Mutate` Grant。
    #[error("runtime access denied: {0:?}")]
    AccessDenied(DenyReason),
    /// `EvidenceRef` 不存在、字段与权威行不一致或目标不属于 run Scope。
    #[error("invalid runtime evidence reference")]
    InvalidEvidenceRef,
    /// 转换元数据不满足持久化不变量。
    #[error("invalid runtime transition metadata: {0}")]
    InvalidMetadata(&'static str),
    /// 调用方使用了已经被其他成功转换推进的快照。
    #[error("runtime snapshot was modified concurrently")]
    ConcurrentModification,
    /// 持久化 Runtime 行无法安全解码。
    #[error("corrupt runtime state: {0}")]
    Corrupt(String),
}

/// 基于 `SQLite` 的 Runtime 状态机执行器。
#[derive(Clone, Debug)]
pub struct RuntimeRunner {
    pool: SqlitePool,
}

impl RuntimeRunner {
    /// 从现有 Store 构造 Runtime 执行器。
    #[must_use]
    pub fn from_store(store: &SqliteStore) -> Self {
        Self {
            pool: store.pool().clone(),
        }
    }

    /// 创建从 [`RuntimeState::Observing`] 开始的新 run。
    ///
    /// # Errors
    ///
    /// 缺少精确 `Mutate` Grant、Scope 无法序列化或数据库写入失败时返回错误。
    pub async fn start(
        &self,
        access: &AccessContext,
        scope: &MemoryScope,
    ) -> Result<RuntimeSnapshot, RuntimeError> {
        authorize(access, scope)?;
        let run_id = RunId::new();
        let scope_json = serde_json::to_string(scope)?;
        let scope_key = scope_key(scope)?;
        let now = Utc::now().timestamp_micros();
        sqlx::query(
            "INSERT INTO runtime_runs (
                run_id, scope_key, scope_json, state, next_step, version,
                created_at_us, updated_at_us
             ) VALUES (?1, ?2, ?3, ?4, 0, 0, ?5, ?5)",
        )
        .bind(run_id.to_string())
        .bind(scope_key)
        .bind(scope_json)
        .bind(RuntimeState::Observing.as_str())
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(RuntimeSnapshot {
            run_id,
            scope: scope.clone(),
            state: RuntimeState::Observing,
            next_step: 0,
            version: 0,
        })
    }

    /// 原子持久化一次合法转换并返回新的已提交快照。
    ///
    /// # Errors
    ///
    /// 授权失败、历史损坏、快照陈旧、证据无效、转换非法或数据库事务失败时返回错误。
    pub async fn transition(
        &self,
        access: &AccessContext,
        expected: &RuntimeSnapshot,
        event: RuntimeEvent,
        metadata: &TransitionMetadata,
    ) -> Result<RuntimeSnapshot, RuntimeError> {
        let (input_refs_json, output_refs_json, payload_json) = encode_metadata(metadata)?;
        let step = i64::try_from(expected.next_step)
            .map_err(|_| RuntimeError::Corrupt("next_step exceeds SQLite INTEGER".into()))?;
        let next_step = expected
            .next_step
            .checked_add(1)
            .ok_or_else(|| RuntimeError::Corrupt("next_step overflow".into()))?;
        let next_version = expected
            .version
            .checked_add(1)
            .ok_or_else(|| RuntimeError::Corrupt("version overflow".into()))?;

        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let current = load_validated_snapshot(&mut transaction, expected.run_id)
            .await?
            .ok_or_else(|| RuntimeError::Corrupt("runtime run does not exist".into()))?;
        authorize(access, &current.scope)?;
        if current != *expected {
            return Err(RuntimeError::ConcurrentModification);
        }
        let next_state = current.state.transition(event)?;
        let current_scope_key = scope_key(&current.scope)?;
        validate_evidence_refs(&mut transaction, &metadata.input_refs, &current_scope_key).await?;
        validate_evidence_refs(&mut transaction, &metadata.output_refs, &current_scope_key).await?;
        let now = Utc::now().timestamp_micros();
        let insert_result = sqlx::query(
            "INSERT INTO transitions (
                id, scope_key, trace_id, from_stage, to_stage, payload_json, created_at_us,
                run_id, step, input_refs_json, output_refs_json, attempt, deadline_at_us
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        )
        .bind(Uuid::now_v7().to_string())
        .bind(current_scope_key)
        .bind(metadata.trace_id.to_string())
        .bind(current.state.as_str())
        .bind(next_state.as_str())
        .bind(payload_json)
        .bind(now)
        .bind(current.run_id.to_string())
        .bind(step)
        .bind(input_refs_json)
        .bind(output_refs_json)
        .bind(i64::from(metadata.attempt))
        .bind(metadata.deadline.timestamp_micros())
        .execute(&mut *transaction)
        .await;
        if let Err(error) = insert_result {
            let _ = transaction.rollback().await;
            return Err(RuntimeError::Storage(error));
        }

        let update_result = sqlx::query(
            "UPDATE runtime_runs
             SET state = ?2, next_step = ?3, version = ?4, updated_at_us = ?5
             WHERE run_id = ?1 AND version = ?6",
        )
        .bind(current.run_id.to_string())
        .bind(next_state.as_str())
        .bind(
            i64::try_from(next_step)
                .map_err(|_| RuntimeError::Corrupt("next_step exceeds SQLite INTEGER".into()))?,
        )
        .bind(
            i64::try_from(next_version)
                .map_err(|_| RuntimeError::Corrupt("version exceeds SQLite INTEGER".into()))?,
        )
        .bind(now)
        .bind(
            i64::try_from(expected.version)
                .map_err(|_| RuntimeError::Corrupt("version exceeds SQLite INTEGER".into()))?,
        )
        .execute(&mut *transaction)
        .await;
        let updated = match update_result {
            Ok(result) => result,
            Err(error) => {
                let _ = transaction.rollback().await;
                return Err(RuntimeError::Storage(error));
            }
        };
        if updated.rows_affected() != 1 {
            let _ = transaction.rollback().await;
            return Err(RuntimeError::ConcurrentModification);
        }
        transaction.commit().await?;
        Ok(RuntimeSnapshot {
            run_id: current.run_id,
            scope: current.scope,
            state: next_state,
            next_step,
            version: next_version,
        })
    }
}

pub(crate) fn authorize(access: &AccessContext, scope: &MemoryScope) -> Result<(), RuntimeError> {
    match AclEngine.authorize(access, Operation::Mutate, scope) {
        Decision::Allow => Ok(()),
        Decision::Deny(reason) => Err(RuntimeError::AccessDenied(reason)),
    }
}

async fn validate_evidence_refs(
    transaction: &mut Transaction<'_, Sqlite>,
    references: &[EvidenceRef],
    scope_key: &str,
) -> Result<(), RuntimeError> {
    for evidence in references {
        validate_evidence_ref(transaction, evidence, scope_key).await?;
    }
    Ok(())
}

async fn validate_evidence_ref(
    transaction: &mut Transaction<'_, Sqlite>,
    evidence: &EvidenceRef,
    scope_key: &str,
) -> Result<(), RuntimeError> {
    if evidence.fragment_id.is_some() == evidence.event_id.is_some() {
        return Err(RuntimeError::InvalidEvidenceRef);
    }
    let row = sqlx::query(
        "SELECT er.referenced_fragment_id, er.referenced_event_id,
                er.verification_method, er.note,
                owner.scope_key AS owner_scope_key,
                CASE
                    WHEN er.referenced_fragment_id IS NOT NULL THEN fragment.scope_key
                    ELSE event.scope_key
                END AS target_scope_key
         FROM evidence_refs er
         JOIN fragments owner ON owner.id = er.owner_fragment_id
         LEFT JOIN fragments fragment ON fragment.id = er.referenced_fragment_id
         LEFT JOIN events event ON event.id = er.referenced_event_id
         WHERE er.id = ?1",
    )
    .bind(evidence.id.to_string())
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(RuntimeError::InvalidEvidenceRef)?;
    let fragment_id: Option<String> = row.try_get("referenced_fragment_id")?;
    let event_id: Option<String> = row.try_get("referenced_event_id")?;
    let method: String = row.try_get("verification_method")?;
    let note: Option<String> = row.try_get("note")?;
    let owner_scope_key: String = row.try_get("owner_scope_key")?;
    let target_scope_key: Option<String> = row.try_get("target_scope_key")?;
    let matches = fragment_id.as_deref()
        == evidence
            .fragment_id
            .as_ref()
            .map(ToString::to_string)
            .as_deref()
        && event_id.as_deref()
            == evidence
                .event_id
                .as_ref()
                .map(ToString::to_string)
                .as_deref()
        && method == verification_method(evidence.method)
        && note == evidence.note
        && owner_scope_key == scope_key
        && target_scope_key.as_deref() == Some(scope_key);
    if matches {
        Ok(())
    } else {
        Err(RuntimeError::InvalidEvidenceRef)
    }
}

const fn verification_method(method: VerificationMethod) -> &'static str {
    match method {
        VerificationMethod::Manual => "manual",
        VerificationMethod::Tool => "tool",
        VerificationMethod::ExternalSource => "external_source",
        VerificationMethod::CrossEvidence => "cross_evidence",
    }
}

fn encode_metadata(
    metadata: &TransitionMetadata,
) -> Result<(String, String, String), RuntimeError> {
    if metadata.attempt == 0 {
        return Err(RuntimeError::InvalidMetadata(
            "attempt must be greater than zero",
        ));
    }
    Ok((
        serde_json::to_string(&metadata.input_refs)?,
        serde_json::to_string(&metadata.output_refs)?,
        serde_json::to_string(&metadata.payload)?,
    ))
}

pub(crate) fn scope_key(scope: &MemoryScope) -> Result<String, RuntimeError> {
    let value = canonicalize(serde_json::to_value(scope)?);
    Ok(hex::encode(Sha256::digest(serde_json::to_vec(&value)?)))
}

fn canonicalize(value: Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.into_iter().map(canonicalize).collect()),
        Value::Object(values) => {
            let mut entries: Vec<_> = values.into_iter().collect();
            entries.sort_by(|left, right| left.0.cmp(&right.0));
            Value::Object(
                entries
                    .into_iter()
                    .map(|(key, value)| (key, canonicalize(value)))
                    .collect(),
            )
        }
        scalar => scalar,
    }
}
