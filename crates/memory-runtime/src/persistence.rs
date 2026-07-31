//! Runtime 控制检查点的 `SQLite` 持久化。

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use jc_memory_storage::SqliteStore;
use sqlx::Row as _;

use crate::{ControlState, RunId, RuntimeState};

/// 一次可跨重启恢复的 Runtime 控制检查点。
#[derive(Clone, Debug, PartialEq)]
pub struct RuntimeCheckpoint {
    /// 所属 Runtime run。
    pub run_id: RunId,
    /// 控制层状态。
    pub state: ControlState,
    /// 恢复后要回到的 Runtime 状态。
    pub resume_state: RuntimeState,
    /// 用户补充输入。
    pub user_input: Option<String>,
    /// 下一次允许尝试的时间。
    pub next_attempt_at: Option<SystemTime>,
    /// 当前尝试次数。
    pub attempt: u32,
}

/// Runtime 检查点持久化错误。
#[derive(Debug, thiserror::Error)]
pub enum RuntimeCheckpointError {
    /// `SQLite` 操作失败。
    #[error("runtime checkpoint database failure: {0}")]
    Storage(#[from] sqlx::Error),
    /// 检查点数据无法解码。
    #[error("invalid runtime checkpoint: {0}")]
    Invalid(String),
}

/// 基于 `SQLite` 的 Runtime 检查点存储。
#[derive(Clone, Debug)]
pub struct RuntimeCheckpointStore {
    pool: sqlx::SqlitePool,
}

impl RuntimeCheckpointStore {
    /// 从现有 Store 创建检查点存储。
    #[must_use]
    pub fn from_store(store: &SqliteStore) -> Self {
        Self {
            pool: store.pool().clone(),
        }
    }

    /// 原子保存一个检查点。
    ///
    /// # Errors
    ///
    /// run 不存在、字段无法编码或 `SQLite` 写入失败时返回错误。
    pub async fn save(&self, checkpoint: &RuntimeCheckpoint) -> Result<(), RuntimeCheckpointError> {
        let next_attempt_at_us = checkpoint
            .next_attempt_at
            .map(system_time_to_micros)
            .transpose()
            .map_err(RuntimeCheckpointError::Invalid)?;
        sqlx::query(
            "INSERT INTO runtime_control_checkpoints
                (run_id, control_state, resume_state, user_input, next_attempt_at_us, attempt, updated_at_us)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(run_id) DO UPDATE SET
                control_state = excluded.control_state,
                resume_state = excluded.resume_state,
                user_input = excluded.user_input,
                next_attempt_at_us = excluded.next_attempt_at_us,
                attempt = excluded.attempt,
                updated_at_us = excluded.updated_at_us",
        )
        .bind(checkpoint.run_id.to_string())
        .bind(checkpoint.state.as_str())
        .bind(checkpoint.resume_state.as_str())
        .bind(&checkpoint.user_input)
        .bind(next_attempt_at_us)
        .bind(i64::from(checkpoint.attempt))
        .bind(chrono::Utc::now().timestamp_micros())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// 读取指定 run 的最新检查点。
    ///
    /// # Errors
    ///
    /// `SQLite` 读取失败或持久化字段无法解码时返回错误。
    pub async fn load(
        &self,
        run_id: RunId,
    ) -> Result<Option<RuntimeCheckpoint>, RuntimeCheckpointError> {
        let row = sqlx::query(
            "SELECT control_state, resume_state, user_input, next_attempt_at_us, attempt
             FROM runtime_control_checkpoints WHERE run_id = ?1",
        )
        .bind(run_id.to_string())
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let state: String = row.try_get("control_state")?;
        let state = ControlState::parse(&state)
            .ok_or_else(|| RuntimeCheckpointError::Invalid("unknown control state".into()))?;
        let resume_state: String = row.try_get("resume_state")?;
        let resume_state = RuntimeState::parse(&resume_state)
            .ok_or_else(|| RuntimeCheckpointError::Invalid("unknown resume state".into()))?;
        let attempt: i64 = row.try_get("attempt")?;
        let attempt = u32::try_from(attempt)
            .map_err(|_| RuntimeCheckpointError::Invalid("attempt out of range".into()))?;
        let next_attempt_at_us: Option<i64> = row.try_get("next_attempt_at_us")?;
        let next_attempt_at = next_attempt_at_us
            .map(micros_to_system_time)
            .transpose()
            .map_err(RuntimeCheckpointError::Invalid)?;
        Ok(Some(RuntimeCheckpoint {
            run_id,
            state,
            resume_state,
            user_input: row.try_get("user_input")?,
            next_attempt_at,
            attempt,
        }))
    }
}

fn system_time_to_micros(value: SystemTime) -> Result<i64, String> {
    let duration = value
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("time before Unix epoch: {error}"))?;
    i64::try_from(duration.as_micros()).map_err(|_| "timestamp exceeds SQLite INTEGER".into())
}

fn micros_to_system_time(value: i64) -> Result<SystemTime, String> {
    let micros = u64::try_from(value).map_err(|_| "negative timestamp".to_owned())?;
    Ok(UNIX_EPOCH + Duration::from_micros(micros))
}
