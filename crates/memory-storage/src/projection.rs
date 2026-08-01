use std::{fmt, sync::Arc};

use chrono::{DateTime, Utc};
use jc_memory_protocol::MemoryScope;
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::{Row, SqlitePool};

use crate::{SqliteStore, StorageError};

/// 可独立追赶的持久化投影类别。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProjectorKind {
    /// 显式状态版本投影。
    State,
    /// Point 与 Edge 图投影。
    Graph,
}

impl ProjectorKind {
    /// 返回持久化到 Checkpoint 表中的稳定标识。
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::State => "state",
            Self::Graph => "graph",
        }
    }
}

impl fmt::Display for ProjectorKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// 一个 Projector 在精确 Scope 内已提交的位置。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectionCheckpoint {
    /// Checkpoint 所属的投影类别。
    pub projector: ProjectorKind,
    /// 已完整提交的最大 Event sequence。
    pub sequence: u64,
    /// Checkpoint 最后更新时间。
    pub updated_at: DateTime<Utc>,
}

/// 一次 Recall 固定追赶到的 Event sequence。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProjectionTarget {
    /// 追赶开始时 Scope 内可见的最大 Event sequence。
    pub sequence: u64,
}

/// 用于证明投影事务在提交前完整回滚的故障边界。
pub trait ProjectionFailpoint: Send + Sync {
    /// 在派生行与 Checkpoint 写入后、事务提交前运行。
    ///
    /// # Errors
    ///
    /// 返回错误时调用方必须回滚整个投影事务。
    fn before_commit(
        &self,
        projector: ProjectorKind,
        next_sequence: u64,
    ) -> Result<(), StorageError>;
}

#[derive(Debug)]
struct NoProjectionFailpoint;

impl ProjectionFailpoint for NoProjectionFailpoint {
    fn before_commit(
        &self,
        _projector: ProjectorKind,
        _next_sequence: u64,
    ) -> Result<(), StorageError> {
        Ok(())
    }
}

/// 共享 `SQLite` 连接池的 State 与 Graph 投影存储。
#[derive(Clone)]
pub struct SqliteProjectionStore {
    pool: SqlitePool,
    failpoint: Arc<dyn ProjectionFailpoint>,
}

impl fmt::Debug for SqliteProjectionStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SqliteProjectionStore")
            .field("pool", &self.pool)
            .field("failpoint_refs", &Arc::strong_count(&self.failpoint))
            .finish()
    }
}

impl SqliteProjectionStore {
    /// 从现有 `SQLite` 连接池创建投影存储。
    #[must_use]
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            pool,
            failpoint: Arc::new(NoProjectionFailpoint),
        }
    }

    /// 共享已经打开的 `SqliteStore` 连接池。
    #[must_use]
    pub fn from_store(store: &SqliteStore) -> Self {
        Self::new(store.pool().clone())
    }

    /// 替换事务提交前的确定性故障钩子。
    #[must_use]
    pub fn with_failpoint(mut self, failpoint: Arc<dyn ProjectionFailpoint>) -> Self {
        self.failpoint = failpoint;
        self
    }

    /// 固定当前精确 Scope 的最大权威 Event sequence。
    ///
    /// # Errors
    ///
    /// Scope 无法 canonical 序列化、查询失败或 sequence 损坏时返回错误。
    pub async fn target(&self, scope: &MemoryScope) -> Result<ProjectionTarget, StorageError> {
        let scope_hash = canonical_scope_key(scope)?;
        let sequence: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(sequence), 0) FROM events WHERE scope_key = ?1",
        )
        .bind(&scope_hash)
        .fetch_one(&self.pool)
        .await?;
        let sequence =
            u64::try_from(sequence).map_err(|_| StorageError::ProjectionEventCorrupt {
                projector: ProjectorKind::State,
                field: "sequence",
                scope_hash,
            })?;
        Ok(ProjectionTarget { sequence })
    }

    /// 读取一个 Projector 在精确 Scope 内的已提交位置。
    ///
    /// # Errors
    ///
    /// Scope 无法 canonical 序列化、查询失败或 Checkpoint 损坏时返回错误。
    pub async fn checkpoint(
        &self,
        scope: &MemoryScope,
        projector: ProjectorKind,
    ) -> Result<ProjectionCheckpoint, StorageError> {
        let scope_hash = canonical_scope_key(scope)?;
        let row = sqlx::query(
            "SELECT sequence, updated_at_us
             FROM projection_checkpoints
             WHERE scope_key = ?1 AND projector = ?2",
        )
        .bind(&scope_hash)
        .bind(projector.as_str())
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else {
            return Ok(ProjectionCheckpoint {
                projector,
                sequence: 0,
                updated_at: DateTime::<Utc>::UNIX_EPOCH,
            });
        };
        let sequence = row.try_get::<i64, _>("sequence")?;
        let updated_at_us = row.try_get::<i64, _>("updated_at_us")?;
        let sequence =
            u64::try_from(sequence).map_err(|_| StorageError::ProjectionCheckpointCorrupt {
                projector,
                scope_hash: scope_hash.clone(),
            })?;
        let updated_at = DateTime::from_timestamp_micros(updated_at_us).ok_or_else(|| {
            StorageError::ProjectionCheckpointCorrupt {
                projector,
                scope_hash,
            }
        })?;
        Ok(ProjectionCheckpoint {
            projector,
            sequence,
            updated_at,
        })
    }
}

fn canonical_scope_key(scope: &MemoryScope) -> Result<String, StorageError> {
    let value = serde_json::to_value(scope)?;
    let bytes = serde_json::to_vec(&canonicalize(value))?;
    Ok(hex::encode(Sha256::digest(bytes)))
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
