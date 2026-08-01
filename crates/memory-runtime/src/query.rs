//! Runtime Run、Transition 与控制检查点的有界只读查询。

use std::str::FromStr as _;

use jc_memory_protocol::{
    AccessContext, Decision, DenyReason, EvidenceRef, MemoryScope, Operation,
};
use jc_memory_security::AclEngine;
use jc_memory_storage::SqliteStore;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{Row as _, sqlite::SqliteRow};

use crate::{ControlState, RunId, RuntimeState, runner::scope_key};

/// Runtime 列表的有界过滤条件。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RuntimeRunQuery {
    /// 可选的精确状态过滤。
    pub state: Option<RuntimeState>,
    /// 当前页最多返回的 Run 数量，范围为 1..=100。
    pub limit: u32,
    /// 从排序结果起点跳过的 Run 数量。
    pub offset: u32,
}

impl Default for RuntimeRunQuery {
    fn default() -> Self {
        Self {
            state: None,
            limit: 25,
            offset: 0,
        }
    }
}

/// 已持久化的 Runtime 控制检查点，不复制权威 Memory 正文。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RuntimeControlRecord {
    /// 当前控制状态。
    pub state: ControlState,
    /// 恢复时返回的 Runtime 状态。
    pub resume_state: RuntimeState,
    /// 等待用户时保存的输入。
    pub user_input: Option<String>,
    /// 下一次允许尝试的 Unix 微秒时间。
    pub next_attempt_at_us: Option<i64>,
    /// 当前尝试次数。
    pub attempt: u32,
    /// 检查点更新时间。
    pub updated_at_us: i64,
}

/// Runtime Run 列表项。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RuntimeRunRecord {
    /// Run 标识。
    pub run_id: RunId,
    /// 精确所有权 Scope。
    pub scope: MemoryScope,
    /// 最近一次已提交状态。
    pub state: RuntimeState,
    /// 下一次转换步骤号。
    pub next_step: u64,
    /// 乐观并发版本。
    pub version: u64,
    /// 创建时间，Unix 微秒。
    pub created_at_us: i64,
    /// 更新时间，Unix 微秒。
    pub updated_at_us: i64,
    /// 可选控制检查点。
    pub control: Option<RuntimeControlRecord>,
}

/// 一个 Runtime Transition 的结构化只读记录。
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct RuntimeTransitionRecord {
    /// Transition 标识。
    pub id: String,
    /// 所属 Run。
    pub run_id: RunId,
    /// Run 内单调步骤号。
    pub step: u64,
    /// Trace 标识。
    pub trace_id: String,
    /// 转换前状态。
    pub from_state: RuntimeState,
    /// 转换后状态。
    pub to_state: RuntimeState,
    /// 输入权威证据引用。
    pub input_refs: Vec<EvidenceRef>,
    /// 输出权威证据引用。
    pub output_refs: Vec<EvidenceRef>,
    /// 当前尝试次数。
    pub attempt: u32,
    /// 截止时间，Unix 微秒。
    pub deadline_at_us: i64,
    /// 结构化转换载荷。
    pub payload: Value,
    /// 创建时间，Unix 微秒。
    pub created_at_us: i64,
}

/// Runtime Run 分页结果。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RuntimeRunPage {
    /// 当前页 Run。
    pub items: Vec<RuntimeRunRecord>,
    /// 过滤后的总数。
    pub total: u64,
    /// 当前页上限。
    pub limit: u32,
    /// 当前偏移。
    pub offset: u32,
}

/// Runtime Run 与完整 Transition 时间线。
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct RuntimeRunDetail {
    /// Run 当前状态。
    pub run: RuntimeRunRecord,
    /// 按 step 升序排列的不可变 Transition。
    pub transitions: Vec<RuntimeTransitionRecord>,
}

/// Runtime 只读查询错误。
#[derive(Debug, thiserror::Error)]
pub enum RuntimeQueryError {
    /// `SQLite` 查询失败。
    #[error("runtime query database failure: {0}")]
    Storage(#[from] sqlx::Error),
    /// 调用方缺少精确 Scope 的 Recall Grant。
    #[error("runtime query access denied: {0:?}")]
    AccessDenied(DenyReason),
    /// 页参数超过服务边界。
    #[error("invalid runtime query: {0}")]
    Invalid(&'static str),
    /// 持久化数据无法安全解码。
    #[error("corrupt runtime query row: {0}")]
    Corrupt(String),
}

/// 使用 Runtime 持久化表的有界只读仓储。
#[derive(Clone, Debug)]
pub struct RuntimeReadStore {
    pool: sqlx::SqlitePool,
}

impl RuntimeReadStore {
    /// 从现有 Memory `SQLite` Store 创建查询仓储。
    #[must_use]
    pub fn from_store(store: &SqliteStore) -> Self {
        Self {
            pool: store.pool().clone(),
        }
    }

    /// 按精确 Scope 返回有界 Run 列表。
    ///
    /// # Errors
    ///
    /// 缺少 Recall Grant、页参数无效、数据库失败或持久化行损坏时返回错误。
    pub async fn list(
        &self,
        access: &AccessContext,
        scope: &MemoryScope,
        query: RuntimeRunQuery,
    ) -> Result<RuntimeRunPage, RuntimeQueryError> {
        authorize(access, scope)?;
        if !(1..=100).contains(&query.limit) {
            return Err(RuntimeQueryError::Invalid("limit must be in 1..=100"));
        }
        if query.offset > 1_000_000 {
            return Err(RuntimeQueryError::Invalid("offset exceeds 1000000"));
        }
        let scope_key =
            scope_key(scope).map_err(|error| RuntimeQueryError::Corrupt(error.to_string()))?;
        let state = query.state.map(RuntimeState::as_str);
        let total: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM runtime_runs
             WHERE scope_key = ?1 AND (?2 IS NULL OR state = ?2)",
        )
        .bind(&scope_key)
        .bind(state)
        .fetch_one(&self.pool)
        .await?;
        let rows = sqlx::query(
            "SELECT r.run_id, r.scope_json, r.state, r.next_step, r.version,
                    r.created_at_us, r.updated_at_us,
                    c.control_state, c.resume_state, c.user_input,
                    c.next_attempt_at_us, c.attempt AS control_attempt,
                    c.updated_at_us AS control_updated_at_us
             FROM runtime_runs r
             LEFT JOIN runtime_control_checkpoints c ON c.run_id = r.run_id
             WHERE r.scope_key = ?1 AND (?2 IS NULL OR r.state = ?2)
             ORDER BY r.updated_at_us DESC, r.run_id DESC
             LIMIT ?3 OFFSET ?4",
        )
        .bind(scope_key)
        .bind(state)
        .bind(i64::from(query.limit))
        .bind(i64::from(query.offset))
        .fetch_all(&self.pool)
        .await?;
        let items = rows
            .iter()
            .map(|row| decode_run(row, scope))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(RuntimeRunPage {
            items,
            total: nonnegative_u64(total, "total")?,
            limit: query.limit,
            offset: query.offset,
        })
    }

    /// 返回一个 Scope 内 Run 的完整 Transition 时间线。
    ///
    /// # Errors
    ///
    /// 与 [`Self::list`] 相同；目标不存在或不属于 Scope 时返回 `Ok(None)`。
    pub async fn get(
        &self,
        access: &AccessContext,
        scope: &MemoryScope,
        run_id: RunId,
    ) -> Result<Option<RuntimeRunDetail>, RuntimeQueryError> {
        authorize(access, scope)?;
        let scope_key =
            scope_key(scope).map_err(|error| RuntimeQueryError::Corrupt(error.to_string()))?;
        let row = sqlx::query(
            "SELECT r.run_id, r.scope_json, r.state, r.next_step, r.version,
                    r.created_at_us, r.updated_at_us,
                    c.control_state, c.resume_state, c.user_input,
                    c.next_attempt_at_us, c.attempt AS control_attempt,
                    c.updated_at_us AS control_updated_at_us
             FROM runtime_runs r
             LEFT JOIN runtime_control_checkpoints c ON c.run_id = r.run_id
             WHERE r.run_id = ?1 AND r.scope_key = ?2",
        )
        .bind(run_id.to_string())
        .bind(&scope_key)
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let run = decode_run(&row, scope)?;
        let rows = sqlx::query(
            "SELECT id, run_id, step, trace_id, from_stage, to_stage,
                    input_refs_json, output_refs_json, attempt, deadline_at_us,
                    payload_json, created_at_us
             FROM transitions
             WHERE run_id = ?1 AND scope_key = ?2
             ORDER BY step ASC",
        )
        .bind(run_id.to_string())
        .bind(scope_key)
        .fetch_all(&self.pool)
        .await?;
        let transitions = rows
            .iter()
            .map(decode_transition)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Some(RuntimeRunDetail { run, transitions }))
    }
}

fn authorize(access: &AccessContext, scope: &MemoryScope) -> Result<(), RuntimeQueryError> {
    match AclEngine.authorize(access, Operation::Recall, scope) {
        Decision::Allow => Ok(()),
        Decision::Deny(reason) => Err(RuntimeQueryError::AccessDenied(reason)),
    }
}

fn decode_run(
    row: &SqliteRow,
    expected_scope: &MemoryScope,
) -> Result<RuntimeRunRecord, RuntimeQueryError> {
    let run_id_value: String = row.try_get("run_id")?;
    let run_id = parse_run_id(&run_id_value)?;
    let scope_json: String = row.try_get("scope_json")?;
    let scope: MemoryScope = serde_json::from_str(&scope_json)
        .map_err(|error| RuntimeQueryError::Corrupt(format!("invalid scope: {error}")))?;
    if &scope != expected_scope {
        return Err(RuntimeQueryError::Corrupt(
            "scope key does not match scope JSON".into(),
        ));
    }
    let state_value: String = row.try_get("state")?;
    let state = parse_state(&state_value)?;
    let control_state: Option<String> = row.try_get("control_state")?;
    let control = control_state
        .map(|value| decode_control(row, &value))
        .transpose()?;
    Ok(RuntimeRunRecord {
        run_id,
        scope,
        state,
        next_step: nonnegative_u64(row.try_get("next_step")?, "next_step")?,
        version: nonnegative_u64(row.try_get("version")?, "version")?,
        created_at_us: row.try_get("created_at_us")?,
        updated_at_us: row.try_get("updated_at_us")?,
        control,
    })
}

fn decode_control(row: &SqliteRow, state: &str) -> Result<RuntimeControlRecord, RuntimeQueryError> {
    let state = ControlState::parse(state)
        .ok_or_else(|| RuntimeQueryError::Corrupt("unknown control state".into()))?;
    let resume_state_value: String = row.try_get("resume_state")?;
    let resume_state = parse_state(&resume_state_value)?;
    Ok(RuntimeControlRecord {
        state,
        resume_state,
        user_input: row.try_get("user_input")?,
        next_attempt_at_us: row.try_get("next_attempt_at_us")?,
        attempt: nonnegative_u32(row.try_get("control_attempt")?, "control attempt")?,
        updated_at_us: row.try_get("control_updated_at_us")?,
    })
}

fn decode_transition(row: &SqliteRow) -> Result<RuntimeTransitionRecord, RuntimeQueryError> {
    let input_refs_json: String = row.try_get("input_refs_json")?;
    let output_refs_json: String = row.try_get("output_refs_json")?;
    let payload_json: String = row.try_get("payload_json")?;
    let run_id_value: String = row.try_get("run_id")?;
    let from_state_value: String = row.try_get("from_stage")?;
    let to_state_value: String = row.try_get("to_stage")?;
    Ok(RuntimeTransitionRecord {
        id: row.try_get("id")?,
        run_id: parse_run_id(&run_id_value)?,
        step: nonnegative_u64(row.try_get("step")?, "step")?,
        trace_id: row.try_get("trace_id")?,
        from_state: parse_state(&from_state_value)?,
        to_state: parse_state(&to_state_value)?,
        input_refs: serde_json::from_str(&input_refs_json)
            .map_err(|error| RuntimeQueryError::Corrupt(format!("invalid input refs: {error}")))?,
        output_refs: serde_json::from_str(&output_refs_json)
            .map_err(|error| RuntimeQueryError::Corrupt(format!("invalid output refs: {error}")))?,
        attempt: nonnegative_u32(row.try_get("attempt")?, "attempt")?,
        deadline_at_us: row.try_get("deadline_at_us")?,
        payload: serde_json::from_str(&payload_json)
            .map_err(|error| RuntimeQueryError::Corrupt(format!("invalid payload: {error}")))?,
        created_at_us: row.try_get("created_at_us")?,
    })
}

fn parse_run_id(value: &str) -> Result<RunId, RuntimeQueryError> {
    RunId::from_str(value)
        .map_err(|error| RuntimeQueryError::Corrupt(format!("invalid run ID: {error}")))
}

fn parse_state(value: &str) -> Result<RuntimeState, RuntimeQueryError> {
    RuntimeState::parse(value)
        .ok_or_else(|| RuntimeQueryError::Corrupt(format!("unknown runtime state: {value}")))
}

fn nonnegative_u64(value: i64, field: &str) -> Result<u64, RuntimeQueryError> {
    u64::try_from(value).map_err(|_| RuntimeQueryError::Corrupt(format!("{field} is negative")))
}

fn nonnegative_u32(value: i64, field: &str) -> Result<u32, RuntimeQueryError> {
    u32::try_from(value).map_err(|_| RuntimeQueryError::Corrupt(format!("{field} is out of range")))
}
