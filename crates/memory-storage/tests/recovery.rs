//! Integrity audit, garbage collection, and recovery contract tests.

use std::{
    fs::OpenOptions,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use fs2::FileExt as _;
use jc_memory_events::factory::EventFactory;
use jc_memory_protocol::{
    BlobId, CognitiveFragment, EventDraft, FragmentContent, FragmentId, FragmentKind,
    FragmentSource, MemoryScope, NamespaceId, Perspective, SecurityLabel, SourceType,
    ValidityStatus,
};
use jc_memory_storage::{
    BlobOptions, BlobStore, GcPolicy, MemoryRepository, RebuildStage, RecoveryError,
    RecoveryFailpoint, RecoveryManager, SqliteConfig, SqliteMemoryRepository,
    SqliteProjectionStore, SqliteStore, verify_authority_hash,
};
use tempfile::TempDir;

#[test]
fn trusted_projection_stages_precede_legacy_derived_stages() {
    assert_eq!(
        &RebuildStage::ORDER[..2],
        &[RebuildStage::StateProjection, RebuildStage::GraphProjection]
    );
    assert!(verify_authority_hash("same", "same").is_ok());
    assert!(matches!(
        verify_authority_hash("before", "after"),
        Err(RecoveryError::RebuildAuthorityChanged)
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rebuild_lease_rejects_same_scope_without_blocking_another_scope() {
    let harness = Harness::new().await;
    let scope = MemoryScope::new(NamespaceId::new());
    let other_scope = MemoryScope::new(NamespaceId::new());
    let entered = Arc::new(AtomicBool::new(false));
    let release = Arc::new(AtomicBool::new(false));
    let blocker = Arc::new(BlockAfterState {
        entered: Arc::clone(&entered),
        release: Arc::clone(&release),
    });
    let first_manager = harness.recovery.clone().with_failpoint(blocker);
    let first_scope = scope.clone();
    let first = tokio::spawn(async move { first_manager.rebuild_derived(&first_scope).await });
    tokio::time::timeout(Duration::from_secs(5), async {
        while !entered.load(Ordering::Acquire) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("首个重建进入 StateProjection 后的阻塞点");

    let same_scope = harness.recovery.rebuild_derived(&scope).await;
    let other_scope_result = tokio::time::timeout(
        Duration::from_secs(5),
        harness.recovery.rebuild_derived(&other_scope),
    )
    .await;
    release.store(true, Ordering::Release);
    let first_result = first.await.expect("首个重建任务未崩溃");

    assert!(matches!(
        same_scope,
        Err(RecoveryError::RebuildLockUnavailable)
    ));
    assert!(
        other_scope_result
            .expect("不同 Scope 重建不应等待同一 Lease")
            .is_ok()
    );
    assert!(first_result.is_ok());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn authority_change_retries_from_state_projection_instead_of_skipping_stages() {
    let harness = Harness::new().await;
    let scope = MemoryScope::new(NamespaceId::new());
    let entered = Arc::new(AtomicBool::new(false));
    let release = Arc::new(AtomicBool::new(false));
    let blocker = Arc::new(BlockAfterState {
        entered: Arc::clone(&entered),
        release: Arc::clone(&release),
    });
    let interrupted = harness.recovery.clone().with_failpoint(blocker);
    let rebuild_scope = scope.clone();
    let first = tokio::spawn(async move { interrupted.rebuild_derived(&rebuild_scope).await });
    tokio::time::timeout(Duration::from_secs(5), async {
        while !entered.load(Ordering::Acquire) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("重建进入 StateProjection 后的并发写入点");
    harness
        .repository
        .append(&text_event_in_scope(
            scope.clone(),
            "重建期间追加的权威记录",
        ))
        .await
        .expect("并发追加权威 Event");
    harness
        .repository
        .append(&text_event_in_scope(
            scope.clone(),
            "并发追加的第二条权威记录",
        ))
        .await
        .expect("并发追加第二条权威 Event");
    release.store(true, Ordering::Release);
    assert!(matches!(
        first.await.expect("首轮重建任务未崩溃"),
        Err(RecoveryError::RebuildAuthorityChanged)
    ));
    let first_graph_checkpoint = SqliteProjectionStore::from_store(&harness.store)
        .checkpoint(&scope, jc_memory_storage::ProjectorKind::Graph)
        .await
        .expect("读取首轮 Graph 固定目标");
    assert_eq!(first_graph_checkpoint.sequence, 0);

    let retried = harness
        .recovery
        .rebuild_derived(&scope)
        .await
        .expect("权威变化后从 StateProjection 重新执行");
    assert_eq!(retried.resumed_after, None);
    let projection = SqliteProjectionStore::from_store(&harness.store);
    for projector in [
        jc_memory_storage::ProjectorKind::State,
        jc_memory_storage::ProjectorKind::Graph,
    ] {
        assert_eq!(
            projection
                .checkpoint(&scope, projector)
                .await
                .expect("读取重试后的投影 Checkpoint")
                .sequence,
            1
        );
    }
}

#[tokio::test]
async fn authority_change_between_attempts_invalidates_completed_state_stage() {
    let harness = Harness::new().await;
    let scope = MemoryScope::new(NamespaceId::new());
    let interrupted = harness
        .recovery
        .clone()
        .with_failpoint(Arc::new(FailAfterState));
    assert!(matches!(
        interrupted.rebuild_derived(&scope).await,
        Err(RecoveryError::InjectedFailure {
            point: "after_state_projection"
        })
    ));
    for text in ["中断后追加的首条权威记录", "中断后追加的第二条权威记录"]
    {
        harness
            .repository
            .append(&text_event_in_scope(scope.clone(), text))
            .await
            .expect("在两次重建之间追加权威 Event");
    }

    let retried = harness
        .recovery
        .rebuild_derived(&scope)
        .await
        .expect("权威哈希变化后重新执行全部阶段");
    assert_eq!(retried.resumed_after, None);
    let checkpoint = SqliteProjectionStore::from_store(&harness.store)
        .checkpoint(&scope, jc_memory_storage::ProjectorKind::State)
        .await
        .expect("读取重试后的 State Checkpoint");
    assert_eq!(checkpoint.sequence, 1);
}

#[tokio::test]
async fn legacy_rebuild_checkpoint_is_safely_invalidated_after_stage_order_upgrade() {
    let harness = Harness::new().await;
    let scope = MemoryScope::new(NamespaceId::new());
    let interrupted = harness
        .recovery
        .clone()
        .with_failpoint(Arc::new(FailAfterState));
    assert!(interrupted.rebuild_derived(&scope).await.is_err());
    let maintenance = harness.blob_root.parent().unwrap().join(".maintenance");
    let checkpoint_path = std::fs::read_dir(&maintenance)
        .expect("读取维护目录")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .expect("存在重建 Job Checkpoint");
    let legacy = serde_json::json!({
        "version": 1,
        "scope": scope,
        "completed_stages": ["fts"]
    });
    tokio::fs::write(
        checkpoint_path,
        serde_json::to_vec_pretty(&legacy).expect("序列化旧版 Checkpoint"),
    )
    .await
    .expect("写入旧版 Checkpoint");

    let rebuilt = harness
        .recovery
        .rebuild_derived(&scope)
        .await
        .expect("安全失效旧阶段顺序并从头重建");
    assert_eq!(rebuilt.resumed_after, None);
    assert_eq!(rebuilt.completed_stages, RebuildStage::ORDER);
}

#[tokio::test]
async fn audit_reports_missing_corrupt_and_orphan_blobs_without_deleting_evidence() {
    let harness = Harness::new().await;
    let missing = harness.put_referenced_blob(b"missing evidence").await;
    tokio::fs::remove_file(harness.blob_path(&missing))
        .await
        .expect("remove referenced blob");
    let corrupt = harness.put_referenced_blob(b"corrupt evidence").await;
    let corrupt_path = harness.blob_path(&corrupt);
    let mut encoded = tokio::fs::read(&corrupt_path).await.expect("read blob");
    encoded[14] ^= 0x01;
    tokio::fs::write(&corrupt_path, encoded)
        .await
        .expect("corrupt blob");
    let orphan = harness.put_orphan_blob(b"orphan evidence").await;

    let report = harness.recovery.audit().await.expect("audit storage");

    assert!(report.database_ok);
    assert_eq!(report.checked_blobs, 3);
    assert_eq!(report.missing_blobs, vec![missing]);
    assert_eq!(report.corrupt_blobs, vec![corrupt]);
    assert_eq!(report.orphan_blobs, vec![orphan.clone()]);
    assert_eq!(harness.fragment_count().await, 2);
    assert!(
        tokio::fs::try_exists(harness.blob_path(&orphan))
            .await
            .expect("inspect orphan")
    );
}

#[tokio::test]
async fn repository_registers_blob_references_transactionally() {
    let harness = Harness::new().await;
    let blob_id = harness.put_referenced_blob(b"registered evidence").await;

    let reference_count: i64 = sqlx::query_scalar(
        "SELECT ref_count FROM blob_refs
         WHERE blob_id = ?1 AND owner_type = 'fragment'",
    )
    .bind(blob_id.as_str())
    .fetch_one(harness.store.pool())
    .await
    .expect("read blob reference");
    assert_eq!(reference_count, 1);
}

#[tokio::test]
async fn garbage_collection_is_dry_run_by_default_and_requires_explicit_deletion() {
    let harness = Harness::new().await;
    let referenced = harness.put_referenced_blob(b"retained evidence").await;
    let orphan = harness.put_orphan_blob(b"collectable evidence").await;

    let dry_run = harness
        .recovery
        .collect_garbage(GcPolicy::default())
        .await
        .expect("preview garbage collection");
    assert_eq!(dry_run.candidates, vec![orphan.clone()]);
    assert!(dry_run.removed.is_empty());
    assert!(
        tokio::fs::try_exists(harness.blob_path(&orphan))
            .await
            .expect("inspect dry-run candidate")
    );

    let collected = harness
        .recovery
        .collect_garbage(GcPolicy::delete_after(Duration::ZERO))
        .await
        .expect("collect garbage");
    assert_eq!(collected.removed, vec![orphan.clone()]);
    assert!(collected.retained.contains(&referenced));
    assert!(
        !tokio::fs::try_exists(harness.blob_path(&orphan))
            .await
            .expect("inspect removed orphan")
    );
}

#[tokio::test]
async fn garbage_collection_respects_grace_period_and_process_lock() {
    let harness = Harness::new().await;
    let orphan = harness.put_orphan_blob(b"young orphan").await;
    let report = harness
        .recovery
        .collect_garbage(GcPolicy::delete_after(Duration::from_hours(1)))
        .await
        .expect("collect with grace period");
    assert!(report.removed.is_empty());
    assert!(report.retained.contains(&orphan));

    let lock_path = harness.blob_root.join(".gc.lock");
    let lock_file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(lock_path)
        .expect("open GC lock");
    lock_file.try_lock_exclusive().expect("hold GC lock");
    assert!(matches!(
        harness.recovery.collect_garbage(GcPolicy::default()).await,
        Err(RecoveryError::LockUnavailable)
    ));
    lock_file.unlock().expect("release GC lock");
}

#[tokio::test]
async fn audit_detects_fts_mismatch_and_rebuild_restores_search() {
    let harness = Harness::new().await;
    let draft = text_event("rebuildable memory evidence");
    harness
        .repository
        .append(&draft)
        .await
        .expect("append text");
    sqlx::query("DELETE FROM fragment_fts WHERE fragment_id = ?1")
        .bind(draft.fragment_id.to_string())
        .execute(harness.store.pool())
        .await
        .expect("remove FTS row");

    assert_eq!(
        harness
            .recovery
            .audit()
            .await
            .expect("audit FTS")
            .fts_mismatches,
        1
    );
    let rebuilt = harness.recovery.rebuild_fts().await.expect("rebuild FTS");
    assert_eq!(rebuilt.indexed_fragments, 1);
    assert_eq!(rebuilt.replaced_rows, 0);
    assert_eq!(
        harness
            .repository
            .search_fts(&draft.scope, "rebuildable memory", 10)
            .await
            .expect("search rebuilt FTS")
            .len(),
        1
    );
}

#[tokio::test]
async fn interrupted_fts_rebuild_preserves_the_previous_index() {
    let harness = Harness::new().await;
    let draft = text_event("stable searchable evidence");
    harness
        .repository
        .append(&draft)
        .await
        .expect("append text");
    let interrupted = harness
        .recovery
        .clone()
        .with_failpoint(Arc::new(InterruptBeforeFtsSwap));

    assert!(matches!(
        interrupted.rebuild_fts().await,
        Err(RecoveryError::InjectedFailure {
            point: "before_fts_swap"
        })
    ));
    assert_eq!(
        harness
            .repository
            .search_fts(&draft.scope, "stable searchable", 10)
            .await
            .expect("search original FTS")
            .len(),
        1
    );
    let temporary_tables: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE name LIKE 'fragment_fts_rebuild%'",
    )
    .fetch_one(harness.store.pool())
    .await
    .expect("count rebuild tables");
    assert_eq!(temporary_tables, 0);
}

#[derive(Debug)]
struct InterruptBeforeFtsSwap;

impl RecoveryFailpoint for InterruptBeforeFtsSwap {
    fn before_fts_swap(&self) -> Result<(), RecoveryError> {
        Err(RecoveryError::InjectedFailure {
            point: "before_fts_swap",
        })
    }
}

#[derive(Debug)]
struct BlockAfterState {
    entered: Arc<AtomicBool>,
    release: Arc<AtomicBool>,
}

#[derive(Debug)]
struct FailAfterState;

impl RecoveryFailpoint for FailAfterState {
    fn before_fts_swap(&self) -> Result<(), RecoveryError> {
        Ok(())
    }

    fn after_rebuild_stage(&self, stage: RebuildStage) -> Result<(), RecoveryError> {
        if stage == RebuildStage::StateProjection {
            Err(RecoveryError::InjectedFailure {
                point: "after_state_projection",
            })
        } else {
            Ok(())
        }
    }
}

impl RecoveryFailpoint for BlockAfterState {
    fn before_fts_swap(&self) -> Result<(), RecoveryError> {
        Ok(())
    }

    fn after_rebuild_stage(&self, stage: RebuildStage) -> Result<(), RecoveryError> {
        if stage == RebuildStage::StateProjection {
            self.entered.store(true, Ordering::Release);
            while !self.release.load(Ordering::Acquire) {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        Ok(())
    }
}

struct Harness {
    _directory: TempDir,
    blob_root: PathBuf,
    store: SqliteStore,
    blobs: BlobStore,
    repository: SqliteMemoryRepository,
    recovery: RecoveryManager,
}

impl Harness {
    async fn new() -> Self {
        let directory = tempfile::tempdir().expect("create recovery temp directory");
        let config = SqliteConfig::new(directory.path().join("memory.sqlite3"));
        let store = SqliteStore::open(&config).await.expect("open SQLite store");
        let blob_root = directory.path().join("blobs");
        let blobs = BlobStore::open(&blob_root, BlobOptions::plain())
            .await
            .expect("open blob store");
        let repository = SqliteMemoryRepository::from_store(&store);
        let recovery = RecoveryManager::from_stores(&store, blobs.clone());
        Self {
            _directory: directory,
            blob_root,
            store,
            blobs,
            repository,
            recovery,
        }
    }

    async fn put_referenced_blob(&self, bytes: &[u8]) -> BlobId {
        let receipt = self.blobs.put(bytes).await.expect("put referenced blob");
        let draft = blob_event(receipt.id.clone(), bytes.len() as u64);
        self.repository
            .append(&draft)
            .await
            .expect("append blob fragment");
        receipt.id
    }

    async fn put_orphan_blob(&self, bytes: &[u8]) -> BlobId {
        self.blobs.put(bytes).await.expect("put orphan blob").id
    }

    async fn fragment_count(&self) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM fragments")
            .fetch_one(self.store.pool())
            .await
            .expect("count fragments")
    }

    fn blob_path(&self, id: &BlobId) -> PathBuf {
        blob_path(&self.blob_root, id)
    }
}

fn blob_event(blob_id: BlobId, byte_len: u64) -> EventDraft {
    event(FragmentContent::Blob {
        blob_id,
        media_type: "application/octet-stream".to_owned(),
        byte_len,
    })
}

fn text_event(text: &str) -> EventDraft {
    event(FragmentContent::Text {
        text: text.to_owned(),
    })
}

fn text_event_in_scope(scope: MemoryScope, text: &str) -> EventDraft {
    event_in_scope(
        scope,
        FragmentContent::Text {
            text: text.to_owned(),
        },
    )
}

fn event(content: FragmentContent) -> EventDraft {
    event_in_scope(MemoryScope::new(NamespaceId::new()), content)
}

fn event_in_scope(scope: MemoryScope, content: FragmentContent) -> EventDraft {
    let now = chrono::Utc::now();
    let fragment = CognitiveFragment {
        id: FragmentId::new(),
        scope,
        session_id: None,
        trace_id: None,
        kind: FragmentKind::Observation,
        content,
        source: FragmentSource {
            source_type: SourceType::System,
            source_id: "recovery-fixture".to_owned(),
            actor_id: None,
            observer_id: None,
            tool_id: None,
            uri: None,
        },
        perspective: Perspective::System,
        occurred_at: now,
        observed_at: now,
        confidence: 1.0,
        importance: 0.5,
        validity: ValidityStatus::Unverified,
        security_label: SecurityLabel::Internal,
        evidence: Vec::new(),
        metadata: serde_json::Map::new(),
    };
    EventFactory::new()
        .from_fragment(fragment)
        .expect("create recovery event")
}

fn blob_path(root: &Path, id: &BlobId) -> PathBuf {
    let digest = id.as_str();
    root.join(&digest[0..2])
        .join(&digest[2..4])
        .join(format!("{digest}.jcb"))
}
