//! Concurrency, rollback, and recovery acceptance tests.

mod common;

use std::{
    collections::BTreeSet,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use futures::future::try_join_all;
use jc_memory_core::{MemoryConfig, MemoryEngine, MemoryError, ObserveReceipt};
use jc_memory_protocol::{
    CognitiveFragment, EvidenceId, EvidenceRef, FragmentContent, FragmentId, MemoryScope,
    NamespaceId, TenantId, VerificationMethod,
};
use jc_memory_storage::{
    BlobOptions, BlobStore, MemoryRepository, RecoveryManager, RepositoryFailpoint, SqliteConfig,
    SqliteMemoryRepository, SqliteStore, StorageError,
};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_observe_assigns_unique_monotonic_exact_scope_sequences() {
    let data_dir = tempfile::tempdir().expect("create engine data directory");
    let engine = MemoryEngine::open(MemoryConfig::local(data_dir.path()))
        .await
        .expect("open memory engine");
    let scope = MemoryScope::new(NamespaceId::new());
    let receipts = try_join_all((0..64).map(|index| {
        let engine = engine.clone();
        let fragment = common::observation_in_scope(scope.clone(), &format!("event {index}"));
        async move { authorized_observe(&engine, fragment).await }
    }))
    .await
    .expect("observe concurrent fragments");

    let mut sequences: Vec<_> = receipts
        .into_iter()
        .map(|receipt| receipt.sequence)
        .collect();
    sequences.sort_unstable();
    assert_eq!(sequences, (0..64).collect::<Vec<_>>());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_same_fragment_is_idempotent() {
    let data_dir = tempfile::tempdir().expect("create engine data directory");
    let engine = MemoryEngine::open(MemoryConfig::local(data_dir.path()))
        .await
        .expect("open memory engine");
    let fragment = common::observation("same concurrent fragment");
    let receipts = try_join_all((0..32).map(|_| {
        let engine = engine.clone();
        let fragment = fragment.clone();
        async move { authorized_observe(&engine, fragment).await }
    }))
    .await
    .expect("observe identical fragment concurrently");

    assert_eq!(receipts.iter().filter(|receipt| receipt.created).count(), 1);
    assert_eq!(
        receipts
            .iter()
            .map(|receipt| receipt.event_id)
            .collect::<BTreeSet<_>>()
            .len(),
        1
    );
    assert!(receipts.iter().all(|receipt| receipt.sequence == 0));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn tenants_under_one_namespace_have_independent_sequences() {
    let data_dir = tempfile::tempdir().expect("create engine data directory");
    let engine = MemoryEngine::open(MemoryConfig::local(data_dir.path()))
        .await
        .expect("open memory engine");
    let namespace_id = NamespaceId::new();
    let mut first_scope = MemoryScope::new(namespace_id);
    first_scope.tenant_id = Some(TenantId::new());
    let mut second_scope = MemoryScope::new(namespace_id);
    second_scope.tenant_id = Some(TenantId::new());

    let first = try_join_all((0..16).map(|index| {
        let engine = engine.clone();
        let fragment =
            common::observation_in_scope(first_scope.clone(), &format!("first tenant {index}"));
        async move { authorized_observe(&engine, fragment).await }
    }));
    let second = try_join_all((0..16).map(|index| {
        let engine = engine.clone();
        let fragment =
            common::observation_in_scope(second_scope.clone(), &format!("second tenant {index}"));
        async move { authorized_observe(&engine, fragment).await }
    }));
    let (first, second) = tokio::try_join!(first, second).expect("observe both tenants");

    for receipts in [first, second] {
        let mut sequences: Vec<_> = receipts
            .into_iter()
            .map(|receipt| receipt.sequence)
            .collect();
        sequences.sort_unstable();
        assert_eq!(sequences, (0..16).collect::<Vec<_>>());
    }
}

#[tokio::test]
async fn failed_commit_never_exposes_fragment_event_or_fts_row() {
    let data_dir = tempfile::tempdir().expect("create engine data directory");
    let engine = MemoryEngine::open_with_repository_failpoint(
        MemoryConfig::local(data_dir.path()),
        Arc::new(InterruptBeforeCommit),
    )
    .await
    .expect("open failpoint engine");
    let fragment = common::observation("must roll back before commit");

    assert!(matches!(
        authorized_observe(&engine, fragment.clone()).await,
        Err(MemoryError::Storage(StorageError::InjectedFailure {
            point: "before_commit"
        }))
    ));
    drop(engine);

    let reopened = MemoryEngine::open(MemoryConfig::local(data_dir.path()))
        .await
        .expect("reopen memory engine");
    assert!(matches!(
        authorized_fragment(&reopened, &fragment.scope, fragment.id).await,
        Err(MemoryError::NotFound { .. })
    ));
    drop(reopened);

    let store = SqliteStore::open(&SqliteConfig::new(data_dir.path().join("memory.sqlite3")))
        .await
        .expect("open store for FTS verification");
    let repository = SqliteMemoryRepository::from_store(&store);
    assert!(
        repository
            .search_fts(&fragment.scope, "must roll back", 10)
            .await
            .expect("search after rollback")
            .is_empty()
    );
}

#[tokio::test]
async fn blob_published_before_failed_commit_is_reported_as_an_orphan() {
    let data_dir = tempfile::tempdir().expect("create engine data directory");
    let blob_root = data_dir.path().join("blobs");
    let blobs = BlobStore::open(&blob_root, BlobOptions::plain())
        .await
        .expect("open blob store");
    let bytes = b"published evidence before transaction";
    let blob = blobs.put(bytes).await.expect("publish blob");
    let engine = MemoryEngine::open_with_repository_failpoint(
        MemoryConfig::local(data_dir.path()),
        Arc::new(InterruptBeforeCommit),
    )
    .await
    .expect("open failpoint engine");
    let mut fragment = common::observation("blob placeholder");
    fragment.content = FragmentContent::Blob {
        blob_id: blob.id.clone(),
        media_type: "application/octet-stream".to_owned(),
        byte_len: bytes.len() as u64,
    };

    assert!(matches!(
        authorized_observe(&engine, fragment.clone()).await,
        Err(MemoryError::Storage(StorageError::InjectedFailure {
            point: "before_commit"
        }))
    ));
    drop(engine);

    let store = SqliteStore::open(&SqliteConfig::new(data_dir.path().join("memory.sqlite3")))
        .await
        .expect("reopen store");
    let recovery = RecoveryManager::from_stores(&store, blobs);
    let report = recovery.audit().await.expect("audit failed blob commit");
    assert_eq!(report.orphan_blobs, vec![blob.id]);
    assert_eq!(report.checked_blobs, 1);
    assert!(report.missing_blobs.is_empty());
    assert!(report.corrupt_blobs.is_empty());
}

#[derive(Debug)]
struct InterruptBeforeCommit;

impl RepositoryFailpoint for InterruptBeforeCommit {
    fn before_commit(&self) -> Result<(), StorageError> {
        Err(StorageError::InjectedFailure {
            point: "before_commit",
        })
    }
}

#[tokio::test]
async fn busy_timeout_is_bounded_and_diagnosable() {
    let data_dir = tempfile::tempdir().expect("create engine data directory");
    let config = MemoryConfig::local(data_dir.path()).with_busy_timeout(Duration::from_millis(10));
    let engine = MemoryEngine::open(config)
        .await
        .expect("open memory engine");
    let lock_store = SqliteStore::open(&SqliteConfig::new(data_dir.path().join("memory.sqlite3")))
        .await
        .expect("open lock store");
    let lock = lock_store
        .pool()
        .begin_with("BEGIN IMMEDIATE")
        .await
        .expect("hold database write lock");

    let result = tokio::time::timeout(
        Duration::from_secs(1),
        authorized_observe(&engine, common::observation("blocked observation")),
    )
    .await
    .expect("busy retries must be bounded");
    assert!(matches!(result, Err(MemoryError::Busy { .. })));

    lock.rollback().await.expect("release database write lock");
    let receipt = authorized_observe(&engine, common::observation("write after lock release"))
        .await
        .expect("observe after releasing lock");
    assert_eq!(receipt.sequence, 0);
}

#[tokio::test]
async fn identity_conflict_has_a_distinct_error_category() {
    let data_dir = tempfile::tempdir().expect("create engine data directory");
    let engine = MemoryEngine::open(MemoryConfig::local(data_dir.path()))
        .await
        .expect("open memory engine");
    let fragment = common::observation("original immutable content");
    authorized_observe(&engine, fragment.clone())
        .await
        .expect("observe original fragment");
    let mut conflicting = fragment;
    conflicting.content = FragmentContent::Text {
        text: "conflicting immutable content".to_owned(),
    };

    assert!(matches!(
        authorized_observe(&engine, conflicting).await,
        Err(MemoryError::Conflict {
            entity: "fragment",
            ..
        })
    ));
}

#[tokio::test]
async fn sqlite_constraint_has_a_distinct_error_category_and_rolls_back() {
    let data_dir = tempfile::tempdir().expect("create engine data directory");
    let engine = MemoryEngine::open(MemoryConfig::local(data_dir.path()))
        .await
        .expect("open memory engine");
    let mut fragment = common::observation("foreign key must fail");
    fragment.evidence.push(EvidenceRef {
        id: EvidenceId::new(),
        fragment_id: Some(FragmentId::new()),
        event_id: None,
        method: VerificationMethod::Tool,
        note: None,
    });

    assert!(matches!(
        authorized_observe(&engine, fragment).await,
        Err(MemoryError::Constraint { .. })
    ));
    let receipt = authorized_observe(
        &engine,
        common::observation("write after constraint rollback"),
    )
    .await
    .expect("observe after constraint rollback");
    assert_eq!(receipt.sequence, 0);
}

#[tokio::test]
async fn corrupt_authoritative_row_has_a_distinct_error_category() {
    let data_dir = tempfile::tempdir().expect("create engine data directory");
    let engine = MemoryEngine::open(MemoryConfig::local(data_dir.path()))
        .await
        .expect("open memory engine");
    let fragment = common::observation("row to corrupt");
    authorized_observe(&engine, fragment.clone())
        .await
        .expect("observe fragment");
    let store = SqliteStore::open(&SqliteConfig::new(data_dir.path().join("memory.sqlite3")))
        .await
        .expect("open mutation store");
    sqlx::query("UPDATE fragments SET validity = 'unknown' WHERE id = ?1")
        .bind(fragment.id.to_string())
        .execute(store.pool())
        .await
        .expect("corrupt fragment validity");

    assert!(matches!(
        authorized_fragment(&engine, &fragment.scope, fragment.id).await,
        Err(MemoryError::Corrupt { .. })
    ));
}

#[tokio::test]
async fn data_directory_io_has_a_distinct_error_category() {
    let data_dir = tempfile::tempdir().expect("create engine data directory");
    let file_path = data_dir.path().join("not-a-directory");
    tokio::fs::write(&file_path, b"file")
        .await
        .expect("create blocking file");

    assert!(matches!(
        MemoryEngine::open(MemoryConfig::local(file_path)).await,
        Err(MemoryError::DataDirectory(_))
    ));
}

#[tokio::test]
async fn non_busy_failpoint_is_not_retried() {
    let data_dir = tempfile::tempdir().expect("create engine data directory");
    let attempts = Arc::new(AtomicUsize::new(0));
    let engine = MemoryEngine::open_with_repository_failpoint(
        MemoryConfig::local(data_dir.path()),
        Arc::new(CountingFailpoint(Arc::clone(&attempts))),
    )
    .await
    .expect("open failpoint engine");

    assert!(
        authorized_observe(&engine, common::observation("single failpoint attempt"))
            .await
            .is_err()
    );
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
}

#[derive(Debug)]
struct CountingFailpoint(Arc<AtomicUsize>);

impl RepositoryFailpoint for CountingFailpoint {
    fn before_commit(&self) -> Result<(), StorageError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Err(StorageError::InjectedFailure {
            point: "before_commit",
        })
    }
}

async fn authorized_observe(
    engine: &MemoryEngine,
    fragment: CognitiveFragment,
) -> Result<ObserveReceipt, MemoryError> {
    let context = common::access_context(&fragment.scope);
    engine.observe(&context, fragment).await
}

async fn authorized_fragment(
    engine: &MemoryEngine,
    scope: &MemoryScope,
    fragment_id: FragmentId,
) -> Result<CognitiveFragment, MemoryError> {
    let context = common::access_context(scope);
    engine.fragment(&context, fragment_id).await
}
