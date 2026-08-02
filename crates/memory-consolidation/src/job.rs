//! 持久化 Job 状态、Lease 与 Checkpoint 仓储。

use std::{fmt, str::FromStr, time::Duration};

use chrono::Utc;
use jc_memory_protocol::MemoryScope;
use jc_memory_storage::SqliteStore;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest as _, Sha256};
use sqlx::{Row as _, Sqlite, SqlitePool, Transaction};
use uuid::Uuid;

use crate::{BatchOutcome, checkpoint::BatchUsage};

/// 一个持久化巩固 Job 的强类型标识。
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct JobId(Uuid);

impl JobId {
    /// 创建 UUID v7 Job 标识。
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl Default for JobId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for JobId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl FromStr for JobId {
    type Err = uuid::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Uuid::parse_str(value).map(Self)
    }
}

/// M3 支持的巩固任务类别。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JobKind {
    /// 重放权威 Event。
    Replay,
    /// 创建保守去重关系。
    Deduplication,
    /// 增强有证据支持的 Edge。
    EdgeReinforcement,
    /// 弱化被失败证据反驳的 Edge。
    EdgeWeakening,
    /// 发现新的 Surface 候选。
    SurfaceDiscovery,
    /// 合并 Surface 版本。
    SurfaceMerge,
    /// 分裂 Surface 版本。
    SurfaceSplit,
    /// 提升达到证据门槛的语义项。
    SemanticPromotion,
    /// 检测冲突证据。
    ConflictDetection,
    /// 检测陈旧状态。
    StaleStateDetection,
    /// 移动冷数据层级。
    ColdTierMovement,
    /// 重建可派生结构。
    Rebuild,
}

impl JobKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Replay => "replay",
            Self::Deduplication => "deduplication",
            Self::EdgeReinforcement => "edge_reinforcement",
            Self::EdgeWeakening => "edge_weakening",
            Self::SurfaceDiscovery => "surface_discovery",
            Self::SurfaceMerge => "surface_merge",
            Self::SurfaceSplit => "surface_split",
            Self::SemanticPromotion => "semantic_promotion",
            Self::ConflictDetection => "conflict_detection",
            Self::StaleStateDetection => "stale_state_detection",
            Self::ColdTierMovement => "cold_tier_movement",
            Self::Rebuild => "rebuild",
        }
    }

    fn parse(value: &str) -> Result<Self, JobError> {
        match value {
            "replay" => Ok(Self::Replay),
            "deduplication" => Ok(Self::Deduplication),
            "edge_reinforcement" => Ok(Self::EdgeReinforcement),
            "edge_weakening" => Ok(Self::EdgeWeakening),
            "surface_discovery" => Ok(Self::SurfaceDiscovery),
            "surface_merge" => Ok(Self::SurfaceMerge),
            "surface_split" => Ok(Self::SurfaceSplit),
            "semantic_promotion" => Ok(Self::SemanticPromotion),
            "conflict_detection" => Ok(Self::ConflictDetection),
            "stale_state_detection" => Ok(Self::StaleStateDetection),
            "cold_tier_movement" => Ok(Self::ColdTierMovement),
            "rebuild" => Ok(Self::Rebuild),
            _ => Err(JobError::Corrupt(format!("unknown job kind: {value}"))),
        }
    }
}

/// 持久化 Job 状态机。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    /// 等待 Worker 获取 Lease。
    Pending,
    /// Worker 正在处理。
    Running,
    /// 已暂停，可显式恢复。
    Paused,
    /// 失败后等待重试。
    Retrying,
    /// 已处理全部批次。
    Completed,
    /// 已超过重试上限。
    Failed,
    /// 已取消且不得继续处理。
    Cancelled,
}

impl JobStatus {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Retrying => "retrying",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    fn parse(value: &str) -> Result<Self, JobError> {
        match value {
            "pending" => Ok(Self::Pending),
            "running" => Ok(Self::Running),
            "paused" => Ok(Self::Paused),
            "retrying" => Ok(Self::Retrying),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err(JobError::Corrupt(format!("unknown job status: {value}"))),
        }
    }
}

/// 每个 Job 的硬资源预算。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ResourceLimits {
    /// 单批最多处理的条目数。
    pub max_batch_size: u32,
    /// Job 累计执行时间预算；Runner 以批次 elapsed 作为保守 CPU 代理。
    pub max_cpu_ms: u64,
    /// Job 累计 IO 操作预算。
    pub max_io_ops: u64,
    /// 单次 Runner 执行的墙钟预算毫秒数。
    pub max_runtime_ms: u64,
    /// 允许记录的最大失败次数。
    pub max_attempts: u32,
}

impl ResourceLimits {
    fn validate(self) -> Result<(), JobError> {
        if self.max_batch_size == 0 {
            return Err(JobError::InvalidSpec("max_batch_size"));
        }
        if self.max_cpu_ms == 0 {
            return Err(JobError::InvalidSpec("max_cpu_ms"));
        }
        if self.max_io_ops == 0 {
            return Err(JobError::InvalidSpec("max_io_ops"));
        }
        if self.max_runtime_ms == 0 {
            return Err(JobError::InvalidSpec("max_runtime_ms"));
        }
        if self.max_attempts == 0 {
            return Err(JobError::InvalidSpec("max_attempts"));
        }
        Ok(())
    }
}

/// 提交一个巩固 Job 所需的完整规格。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct JobSpec {
    /// Handler 类别。
    pub kind: JobKind,
    /// 精确所有权 Scope。
    pub scope: MemoryScope,
    /// 首次运行的游标。
    pub cursor: u64,
    /// 硬资源预算。
    pub limits: ResourceLimits,
}

/// 一个 Worker 对 Job 的限时独占权。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Lease {
    /// Worker 稳定标识。
    pub worker_id: String,
    /// Lease 获取时间。
    pub acquired_at_us: i64,
    /// Lease 失效时间。
    pub expires_at_us: i64,
}

/// 只在批次提交成功后推进的持久化 Checkpoint。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct JobCheckpoint {
    /// Job 的精确 Scope，用于从现有表结构恢复规格。
    pub scope: MemoryScope,
    /// 最近一次成功提交批次之后的游标。
    pub cursor: u64,
    /// 已成功提交的批次数。
    pub committed_batches: u64,
    /// 已成功处理的条目总数。
    pub processed_items: u64,
    /// 已提交批次报告的累计 CPU 毫秒数。
    pub cpu_ms: u64,
    /// 已提交批次报告的累计 IO 操作数。
    pub io_ops: u64,
    /// 当前 Lease；非 Running 状态通常为空。
    pub lease: Option<Lease>,
    /// Retrying 状态允许再次执行的最早时间。
    pub next_retry_at_us: Option<i64>,
}

/// 一个完整的持久化 Job 视图。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JobRecord {
    /// Job 标识。
    pub id: JobId,
    /// 当前规格；cursor 与 Checkpoint 保持一致。
    pub spec: JobSpec,
    /// 当前状态。
    pub status: JobStatus,
    /// 最近成功提交的 Checkpoint。
    pub checkpoint: JobCheckpoint,
    /// 已记录失败次数。
    pub attempts: u32,
    /// 创建时间微秒时间戳。
    pub created_at_us: i64,
    /// 最近更新时间微秒时间戳。
    pub updated_at_us: i64,
}

/// Job Runner 和持久化状态机错误。
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum JobError {
    /// `SQLite` 操作失败。
    #[error("consolidation database failure: {0}")]
    Database(String),
    /// JSON 编解码失败。
    #[error("consolidation serialization failure: {0}")]
    Serialization(String),
    /// Job 不存在。
    #[error("consolidation job {0} was not found")]
    NotFound(JobId),
    /// 持久化行无法安全解码。
    #[error("corrupt consolidation job: {0}")]
    Corrupt(String),
    /// Job 规格字段不合法。
    #[error("invalid consolidation job spec: {0}")]
    InvalidSpec(&'static str),
    /// Handler 返回了违反游标或批量不变量的结果。
    #[error("invalid consolidation batch outcome: {0}")]
    InvalidOutcome(&'static str),
    /// 当前状态不允许请求的操作。
    #[error("job state {status:?} does not allow {operation}")]
    InvalidState {
        /// 当前状态。
        status: JobStatus,
        /// 被拒绝的操作。
        operation: &'static str,
    },
    /// 未到重试时间或 Lease 仍属于其他 Worker。
    #[error("job lease is unavailable")]
    LeaseUnavailable,
    /// Job 已暂停。
    #[error("job is paused")]
    Paused,
    /// Job 已取消。
    #[error("job was cancelled")]
    Cancelled,
    /// 资源预算被耗尽。
    #[error("consolidation budget exceeded: {0}")]
    BudgetExceeded(&'static str),
    /// Handler 请求修改不可由巩固任务覆盖的权威表。
    #[error("consolidation cannot mutate authoritative table: {0}")]
    AuthoritativeMutationDenied(&'static str),
    /// Handler 请求的派生变更不属于当前 Checkpoint Scope。
    #[error("consolidation mutation scope does not match checkpoint scope")]
    MutationScopeMismatch,
    /// Typed write 引用了其他精确 Scope 的记录。
    #[error("consolidation reference does not belong to checkpoint scope: {0}")]
    ReferenceScopeMismatch(&'static str),
    /// 具体 Handler 处理失败。
    #[error("consolidation task failed: {0}")]
    Task(String),
}

impl From<sqlx::Error> for JobError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error.to_string())
    }
}

impl From<serde_json::Error> for JobError {
    fn from(error: serde_json::Error) -> Self {
        Self::Serialization(error.to_string())
    }
}

/// 基于现有 `consolidation_jobs` 表的持久化仓储。
#[derive(Clone, Debug)]
pub struct ConsolidationJobStore {
    pub(crate) pool: SqlitePool,
}

impl ConsolidationJobStore {
    /// 复用 `SqliteStore` 的连接池。
    #[must_use]
    pub fn from_store(store: &SqliteStore) -> Self {
        Self {
            pool: store.pool().clone(),
        }
    }

    /// 提交一个 Pending Job。
    ///
    /// # Errors
    ///
    /// 规格不合法或数据库写入失败时返回错误。
    pub async fn submit(&self, spec: JobSpec) -> Result<JobRecord, JobError> {
        spec.limits.validate()?;
        let id = JobId::new();
        let now = now_us();
        let checkpoint = JobCheckpoint {
            scope: spec.scope.clone(),
            cursor: spec.cursor,
            committed_batches: 0,
            processed_items: 0,
            cpu_ms: 0,
            io_ops: 0,
            lease: None,
            next_retry_at_us: None,
        };
        sqlx::query(
            "INSERT INTO consolidation_jobs
             (id, scope_key, kind, status, checkpoint_json, attempts,
              resource_limits_json, created_at_us, updated_at_us)
             VALUES (?1, ?2, ?3, 'pending', ?4, 0, ?5, ?6, ?6)",
        )
        .bind(id.to_string())
        .bind(scope_key(&spec.scope)?)
        .bind(spec.kind.as_str())
        .bind(serde_json::to_string(&checkpoint)?)
        .bind(serde_json::to_string(&spec.limits)?)
        .bind(now)
        .execute(&self.pool)
        .await?;
        self.get(id).await
    }

    /// 读取一个 Job 的完整持久化状态。
    ///
    /// # Errors
    ///
    /// Job 不存在或行损坏时返回错误。
    pub async fn get(&self, id: JobId) -> Result<JobRecord, JobError> {
        let row = sqlx::query(
            "SELECT id, scope_key, kind, status, checkpoint_json, attempts,
                    resource_limits_json, created_at_us, updated_at_us
             FROM consolidation_jobs WHERE id = ?1",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await?
        .ok_or(JobError::NotFound(id))?;
        decode_row(&row)
    }

    /// 原子获取 Job Lease；Running Job 只有在 Lease 过期后才能被接管。
    ///
    /// # Errors
    ///
    /// Job 不可运行、尚未到重试时间或 Lease 未过期时返回错误。
    pub async fn claim(
        &self,
        id: JobId,
        worker_id: &str,
        lease_duration: Duration,
    ) -> Result<JobRecord, JobError> {
        if worker_id.trim().is_empty() {
            return Err(JobError::InvalidSpec("worker_id"));
        }
        let now = now_us();
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let mut record = load_for_update(&mut transaction, id).await?;
        match record.status {
            JobStatus::Pending => {}
            JobStatus::Retrying => {
                if record
                    .checkpoint
                    .next_retry_at_us
                    .is_some_and(|ready_at| ready_at > now)
                {
                    return Err(JobError::LeaseUnavailable);
                }
            }
            JobStatus::Running => {
                if record
                    .checkpoint
                    .lease
                    .as_ref()
                    .is_some_and(|lease| lease.expires_at_us > now)
                {
                    return Err(JobError::LeaseUnavailable);
                }
            }
            JobStatus::Paused => return Err(JobError::Paused),
            JobStatus::Cancelled => return Err(JobError::Cancelled),
            status @ (JobStatus::Completed | JobStatus::Failed) => {
                return Err(JobError::InvalidState {
                    status,
                    operation: "claim",
                });
            }
        }
        record.status = JobStatus::Running;
        record.updated_at_us = now;
        record.checkpoint.lease = Some(Lease {
            worker_id: worker_id.to_owned(),
            acquired_at_us: now,
            expires_at_us: now.saturating_add(duration_us(lease_duration)),
        });
        record.checkpoint.next_retry_at_us = None;
        write_record(&mut transaction, &record).await?;
        transaction.commit().await?;
        Ok(record)
    }

    /// 暂停一个尚未结束的 Job，并释放 Lease。
    ///
    /// # Errors
    ///
    /// 当前状态不允许暂停或数据库写入失败时返回错误。
    pub async fn pause(&self, id: JobId) -> Result<JobRecord, JobError> {
        self.transition(
            id,
            "pause",
            &[JobStatus::Pending, JobStatus::Running, JobStatus::Retrying],
            JobStatus::Paused,
        )
        .await
    }

    /// 把 Paused Job 恢复为 Pending。
    ///
    /// # Errors
    ///
    /// 当前状态不是 Paused 或数据库写入失败时返回错误。
    pub async fn resume(&self, id: JobId) -> Result<JobRecord, JobError> {
        self.transition(id, "resume", &[JobStatus::Paused], JobStatus::Pending)
            .await
    }

    /// 立即把 Retrying 或可重试 Failed Job 放回 Pending。
    ///
    /// # Errors
    ///
    /// 当前状态不可重试、重试次数耗尽或数据库写入失败时返回错误。
    pub async fn retry(&self, id: JobId) -> Result<JobRecord, JobError> {
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let mut record = load_for_update(&mut transaction, id).await?;
        if !matches!(record.status, JobStatus::Retrying | JobStatus::Failed) {
            return Err(JobError::InvalidState {
                status: record.status,
                operation: "retry",
            });
        }
        if record.attempts >= record.spec.limits.max_attempts {
            return Err(JobError::InvalidState {
                status: record.status,
                operation: "retry_exhausted",
            });
        }
        record.status = JobStatus::Pending;
        record.updated_at_us = now_us();
        record.checkpoint.lease = None;
        record.checkpoint.next_retry_at_us = None;
        write_record(&mut transaction, &record).await?;
        transaction.commit().await?;
        Ok(record)
    }

    /// 取消一个尚未结束的 Job，并释放持久化 Lease。
    ///
    /// # Errors
    ///
    /// 当前状态不可取消或数据库写入失败时返回错误。
    pub async fn cancel(&self, id: JobId) -> Result<JobRecord, JobError> {
        self.transition(
            id,
            "cancel",
            &[
                JobStatus::Pending,
                JobStatus::Running,
                JobStatus::Paused,
                JobStatus::Retrying,
            ],
            JobStatus::Cancelled,
        )
        .await
    }

    pub(crate) async fn record_failure(
        &self,
        id: JobId,
        worker_id: &str,
        retry_base: Duration,
    ) -> Result<JobRecord, JobError> {
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let mut record = load_for_update(&mut transaction, id).await?;
        verify_worker_in_write_transaction(&record, worker_id)?;
        record.attempts = record.attempts.saturating_add(1);
        record.updated_at_us = now_us();
        record.checkpoint.lease = None;
        if record.attempts >= record.spec.limits.max_attempts {
            record.status = JobStatus::Failed;
            record.checkpoint.next_retry_at_us = None;
        } else {
            record.status = JobStatus::Retrying;
            let delay = exponential_backoff(retry_base, record.attempts);
            record.checkpoint.next_retry_at_us =
                Some(record.updated_at_us.saturating_add(duration_us(delay)));
        }
        write_record(&mut transaction, &record).await?;
        transaction.commit().await?;
        Ok(record)
    }

    async fn transition(
        &self,
        id: JobId,
        operation: &'static str,
        allowed: &[JobStatus],
        target: JobStatus,
    ) -> Result<JobRecord, JobError> {
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let mut record = load_for_update(&mut transaction, id).await?;
        if !allowed.contains(&record.status) {
            return Err(JobError::InvalidState {
                status: record.status,
                operation,
            });
        }
        record.status = target;
        record.updated_at_us = now_us();
        record.checkpoint.lease = None;
        record.checkpoint.next_retry_at_us = None;
        write_record(&mut transaction, &record).await?;
        transaction.commit().await?;
        Ok(record)
    }
}

pub(crate) async fn load_for_update(
    transaction: &mut Transaction<'_, Sqlite>,
    id: JobId,
) -> Result<JobRecord, JobError> {
    let row = sqlx::query(
        "SELECT id, scope_key, kind, status, checkpoint_json, attempts,
                resource_limits_json, created_at_us, updated_at_us
         FROM consolidation_jobs WHERE id = ?1",
    )
    .bind(id.to_string())
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(JobError::NotFound(id))?;
    decode_row(&row)
}

pub(crate) async fn write_record(
    transaction: &mut Transaction<'_, Sqlite>,
    record: &JobRecord,
) -> Result<(), JobError> {
    sqlx::query(
        "UPDATE consolidation_jobs
         SET status = ?2, checkpoint_json = ?3, attempts = ?4, updated_at_us = ?5
         WHERE id = ?1",
    )
    .bind(record.id.to_string())
    .bind(record.status.as_str())
    .bind(serde_json::to_string(&record.checkpoint)?)
    .bind(i64::from(record.attempts))
    .bind(record.updated_at_us)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

fn decode_row(row: &sqlx::sqlite::SqliteRow) -> Result<JobRecord, JobError> {
    let raw_id: String = row.try_get("id")?;
    let id = JobId::from_str(&raw_id)
        .map_err(|error| JobError::Corrupt(format!("invalid job id {raw_id}: {error}")))?;
    let kind = JobKind::parse(&row.try_get::<String, _>("kind")?)?;
    let status = JobStatus::parse(&row.try_get::<String, _>("status")?)?;
    let checkpoint: JobCheckpoint =
        serde_json::from_str(&row.try_get::<String, _>("checkpoint_json")?)?;
    let stored_scope_key: String = row.try_get("scope_key")?;
    if stored_scope_key != scope_key(&checkpoint.scope)? {
        return Err(JobError::Corrupt(
            "checkpoint scope does not match scope_key".into(),
        ));
    }
    let limits: ResourceLimits =
        serde_json::from_str(&row.try_get::<String, _>("resource_limits_json")?)?;
    limits.validate()?;
    let attempts_raw: i64 = row.try_get("attempts")?;
    let attempts = u32::try_from(attempts_raw)
        .map_err(|_| JobError::Corrupt(format!("invalid attempts: {attempts_raw}")))?;
    Ok(JobRecord {
        id,
        spec: JobSpec {
            kind,
            scope: checkpoint.scope.clone(),
            cursor: checkpoint.cursor,
            limits,
        },
        status,
        checkpoint,
        attempts,
        created_at_us: row.try_get("created_at_us")?,
        updated_at_us: row.try_get("updated_at_us")?,
    })
}

/// 调用方持有独占写事务时，只需确认 Lease 尚未被其他 Worker 接管。
///
/// Lease 过期仅赋予其他 Worker 在 `claim` 时竞争接管的资格；当前 Worker
/// 一旦先取得写事务，其他写方会被 `SQLite` 串行化，不能在批次中途替换所有权。
pub(crate) fn verify_worker_in_write_transaction(
    record: &JobRecord,
    worker_id: &str,
) -> Result<(), JobError> {
    if record.status != JobStatus::Running {
        return match record.status {
            JobStatus::Paused => Err(JobError::Paused),
            JobStatus::Cancelled => Err(JobError::Cancelled),
            status => Err(JobError::InvalidState {
                status,
                operation: "commit_batch",
            }),
        };
    }
    if record
        .checkpoint
        .lease
        .as_ref()
        .is_none_or(|lease| lease.worker_id != worker_id)
    {
        return Err(JobError::LeaseUnavailable);
    }
    Ok(())
}

pub(crate) fn now_us() -> i64 {
    Utc::now().timestamp_micros()
}

pub(crate) fn duration_us(duration: Duration) -> i64 {
    i64::try_from(duration.as_micros()).unwrap_or(i64::MAX)
}

fn exponential_backoff(base: Duration, attempts: u32) -> Duration {
    if base.is_zero() {
        return Duration::ZERO;
    }
    let mut delay = base;
    for _ in 1..attempts {
        let doubled = delay.saturating_mul(2);
        if doubled == delay {
            break;
        }
        delay = doubled;
    }
    delay
}

pub(crate) async fn stage_committed_batch(
    transaction: &mut Transaction<'_, Sqlite>,
    mut record: JobRecord,
    worker_id: &str,
    outcome: BatchOutcome,
    usage: BatchUsage,
    lease_duration: Duration,
) -> Result<JobRecord, JobError> {
    let now = now_us();
    verify_worker_in_write_transaction(&record, worker_id)?;
    record.checkpoint = record
        .checkpoint
        .advanced(outcome, usage, &record.spec.limits)?;
    record.updated_at_us = now;
    if outcome.complete {
        record.status = JobStatus::Completed;
        record.checkpoint.lease = None;
    } else if let Some(lease) = record.checkpoint.lease.as_mut() {
        lease.expires_at_us = now.saturating_add(duration_us(lease_duration));
    }
    write_record(transaction, &record).await?;
    Ok(record)
}

pub(crate) fn scope_key(scope: &MemoryScope) -> Result<String, JobError> {
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
