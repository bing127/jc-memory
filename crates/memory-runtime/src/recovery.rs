//! 从最近一次事务提交恢复 Runtime run。

use std::str::FromStr as _;

use jc_memory_protocol::MemoryScope;
use jc_memory_storage::SqliteStore;
use sqlx::{Row as _, Sqlite, SqlitePool, Transaction, sqlite::SqliteRow};

use crate::{
    RunId, RuntimeError, RuntimeSnapshot, RuntimeState,
    runner::{authorize, scope_key},
};

/// 读取已提交 Runtime 快照的恢复入口。
#[derive(Clone, Debug)]
pub struct RuntimeRecovery {
    pool: SqlitePool,
}

impl RuntimeRecovery {
    /// 从现有 Store 构造恢复入口。
    #[must_use]
    pub fn from_store(store: &SqliteStore) -> Self {
        Self {
            pool: store.pool().clone(),
        }
    }

    /// 恢复指定 run 最近一次已提交状态与下一步骤号。
    ///
    /// # Errors
    ///
    /// 授权失败、数据库读取失败或持久化历史无法验证时返回错误。
    pub async fn recover(
        &self,
        access: &jc_memory_protocol::AccessContext,
        run_id: RunId,
    ) -> Result<Option<RuntimeSnapshot>, RuntimeError> {
        let mut transaction = self.pool.begin().await?;
        let snapshot = load_validated_snapshot(&mut transaction, run_id).await?;
        if let Some(snapshot) = &snapshot {
            authorize(access, &snapshot.scope)?;
        }
        transaction.commit().await?;
        Ok(snapshot)
    }
}

pub(crate) async fn load_validated_snapshot(
    transaction: &mut Transaction<'_, Sqlite>,
    run_id: RunId,
) -> Result<Option<RuntimeSnapshot>, RuntimeError> {
    let row = sqlx::query(
        "SELECT run_id, scope_key, scope_json, state, next_step, version
             FROM runtime_runs WHERE run_id = ?1",
    )
    .bind(run_id.to_string())
    .fetch_optional(&mut **transaction)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let snapshot = decode_snapshot(&row)?;
    validate_history(transaction, &snapshot).await?;
    Ok(Some(snapshot))
}

async fn validate_history(
    transaction: &mut Transaction<'_, Sqlite>,
    snapshot: &RuntimeSnapshot,
) -> Result<(), RuntimeError> {
    if snapshot.version != snapshot.next_step {
        return Err(RuntimeError::Corrupt(
            "runtime version does not match next_step".into(),
        ));
    }
    let rows = sqlx::query(
        "SELECT step, from_stage, to_stage, scope_key
         FROM transitions WHERE run_id = ?1 ORDER BY step",
    )
    .bind(snapshot.run_id.to_string())
    .fetch_all(&mut **transaction)
    .await?;
    let expected_count = usize::try_from(snapshot.next_step)
        .map_err(|_| RuntimeError::Corrupt("next_step exceeds usize".into()))?;
    if rows.len() != expected_count {
        return Err(RuntimeError::Corrupt(
            "runtime transition count does not match next_step".into(),
        ));
    }
    let expected_scope_key = scope_key(&snapshot.scope)?;
    let mut expected_from = RuntimeState::Observing.as_str();
    for (expected_step, row) in rows.iter().enumerate() {
        let step: i64 = row.try_get("step")?;
        let from_stage: String = row.try_get("from_stage")?;
        let to_stage: String = row.try_get("to_stage")?;
        let stored_scope_key: String = row.try_get("scope_key")?;
        let parsed_states = (
            RuntimeState::parse(&from_stage),
            RuntimeState::parse(&to_stage),
        );
        if step != i64::try_from(expected_step).unwrap_or(i64::MAX)
            || from_stage != expected_from
            || stored_scope_key != expected_scope_key
            || !matches!(parsed_states, (Some(from), Some(to)) if valid_state_edge(from, to))
        {
            return Err(RuntimeError::Corrupt(
                "runtime transition history is not contiguous".into(),
            ));
        }
        expected_from = parsed_states.1.expect("to_stage was validated").as_str();
    }
    if expected_from != snapshot.state.as_str() {
        return Err(RuntimeError::Corrupt(
            "runtime snapshot state does not match transition history".into(),
        ));
    }
    Ok(())
}

const fn valid_state_edge(from: RuntimeState, to: RuntimeState) -> bool {
    matches!(
        (from, to),
        (
            RuntimeState::Observing,
            RuntimeState::Recalling | RuntimeState::Failed | RuntimeState::Cancelled
        ) | (
            RuntimeState::Recalling,
            RuntimeState::Reasoning | RuntimeState::Failed | RuntimeState::Cancelled
        ) | (
            RuntimeState::Reasoning,
            RuntimeState::AwaitingApproval
                | RuntimeState::Executing
                | RuntimeState::WaitingUser
                | RuntimeState::Failed
                | RuntimeState::Cancelled
        ) | (
            RuntimeState::AwaitingApproval,
            RuntimeState::Executing | RuntimeState::Failed | RuntimeState::Cancelled
        ) | (
            RuntimeState::Executing,
            RuntimeState::Committing | RuntimeState::Failed | RuntimeState::Cancelled
        ) | (
            RuntimeState::Committing,
            RuntimeState::Completed | RuntimeState::Failed | RuntimeState::Cancelled
        ) | (
            RuntimeState::WaitingUser | RuntimeState::Paused,
            RuntimeState::Failed | RuntimeState::Cancelled
        )
    )
}

pub(crate) fn decode_snapshot(row: &SqliteRow) -> Result<RuntimeSnapshot, RuntimeError> {
    let raw_run_id: String = row.try_get("run_id")?;
    let run_id = RunId::from_str(&raw_run_id)
        .map_err(|error| RuntimeError::Corrupt(format!("invalid run_id {raw_run_id}: {error}")))?;
    let raw_scope: String = row.try_get("scope_json")?;
    let scope: MemoryScope = serde_json::from_str(&raw_scope)
        .map_err(|error| RuntimeError::Corrupt(format!("invalid persisted scope_json: {error}")))?;
    let stored_scope_key: String = row.try_get("scope_key")?;
    if stored_scope_key != scope_key(&scope)? {
        return Err(RuntimeError::Corrupt(
            "scope_json does not match scope_key".into(),
        ));
    }
    let raw_state: String = row.try_get("state")?;
    let state = RuntimeState::parse(&raw_state)
        .ok_or_else(|| RuntimeError::Corrupt(format!("unknown runtime state: {raw_state}")))?;
    let raw_next_step: i64 = row.try_get("next_step")?;
    let next_step = u64::try_from(raw_next_step)
        .map_err(|_| RuntimeError::Corrupt(format!("invalid next_step: {raw_next_step}")))?;
    let raw_version: i64 = row.try_get("version")?;
    let version = u64::try_from(raw_version)
        .map_err(|_| RuntimeError::Corrupt(format!("invalid version: {raw_version}")))?;
    Ok(RuntimeSnapshot {
        run_id,
        scope,
        state,
        next_step,
        version,
    })
}
