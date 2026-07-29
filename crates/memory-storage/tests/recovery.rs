//! Integrity audit, garbage collection, and recovery contract tests.

use std::{
    fs::OpenOptions,
    path::{Path, PathBuf},
    sync::Arc,
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
    BlobOptions, BlobStore, GcPolicy, MemoryRepository, RecoveryError, RecoveryFailpoint,
    RecoveryManager, SqliteConfig, SqliteMemoryRepository, SqliteStore,
};
use tempfile::TempDir;

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
        .collect_garbage(GcPolicy::delete_after(Duration::from_secs(3600)))
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

fn event(content: FragmentContent) -> EventDraft {
    let now = chrono::Utc::now();
    let fragment = CognitiveFragment {
        id: FragmentId::new(),
        scope: MemoryScope::new(NamespaceId::new()),
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
