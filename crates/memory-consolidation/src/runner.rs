//! Cancellation、批循环、墙钟预算和重试协调。

use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use jc_memory_protocol::MemoryScope;
use sqlx::{Sqlite, Transaction};
use tokio::time::{Instant, timeout_at};
use tokio_util::sync::CancellationToken;

use crate::{
    BatchOutcome, ConsolidationJobStore, JobCheckpoint, JobError, JobId, JobRecord, JobStatus,
    ResourceLimits,
    checkpoint::BatchUsage,
    job::{load_for_update, now_us, scope_key, stage_committed_batch, verify_worker},
};

enum BatchAttempt {
    Cancelled,
    TimedOut(&'static str),
    Finished(Result<BatchOutcome, JobError>),
}

/// 受巩固边界保护的 `SQLite` 表类别。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConsolidationTable {
    /// 派生 Point 表。
    Points,
    /// 派生 Edge 表。
    Edges,
    /// 派生 Surface 表。
    Surfaces,
    /// 派生 Entity 表。
    Entities,
    /// 派生 State 表。
    States,
    /// 权威 Fragment 表；巩固任务不得修改。
    Fragments,
    /// 权威 Event 表；巩固任务不得修改。
    Events,
    /// 权威 `EvidenceRef` 表；巩固任务不得修改。
    EvidenceRefs,
}

impl ConsolidationTable {
    const fn name(self) -> &'static str {
        match self {
            Self::Points => "points",
            Self::Edges => "edges",
            Self::Surfaces => "surfaces",
            Self::Entities => "entities",
            Self::States => "states",
            Self::Fragments => "fragments",
            Self::Events => "events",
            Self::EvidenceRefs => "evidence_refs",
        }
    }

    const fn delete_scope_sql(self) -> Option<&'static str> {
        match self {
            Self::Points => Some("DELETE FROM points WHERE scope_key = ?1"),
            Self::Edges => Some("DELETE FROM edges WHERE scope_key = ?1"),
            Self::Surfaces => Some("DELETE FROM surfaces WHERE scope_key = ?1"),
            Self::Entities => Some("DELETE FROM entities WHERE scope_key = ?1"),
            Self::States => Some("DELETE FROM states WHERE scope_key = ?1"),
            Self::Fragments | Self::Events | Self::EvidenceRefs => None,
        }
    }
}

/// 一个由 crate 白名单解释的派生写请求。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DerivedMutation {
    table: ConsolidationTable,
    scope: MemoryScope,
}

impl DerivedMutation {
    /// 创建一个只删除精确 Scope 派生行的请求。
    #[must_use]
    pub const fn delete_scope(table: ConsolidationTable, scope: MemoryScope) -> Self {
        Self { table, scope }
    }
}

/// Handler 可见的受限批次上下文。
///
/// 上下文不公开底层 `SQLx` Transaction。所有数据库变更必须经过
/// [`Self::apply`]，因此只能修改白名单派生表，并自动计入 IO 预算。
///
/// ```compile_fail
/// use jc_memory_consolidation::BatchContext;
///
/// fn raw_transaction(context: &mut BatchContext<'_>) {
///     let _ = &mut context.transaction;
/// }
/// ```
pub struct BatchContext<'transaction> {
    transaction: &'transaction mut Transaction<'static, Sqlite>,
    checkpoint: JobCheckpoint,
    scope_key: String,
    limits: ResourceLimits,
    cancellation: CancellationToken,
    processed_items: u32,
    io_ops: u64,
}

impl<'transaction> BatchContext<'transaction> {
    fn new(
        transaction: &'transaction mut Transaction<'static, Sqlite>,
        checkpoint: &JobCheckpoint,
        limits: ResourceLimits,
        cancellation: &CancellationToken,
    ) -> Result<Self, JobError> {
        Ok(Self {
            transaction,
            checkpoint: checkpoint.clone(),
            scope_key: scope_key(&checkpoint.scope)?,
            limits,
            cancellation: cancellation.clone(),
            processed_items: 0,
            io_ops: 0,
        })
    }

    /// 返回当前批次绑定的不可变 Checkpoint。
    #[must_use]
    pub const fn checkpoint(&self) -> &JobCheckpoint {
        &self.checkpoint
    }

    /// 返回当前批次绑定的精确 Scope。
    #[must_use]
    pub const fn scope(&self) -> &MemoryScope {
        &self.checkpoint.scope
    }

    /// 返回 Runner 传入的协作取消 Token。
    #[must_use]
    pub const fn cancellation(&self) -> &CancellationToken {
        &self.cancellation
    }

    /// 返回单批允许处理的最大条目数。
    #[must_use]
    pub const fn batch_size(&self) -> u32 {
        self.limits.max_batch_size
    }

    /// 在处理条目前消费批量预算令牌。
    ///
    /// # Errors
    ///
    /// 累计条目超过单批上限时返回预算错误。
    pub fn consume_items(&mut self, count: u32) -> Result<(), JobError> {
        let processed_items = self
            .processed_items
            .checked_add(count)
            .ok_or(JobError::BudgetExceeded("batch_size"))?;
        if processed_items > self.limits.max_batch_size {
            return Err(JobError::BudgetExceeded("batch_size"));
        }
        self.processed_items = processed_items;
        Ok(())
    }

    /// 应用一个绑定 Scope、白名单化且自动计量 IO 的派生变更。
    ///
    /// # Errors
    ///
    /// 权威表、跨 Scope、取消、IO 超限或数据库失败时返回错误。
    pub async fn apply(&mut self, mutation: DerivedMutation) -> Result<(), JobError> {
        if self.cancellation.is_cancelled() {
            return Err(JobError::Cancelled);
        }
        let Some(statement) = mutation.table.delete_scope_sql() else {
            return Err(JobError::AuthoritativeMutationDenied(mutation.table.name()));
        };
        if mutation.scope != self.checkpoint.scope {
            return Err(JobError::MutationScopeMismatch);
        }
        let io_ops = self
            .io_ops
            .checked_add(1)
            .ok_or(JobError::BudgetExceeded("io"))?;
        let total_io_ops = self
            .checkpoint
            .io_ops
            .checked_add(io_ops)
            .ok_or(JobError::BudgetExceeded("io"))?;
        if total_io_ops > self.limits.max_io_ops {
            return Err(JobError::BudgetExceeded("io"));
        }
        sqlx::query(statement)
            .bind(&self.scope_key)
            .execute(&mut **self.transaction)
            .await?;
        self.io_ops = io_ops;
        Ok(())
    }

    const fn usage(&self, cpu_ms: u64) -> BatchUsage {
        BatchUsage {
            processed_items: self.processed_items,
            cpu_ms,
            io_ops: self.io_ops,
        }
    }
}

/// 一个具体巩固 Handler 必须实现的批处理边界。
#[async_trait]
pub trait ConsolidationTask: Send + Sync {
    /// 通过受限上下文处理一个批次，但不得自行推进持久化 Checkpoint。
    async fn process_batch(&self, context: &mut BatchContext<'_>)
    -> Result<BatchOutcome, JobError>;
}

/// 使用持久化 Lease 驱动单个 Job 的 Runner。
#[derive(Clone)]
pub struct JobRunner<Task> {
    store: ConsolidationJobStore,
    task: Arc<Task>,
    worker_id: String,
    lease_duration: Duration,
    retry_base: Duration,
    cancellation: CancellationToken,
}

impl<Task> JobRunner<Task>
where
    Task: ConsolidationTask + 'static,
{
    /// 创建 Runner；默认使用新的未取消 Token。
    #[must_use]
    pub fn new(
        store: ConsolidationJobStore,
        task: Arc<Task>,
        worker_id: impl Into<String>,
        lease_duration: Duration,
        retry_base: Duration,
    ) -> Self {
        Self {
            store,
            task,
            worker_id: worker_id.into(),
            lease_duration,
            retry_base,
            cancellation: CancellationToken::new(),
        }
    }

    /// 替换 Runner 使用的协作取消 Token。
    #[must_use]
    pub fn with_cancellation(mut self, cancellation: CancellationToken) -> Self {
        self.cancellation = cancellation;
        self
    }

    async fn process_one_batch(
        &self,
        transaction: &mut Transaction<'static, Sqlite>,
        record: &JobRecord,
        deadline: Instant,
        timeout_budget: &'static str,
    ) -> Result<(BatchAttempt, BatchUsage), JobError> {
        let batch_started = Instant::now();
        let mut context = BatchContext::new(
            transaction,
            &record.checkpoint,
            record.spec.limits,
            &self.cancellation,
        )?;
        let process = timeout_at(deadline, self.task.process_batch(&mut context));
        let attempt = tokio::select! {
            () = self.cancellation.cancelled() => BatchAttempt::Cancelled,
            result = process => match result {
                Ok(result) => BatchAttempt::Finished(result),
                Err(_) => BatchAttempt::TimedOut(timeout_budget),
            },
        };
        let usage = context.usage(elapsed_ms_ceil(batch_started.elapsed()));
        Ok((attempt, usage))
    }

    /// 从最近提交的 Checkpoint 运行到完成、暂停、取消或失败。
    ///
    /// # Errors
    ///
    /// Lease 不可用、资源超限、取消或 Handler 失败时返回错误。
    pub async fn run(&self, id: JobId) -> Result<JobRecord, JobError> {
        if self.cancellation.is_cancelled() {
            self.store.cancel(id).await?;
            return Err(JobError::Cancelled);
        }
        let claimed = self
            .store
            .claim(id, &self.worker_id, self.lease_duration)
            .await?;
        let started = Instant::now();
        let wall_deadline = started + Duration::from_millis(claimed.spec.limits.max_runtime_ms);
        loop {
            if self.cancellation.is_cancelled() {
                self.store.cancel(id).await?;
                return Err(JobError::Cancelled);
            }
            let mut transaction = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
            let record = load_for_update(&mut transaction, id).await?;
            match record.status {
                JobStatus::Running => {}
                JobStatus::Paused => return Err(JobError::Paused),
                JobStatus::Cancelled => return Err(JobError::Cancelled),
                status => {
                    return Err(JobError::InvalidState {
                        status,
                        operation: "run",
                    });
                }
            }
            verify_worker(&record, &self.worker_id, now_us())?;

            let remaining_cpu_ms = record
                .spec
                .limits
                .max_cpu_ms
                .saturating_sub(record.checkpoint.cpu_ms);
            if remaining_cpu_ms == 0 {
                transaction.rollback().await?;
                self.store
                    .record_failure(id, &self.worker_id, self.retry_base)
                    .await?;
                return Err(JobError::BudgetExceeded("cpu"));
            }
            let batch_started = Instant::now();
            let cpu_deadline = batch_started + Duration::from_millis(remaining_cpu_ms);
            let (batch_deadline, timeout_budget) = if cpu_deadline <= wall_deadline {
                (cpu_deadline, "cpu")
            } else {
                (wall_deadline, "wall_clock")
            };
            let (attempt, usage) = self
                .process_one_batch(&mut transaction, &record, batch_deadline, timeout_budget)
                .await?;
            let outcome = match attempt {
                BatchAttempt::Cancelled => {
                    transaction.rollback().await?;
                    self.store.cancel(id).await?;
                    return Err(JobError::Cancelled);
                }
                BatchAttempt::TimedOut(budget) => {
                    transaction.rollback().await?;
                    self.store
                        .record_failure(id, &self.worker_id, self.retry_base)
                        .await?;
                    return Err(JobError::BudgetExceeded(budget));
                }
                BatchAttempt::Finished(Ok(outcome)) => outcome,
                BatchAttempt::Finished(Err(error)) => {
                    transaction.rollback().await?;
                    self.store
                        .record_failure(id, &self.worker_id, self.retry_base)
                        .await?;
                    return Err(error);
                }
            };
            if let Err(error) = record
                .checkpoint
                .advanced(outcome, usage, &record.spec.limits)
            {
                transaction.rollback().await?;
                self.store
                    .record_failure(id, &self.worker_id, self.retry_base)
                    .await?;
                return Err(error);
            }
            let record = stage_committed_batch(
                &mut transaction,
                record,
                &self.worker_id,
                outcome,
                usage,
                self.lease_duration,
            )
            .await?;
            transaction.commit().await?;
            if record.status == JobStatus::Completed {
                return Ok(record);
            }
            tokio::task::yield_now().await;
        }
    }
}

fn elapsed_ms_ceil(elapsed: Duration) -> u64 {
    let micros = elapsed.as_micros();
    let rounded = micros.saturating_add(999) / 1_000;
    u64::try_from(rounded).unwrap_or(u64::MAX)
}
