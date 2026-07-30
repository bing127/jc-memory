//! 持久化 Job 的失败恢复、暂停、取消、预算与 Lease 接管测试。

use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use jc_memory_consolidation::{
    BatchContext, BatchOutcome, ConsolidationJobStore, ConsolidationTable, ConsolidationTask,
    DerivedMutation, JobError, JobKind, JobRunner, JobSpec, JobStatus, ResourceLimits,
};
use jc_memory_protocol::{MemoryScope, NamespaceId};
use jc_memory_storage::{SqliteConfig, SqliteStore};
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;

struct Harness {
    _directory: TempDir,
    path: PathBuf,
    store: SqliteStore,
    jobs: ConsolidationJobStore,
}

impl Harness {
    async fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("memory.sqlite3");
        let store = SqliteStore::open(&SqliteConfig::new(&path)).await.unwrap();
        let jobs = ConsolidationJobStore::from_store(&store);
        Self {
            _directory: directory,
            path,
            store,
            jobs,
        }
    }

    fn spec(max_batch_size: u32) -> JobSpec {
        JobSpec {
            kind: JobKind::Replay,
            scope: MemoryScope::new(NamespaceId::new()),
            cursor: 0,
            limits: ResourceLimits {
                max_batch_size,
                max_cpu_ms: 100,
                max_io_ops: 100,
                max_runtime_ms: 1_000,
                max_attempts: 3,
            },
        }
    }
}

struct SequenceTask {
    total: u64,
    fail_at: Option<u64>,
    fail_once: AtomicBool,
    starts: Mutex<Vec<u64>>,
    delay: Duration,
}

impl SequenceTask {
    fn new(total: u64) -> Self {
        Self {
            total,
            fail_at: None,
            fail_once: AtomicBool::new(false),
            starts: Mutex::new(Vec::new()),
            delay: Duration::ZERO,
        }
    }

    fn failing_once_at(total: u64, cursor: u64) -> Self {
        Self {
            total,
            fail_at: Some(cursor),
            fail_once: AtomicBool::new(true),
            starts: Mutex::new(Vec::new()),
            delay: Duration::ZERO,
        }
    }

    fn slow(total: u64, delay: Duration) -> Self {
        Self {
            delay,
            ..Self::new(total)
        }
    }

    fn starts(&self) -> Vec<u64> {
        self.starts.lock().unwrap().clone()
    }
}

#[async_trait]
impl ConsolidationTask for SequenceTask {
    async fn process_batch(
        &self,
        context: &mut BatchContext<'_>,
    ) -> Result<BatchOutcome, JobError> {
        if context.cancellation().is_cancelled() {
            return Err(JobError::Cancelled);
        }
        let cursor = context.checkpoint().cursor;
        self.starts.lock().unwrap().push(cursor);
        if !self.delay.is_zero() {
            tokio::time::sleep(self.delay).await;
        }
        if self.fail_at == Some(cursor) && self.fail_once.swap(false, Ordering::SeqCst) {
            return Err(JobError::Task("fixture failure".into()));
        }
        let processed = u64::from(context.batch_size()).min(self.total.saturating_sub(cursor));
        context.consume_items(u32::try_from(processed).unwrap())?;
        let next_cursor = cursor + processed;
        Ok(BatchOutcome {
            next_cursor,
            complete: next_cursor == self.total,
        })
    }
}

struct TransactionalFailureTask {
    scope: MemoryScope,
}

#[async_trait]
impl ConsolidationTask for TransactionalFailureTask {
    async fn process_batch(
        &self,
        context: &mut BatchContext<'_>,
    ) -> Result<BatchOutcome, JobError> {
        context
            .apply(DerivedMutation::delete_scope(
                ConsolidationTable::States,
                self.scope.clone(),
            ))
            .await?;
        Err(JobError::Task("transactional fixture failure".into()))
    }
}

struct AuthorityMutationTask {
    scope: MemoryScope,
}

#[async_trait]
impl ConsolidationTask for AuthorityMutationTask {
    async fn process_batch(
        &self,
        context: &mut BatchContext<'_>,
    ) -> Result<BatchOutcome, JobError> {
        context
            .apply(DerivedMutation::delete_scope(
                ConsolidationTable::Fragments,
                self.scope.clone(),
            ))
            .await?;
        Ok(BatchOutcome {
            next_cursor: 1,
            complete: true,
        })
    }
}

struct CrossScopeMutationTask {
    other_scope: MemoryScope,
}

#[async_trait]
impl ConsolidationTask for CrossScopeMutationTask {
    async fn process_batch(
        &self,
        context: &mut BatchContext<'_>,
    ) -> Result<BatchOutcome, JobError> {
        context
            .apply(DerivedMutation::delete_scope(
                ConsolidationTable::States,
                self.other_scope.clone(),
            ))
            .await?;
        Ok(BatchOutcome {
            next_cursor: 1,
            complete: true,
        })
    }
}

struct AlwaysFailTask;

#[async_trait]
impl ConsolidationTask for AlwaysFailTask {
    async fn process_batch(
        &self,
        _context: &mut BatchContext<'_>,
    ) -> Result<BatchOutcome, JobError> {
        Err(JobError::Task("persistent fixture failure".into()))
    }
}

struct ConsumeItemsTask {
    count: u32,
}

#[async_trait]
impl ConsolidationTask for ConsumeItemsTask {
    async fn process_batch(
        &self,
        context: &mut BatchContext<'_>,
    ) -> Result<BatchOutcome, JobError> {
        context.consume_items(self.count)?;
        Ok(BatchOutcome {
            next_cursor: u64::from(self.count),
            complete: true,
        })
    }
}

struct DerivedIoTask {
    operations: u32,
}

struct NeverCompletesTask {
    scope: MemoryScope,
}

#[async_trait]
impl ConsolidationTask for NeverCompletesTask {
    async fn process_batch(
        &self,
        context: &mut BatchContext<'_>,
    ) -> Result<BatchOutcome, JobError> {
        context
            .apply(DerivedMutation::delete_scope(
                ConsolidationTable::States,
                self.scope.clone(),
            ))
            .await?;
        std::future::pending().await
    }
}

#[async_trait]
impl ConsolidationTask for DerivedIoTask {
    async fn process_batch(
        &self,
        context: &mut BatchContext<'_>,
    ) -> Result<BatchOutcome, JobError> {
        for _ in 0..self.operations {
            context
                .apply(DerivedMutation::delete_scope(
                    ConsolidationTable::States,
                    context.scope().clone(),
                ))
                .await?;
        }
        context.consume_items(1)?;
        Ok(BatchOutcome {
            next_cursor: context.checkpoint().cursor + 1,
            complete: true,
        })
    }
}

#[tokio::test]
async fn failed_batch_never_advances_committed_cursor() {
    let harness = Harness::new().await;
    let job = harness.jobs.submit(Harness::spec(4)).await.unwrap();
    let task = Arc::new(SequenceTask::failing_once_at(10, 4));
    let runner = JobRunner::new(
        harness.jobs.clone(),
        Arc::clone(&task),
        "worker-a",
        Duration::from_secs(1),
        Duration::ZERO,
    );

    assert!(runner.run(job.id).await.is_err());
    let failed = harness.jobs.get(job.id).await.unwrap();
    assert_eq!(failed.checkpoint.cursor, 4);
    assert_eq!(failed.status, JobStatus::Retrying);

    harness.jobs.retry(job.id).await.unwrap();
    runner.run(job.id).await.unwrap();
    let completed = harness.jobs.get(job.id).await.unwrap();
    assert_eq!(completed.status, JobStatus::Completed);
    assert_eq!(completed.checkpoint.cursor, 10);
    assert_eq!(task.starts(), vec![0, 4, 4, 8]);
}

#[tokio::test]
async fn paused_job_resumes_from_last_checkpoint() {
    let harness = Harness::new().await;
    let job = harness.jobs.submit(Harness::spec(2)).await.unwrap();
    harness.jobs.pause(job.id).await.unwrap();
    let task = Arc::new(SequenceTask::new(4));
    let runner = JobRunner::new(
        harness.jobs.clone(),
        Arc::clone(&task),
        "worker-a",
        Duration::from_secs(1),
        Duration::ZERO,
    );

    assert_eq!(runner.run(job.id).await.unwrap_err(), JobError::Paused);
    harness.jobs.resume(job.id).await.unwrap();
    runner.run(job.id).await.unwrap();

    let completed = harness.jobs.get(job.id).await.unwrap();
    assert_eq!(completed.status, JobStatus::Completed);
    assert_eq!(task.starts(), vec![0, 2]);
}

#[tokio::test]
async fn running_job_paused_between_batches_resumes_from_nonzero_cursor() {
    let harness = Harness::new().await;
    let job = harness.jobs.submit(Harness::spec(2)).await.unwrap();
    let initial_task = Arc::new(SequenceTask::failing_once_at(4, 2));
    let initial_runner = JobRunner::new(
        harness.jobs.clone(),
        initial_task,
        "worker-a",
        Duration::from_secs(1),
        Duration::ZERO,
    );
    assert!(initial_runner.run(job.id).await.is_err());
    assert_eq!(harness.jobs.get(job.id).await.unwrap().checkpoint.cursor, 2);

    harness.jobs.retry(job.id).await.unwrap();
    let running = harness
        .jobs
        .claim(job.id, "worker-a", Duration::from_secs(1))
        .await
        .unwrap();
    assert_eq!(running.status, JobStatus::Running);
    assert_eq!(running.checkpoint.cursor, 2);
    let paused = harness.jobs.pause(job.id).await.unwrap();
    assert_eq!(paused.status, JobStatus::Paused);
    assert_eq!(paused.checkpoint.cursor, 2);

    harness.jobs.resume(job.id).await.unwrap();
    let resumed_task = Arc::new(SequenceTask::new(4));
    let resumed_runner = JobRunner::new(
        harness.jobs.clone(),
        Arc::clone(&resumed_task),
        "worker-b",
        Duration::from_secs(1),
        Duration::ZERO,
    );
    let completed = resumed_runner.run(job.id).await.unwrap();
    assert_eq!(completed.status, JobStatus::Completed);
    assert_eq!(completed.checkpoint.cursor, 4);
    assert_eq!(resumed_task.starts(), vec![2]);
}

#[tokio::test]
async fn cancellation_is_persisted_without_advancing_cursor() {
    let harness = Harness::new().await;
    let job = harness.jobs.submit(Harness::spec(2)).await.unwrap();
    let task = Arc::new(SequenceTask::new(4));
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    let runner = JobRunner::new(
        harness.jobs.clone(),
        task,
        "worker-a",
        Duration::from_secs(1),
        Duration::ZERO,
    )
    .with_cancellation(cancellation);

    assert_eq!(runner.run(job.id).await.unwrap_err(), JobError::Cancelled);
    let cancelled = harness.jobs.get(job.id).await.unwrap();
    assert_eq!(cancelled.status, JobStatus::Cancelled);
    assert_eq!(cancelled.checkpoint.cursor, 0);
}

#[tokio::test]
async fn expired_lease_can_be_taken_over_by_another_worker() {
    let harness = Harness::new().await;
    let job = harness.jobs.submit(Harness::spec(2)).await.unwrap();
    harness
        .jobs
        .claim(job.id, "worker-a", Duration::from_secs(60))
        .await
        .unwrap();
    assert!(matches!(
        harness
            .jobs
            .claim(job.id, "worker-b", Duration::from_secs(1))
            .await,
        Err(JobError::LeaseUnavailable)
    ));

    sqlx::query(
        "UPDATE consolidation_jobs
         SET checkpoint_json = json_set(checkpoint_json, '$.lease.expires_at_us', 0)
         WHERE id = ?1",
    )
    .bind(job.id.to_string())
    .execute(harness.store.pool())
    .await
    .unwrap();
    let claimed = harness
        .jobs
        .claim(job.id, "worker-b", Duration::from_secs(1))
        .await
        .unwrap();
    assert_eq!(claimed.checkpoint.lease.unwrap().worker_id, "worker-b");
}

#[tokio::test]
async fn concurrent_claim_allows_exactly_one_worker() {
    let harness = Harness::new().await;
    let job = harness.jobs.submit(Harness::spec(1)).await.unwrap();
    let jobs_a = harness.jobs.clone();
    let jobs_b = harness.jobs.clone();

    let (claim_a, claim_b) = tokio::join!(
        jobs_a.claim(job.id, "worker-a", Duration::from_secs(1)),
        jobs_b.claim(job.id, "worker-b", Duration::from_secs(1)),
    );

    assert_eq!(
        usize::from(claim_a.is_ok()) + usize::from(claim_b.is_ok()),
        1
    );
    assert!(matches!(
        (&claim_a, &claim_b),
        (Ok(_), Err(JobError::LeaseUnavailable)) | (Err(JobError::LeaseUnavailable), Ok(_))
    ));
}

#[tokio::test]
async fn wall_clock_budget_failure_keeps_last_committed_cursor() {
    let harness = Harness::new().await;
    let mut spec = Harness::spec(2);
    spec.limits.max_runtime_ms = 1;
    let job = harness.jobs.submit(spec).await.unwrap();
    let task = Arc::new(SequenceTask::slow(4, Duration::from_millis(5)));
    let runner = JobRunner::new(
        harness.jobs.clone(),
        task,
        "worker-a",
        Duration::from_secs(1),
        Duration::ZERO,
    );

    assert_eq!(
        runner.run(job.id).await.unwrap_err(),
        JobError::BudgetExceeded("wall_clock")
    );
    let failed = harness.jobs.get(job.id).await.unwrap();
    assert_eq!(failed.checkpoint.cursor, 0);
    assert_eq!(failed.status, JobStatus::Retrying);
}

#[tokio::test]
async fn wall_clock_deadline_aborts_a_permanently_pending_batch() {
    let harness = Harness::new().await;
    let mut spec = Harness::spec(1);
    spec.limits.max_runtime_ms = 20;
    let scope = spec.scope.clone();
    let job = harness.jobs.submit(spec).await.unwrap();
    let scope_key: String =
        sqlx::query_scalar("SELECT scope_key FROM consolidation_jobs WHERE id = ?1")
            .bind(job.id.to_string())
            .fetch_one(harness.store.pool())
            .await
            .unwrap();
    sqlx::query(
        "INSERT INTO states
         (id, scope_key, state_key, current_version_id, created_at_us)
         VALUES (?1, ?2, 'timeout-fixture', NULL, 0)",
    )
    .bind(NamespaceId::new().to_string())
    .bind(scope_key)
    .execute(harness.store.pool())
    .await
    .unwrap();
    let runner = JobRunner::new(
        harness.jobs.clone(),
        Arc::new(NeverCompletesTask { scope }),
        "worker-a",
        Duration::from_millis(5),
        Duration::ZERO,
    );

    let result = tokio::time::timeout(Duration::from_millis(200), runner.run(job.id))
        .await
        .expect("Runner 必须在墙钟预算内中止永久 pending future");
    assert_eq!(result.unwrap_err(), JobError::BudgetExceeded("wall_clock"));
    let failed = harness.jobs.get(job.id).await.unwrap();
    assert_eq!(failed.status, JobStatus::Retrying);
    assert_eq!(failed.checkpoint.cursor, 0);
    assert_eq!(failed.attempts, 1);
    let state_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM states")
        .fetch_one(harness.store.pool())
        .await
        .unwrap();
    assert_eq!(state_count, 1);
}

#[tokio::test]
async fn checkpoint_survives_store_reopen() {
    let harness = Harness::new().await;
    let job = harness.jobs.submit(Harness::spec(2)).await.unwrap();
    let path = harness.path.clone();
    harness.store.pool().close().await;

    let reopened = SqliteStore::open(&SqliteConfig::new(path)).await.unwrap();
    let record = ConsolidationJobStore::from_store(&reopened)
        .get(job.id)
        .await
        .unwrap();
    assert_eq!(record.checkpoint.cursor, 0);
    assert_eq!(record.status, JobStatus::Pending);
}

#[tokio::test]
async fn nonzero_retrying_checkpoint_survives_reopen_and_resumes() {
    let harness = Harness::new().await;
    let job = harness.jobs.submit(Harness::spec(2)).await.unwrap();
    let failing_runner = JobRunner::new(
        harness.jobs.clone(),
        Arc::new(SequenceTask::failing_once_at(4, 2)),
        "worker-a",
        Duration::from_secs(1),
        Duration::ZERO,
    );
    assert!(failing_runner.run(job.id).await.is_err());
    let path = harness.path.clone();
    harness.store.pool().close().await;

    let reopened = SqliteStore::open(&SqliteConfig::new(path)).await.unwrap();
    let jobs = ConsolidationJobStore::from_store(&reopened);
    let retrying = jobs.get(job.id).await.unwrap();
    assert_eq!(retrying.status, JobStatus::Retrying);
    assert_eq!(retrying.checkpoint.cursor, 2);
    assert_eq!(retrying.attempts, 1);

    jobs.retry(job.id).await.unwrap();
    let resumed_task = Arc::new(SequenceTask::new(4));
    let runner = JobRunner::new(
        jobs,
        Arc::clone(&resumed_task),
        "worker-b",
        Duration::from_secs(1),
        Duration::ZERO,
    );
    let completed = runner.run(job.id).await.unwrap();
    assert_eq!(completed.status, JobStatus::Completed);
    assert_eq!(completed.checkpoint.cursor, 4);
    assert_eq!(resumed_task.starts(), vec![2]);
}

#[tokio::test]
async fn failed_handler_rolls_back_side_effects_with_checkpoint() {
    let harness = Harness::new().await;
    let spec = Harness::spec(2);
    let scope = spec.scope.clone();
    let job = harness.jobs.submit(spec).await.unwrap();
    let scope_key: String =
        sqlx::query_scalar("SELECT scope_key FROM consolidation_jobs WHERE id = ?1")
            .bind(job.id.to_string())
            .fetch_one(harness.store.pool())
            .await
            .unwrap();
    sqlx::query(
        "INSERT INTO states
         (id, scope_key, state_key, current_version_id, created_at_us)
         VALUES (?1, ?2, 'fixture', NULL, 0)",
    )
    .bind(NamespaceId::new().to_string())
    .bind(scope_key)
    .execute(harness.store.pool())
    .await
    .unwrap();
    let runner = JobRunner::new(
        harness.jobs.clone(),
        Arc::new(TransactionalFailureTask { scope }),
        "worker-a",
        Duration::from_secs(1),
        Duration::ZERO,
    );

    assert!(runner.run(job.id).await.is_err());
    let state_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM states")
        .fetch_one(harness.store.pool())
        .await
        .unwrap();
    assert_eq!(state_count, 1);
    assert_eq!(harness.jobs.get(job.id).await.unwrap().checkpoint.cursor, 0);
}

#[tokio::test]
async fn corrupted_checkpoint_scope_is_rejected() {
    let harness = Harness::new().await;
    let job = harness.jobs.submit(Harness::spec(2)).await.unwrap();
    sqlx::query(
        "UPDATE consolidation_jobs
         SET checkpoint_json = json_set(checkpoint_json, '$.scope.namespace_id', ?2)
         WHERE id = ?1",
    )
    .bind(job.id.to_string())
    .bind(NamespaceId::new().to_string())
    .execute(harness.store.pool())
    .await
    .unwrap();

    assert!(matches!(
        harness.jobs.get(job.id).await,
        Err(JobError::Corrupt(_))
    ));
}

#[tokio::test]
async fn retry_backoff_keeps_doubling_after_twenty_failures() {
    let harness = Harness::new().await;
    let mut spec = Harness::spec(1);
    spec.limits.max_attempts = 23;
    let job = harness.jobs.submit(spec).await.unwrap();
    sqlx::query("UPDATE consolidation_jobs SET attempts = 20 WHERE id = ?1")
        .bind(job.id.to_string())
        .execute(harness.store.pool())
        .await
        .unwrap();
    let runner = JobRunner::new(
        harness.jobs.clone(),
        Arc::new(AlwaysFailTask),
        "worker-a",
        Duration::from_secs(10),
        Duration::from_micros(1),
    );

    assert!(runner.run(job.id).await.is_err());
    let first = harness.jobs.get(job.id).await.unwrap();
    assert_eq!(first.attempts, 21);
    assert_eq!(
        first.checkpoint.next_retry_at_us.unwrap() - first.updated_at_us,
        1_i64 << 20
    );

    harness.jobs.retry(job.id).await.unwrap();
    assert!(runner.run(job.id).await.is_err());
    let second = harness.jobs.get(job.id).await.unwrap();
    assert_eq!(second.attempts, 22);
    assert_eq!(
        second.checkpoint.next_retry_at_us.unwrap() - second.updated_at_us,
        1_i64 << 21
    );
}

#[tokio::test]
async fn maximum_attempts_end_in_terminal_failure() {
    let harness = Harness::new().await;
    let mut spec = Harness::spec(1);
    spec.limits.max_attempts = 2;
    let job = harness.jobs.submit(spec).await.unwrap();
    let runner = JobRunner::new(
        harness.jobs.clone(),
        Arc::new(AlwaysFailTask),
        "worker-a",
        Duration::from_secs(1),
        Duration::ZERO,
    );

    assert!(runner.run(job.id).await.is_err());
    let retrying = harness.jobs.get(job.id).await.unwrap();
    assert_eq!(retrying.status, JobStatus::Retrying);
    assert_eq!(retrying.attempts, 1);

    harness.jobs.retry(job.id).await.unwrap();
    assert!(runner.run(job.id).await.is_err());
    let failed = harness.jobs.get(job.id).await.unwrap();
    assert_eq!(failed.status, JobStatus::Failed);
    assert_eq!(failed.attempts, 2);
    assert_eq!(failed.checkpoint.next_retry_at_us, None);
    assert!(matches!(
        harness.jobs.retry(job.id).await,
        Err(JobError::InvalidState {
            status: JobStatus::Failed,
            operation: "retry_exhausted"
        })
    ));
}

#[tokio::test]
async fn oversized_batch_is_rolled_back_before_checkpoint_advance() {
    let harness = Harness::new().await;
    let job = harness.jobs.submit(Harness::spec(2)).await.unwrap();
    let task = Arc::new(ConsumeItemsTask { count: 3 });
    let runner = JobRunner::new(
        harness.jobs.clone(),
        task,
        "worker-a",
        Duration::from_secs(1),
        Duration::ZERO,
    );

    assert_eq!(
        runner.run(job.id).await.unwrap_err(),
        JobError::BudgetExceeded("batch_size")
    );
    let failed = harness.jobs.get(job.id).await.unwrap();
    assert_eq!(failed.checkpoint.cursor, 0);
    assert_eq!(failed.checkpoint.processed_items, 0);
    assert_eq!(failed.status, JobStatus::Retrying);
}

#[tokio::test]
async fn cpu_budget_excess_is_rolled_back() {
    let harness = Harness::new().await;
    let mut spec = Harness::spec(1);
    spec.limits.max_cpu_ms = 1;
    let job = harness.jobs.submit(spec).await.unwrap();
    let task = Arc::new(SequenceTask::slow(1, Duration::from_millis(5)));
    let runner = JobRunner::new(
        harness.jobs.clone(),
        task,
        "worker-a",
        Duration::from_secs(1),
        Duration::ZERO,
    );

    assert_eq!(
        runner.run(job.id).await.unwrap_err(),
        JobError::BudgetExceeded("cpu")
    );
    let failed = harness.jobs.get(job.id).await.unwrap();
    assert_eq!(failed.checkpoint.cursor, 0);
    assert_eq!(failed.checkpoint.cpu_ms, 0);
}

#[tokio::test]
async fn io_budget_excess_is_rolled_back() {
    let harness = Harness::new().await;
    let mut spec = Harness::spec(1);
    spec.limits.max_io_ops = 1;
    let job = harness.jobs.submit(spec).await.unwrap();
    let task = Arc::new(DerivedIoTask { operations: 2 });
    let runner = JobRunner::new(
        harness.jobs.clone(),
        task,
        "worker-a",
        Duration::from_secs(1),
        Duration::ZERO,
    );

    assert_eq!(
        runner.run(job.id).await.unwrap_err(),
        JobError::BudgetExceeded("io")
    );
    let failed = harness.jobs.get(job.id).await.unwrap();
    assert_eq!(failed.checkpoint.cursor, 0);
    assert_eq!(failed.checkpoint.io_ops, 0);
}

#[tokio::test]
async fn handler_cannot_mutate_authoritative_tables() {
    let harness = Harness::new().await;
    let spec = Harness::spec(1);
    let scope = spec.scope.clone();
    let job = harness.jobs.submit(spec).await.unwrap();
    let runner = JobRunner::new(
        harness.jobs.clone(),
        Arc::new(AuthorityMutationTask { scope }),
        "worker-a",
        Duration::from_secs(1),
        Duration::ZERO,
    );

    assert_eq!(
        runner.run(job.id).await.unwrap_err(),
        JobError::AuthoritativeMutationDenied("fragments")
    );
    let failed = harness.jobs.get(job.id).await.unwrap();
    assert_eq!(failed.checkpoint.cursor, 0);
    assert_eq!(failed.checkpoint.io_ops, 0);
}

#[tokio::test]
async fn handler_cannot_mutate_another_scope() {
    let harness = Harness::new().await;
    let job = harness.jobs.submit(Harness::spec(1)).await.unwrap();
    let runner = JobRunner::new(
        harness.jobs.clone(),
        Arc::new(CrossScopeMutationTask {
            other_scope: MemoryScope::new(NamespaceId::new()),
        }),
        "worker-a",
        Duration::from_secs(1),
        Duration::ZERO,
    );

    assert_eq!(
        runner.run(job.id).await.unwrap_err(),
        JobError::MutationScopeMismatch
    );
    let failed = harness.jobs.get(job.id).await.unwrap();
    assert_eq!(failed.checkpoint.cursor, 0);
    assert_eq!(failed.checkpoint.io_ops, 0);
}
