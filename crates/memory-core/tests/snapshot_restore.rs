//! Snapshot、Restore 与派生层重建验收测试。

#[allow(dead_code)]
mod common;

use std::{path::Path, sync::Arc};

use jc_memory_core::{MemoryConfig, MemoryEngine};
use jc_memory_protocol::{
    EvidenceId, EvidenceRef, FragmentContent, MemoryScope, VerificationMethod,
};
use jc_memory_storage::{
    BlobOptions, BlobStore, RebuildStage, RecoveryError, RecoveryFailpoint, RecoveryManager,
    SnapshotError, SnapshotManifest, SqliteConfig, SqliteStore,
};
use sha2::{Digest as _, Sha256};

#[tokio::test]
async fn snapshot_restore_preserves_authoritative_hashes_and_manifest_round_trips() {
    let source = tempfile::tempdir().expect("创建源数据目录");
    let snapshots = tempfile::tempdir().expect("创建快照目录");
    let scope = MemoryScope::new(jc_memory_protocol::NamespaceId::new());
    let context = common::access_context(&scope);
    let engine = MemoryEngine::open(MemoryConfig::local(source.path()))
        .await
        .expect("打开源引擎");
    let first = common::observation_in_scope(scope.clone(), "权威快照基线");
    engine
        .observe(&context, first.clone())
        .await
        .expect("写入首条权威记录");
    let mut second = common::observation_in_scope(scope.clone(), "带 EvidenceRef 的记录");
    second.evidence.push(EvidenceRef {
        id: EvidenceId::new(),
        fragment_id: Some(first.id),
        event_id: None,
        method: VerificationMethod::Manual,
        note: Some("用于验证权威引用不会被派生层覆盖".to_owned()),
    });
    engine
        .observe(&context, second)
        .await
        .expect("写入带引用的权威记录");
    let blob_store = BlobStore::open(source.path().join("blobs"), BlobOptions::plain())
        .await
        .expect("打开 Blob 仓储");
    let blob = blob_store
        .put(b"authoritative blob payload")
        .await
        .expect("写入权威 Blob");
    let mut blob_fragment = common::observation_in_scope(scope.clone(), "Blob 占位正文");
    blob_fragment.content = FragmentContent::Blob {
        blob_id: blob.id,
        media_type: "application/octet-stream".to_owned(),
        byte_len: 26,
    };
    engine
        .observe(&context, blob_fragment)
        .await
        .expect("写入 Blob Fragment");
    let expected = authority_hash(source.path()).await;

    let receipt = engine
        .create_snapshot(&context, snapshots.path(), "baseline")
        .await
        .expect("创建快照");
    assert_eq!(receipt.manifest.scope, scope);
    assert_eq!(receipt.manifest.blobs.len(), 1);
    let encoded = serde_json::to_vec(&receipt.manifest).expect("序列化 Manifest");
    let decoded: SnapshotManifest = serde_json::from_slice(&encoded).expect("反序列化 Manifest");
    assert_eq!(decoded, receipt.manifest);
    drop(engine);

    let restored = tempfile::tempdir().expect("创建 Restore 父目录");
    let restored_path = restored.path().join("current");
    let current = MemoryEngine::open(MemoryConfig::local(&restored_path))
        .await
        .expect("打开待替换引擎");
    current
        .observe(
            &context,
            common::observation_in_scope(scope, "切换前保留在 Backup 的记录"),
        )
        .await
        .expect("写入待替换记录");
    let backup_hash = authority_hash(&restored_path).await;
    drop(current);
    let restore = MemoryEngine::restore_snapshot(
        &context,
        MemoryConfig::local(&restored_path),
        &receipt.path,
    )
    .await
    .expect("恢复已校验快照");
    assert_eq!(authority_hash(&restored_path).await, expected);
    assert_eq!(
        authority_hash(restore.backup_dir.as_deref().expect("保留旧数据目录")).await,
        backup_hash
    );
}

#[tokio::test]
async fn restore_rejects_corrupt_sqlite_without_switching_current_directory() {
    let fixture = snapshot_fixture("sqlite-corruption").await;
    tokio::fs::write(fixture.snapshot.join("memory.sqlite3"), b"corrupt sqlite")
        .await
        .expect("损坏快照 SQLite");
    let current_hash = authority_hash(fixture.current.path()).await;

    let result = MemoryEngine::restore_snapshot(
        &fixture.context,
        MemoryConfig::local(fixture.current.path()),
        &fixture.snapshot,
    )
    .await;
    assert!(matches!(
        result,
        Err(jc_memory_core::MemoryError::Recovery(_))
    ));
    assert_eq!(authority_hash(fixture.current.path()).await, current_hash);
}

#[tokio::test]
async fn restore_rejects_corrupt_blob_without_switching_current_directory() {
    let fixture = snapshot_fixture_with_blob().await;
    let blob_path = first_snapshot_blob(&fixture.snapshot).await;
    tokio::fs::write(blob_path, b"corrupt blob")
        .await
        .expect("损坏快照 Blob");
    let current_hash = authority_hash(fixture.current.path()).await;

    let result = MemoryEngine::restore_snapshot(
        &fixture.context,
        MemoryConfig::local(fixture.current.path()),
        &fixture.snapshot,
    )
    .await;
    assert!(matches!(
        result,
        Err(jc_memory_core::MemoryError::Recovery(_))
    ));
    assert_eq!(authority_hash(fixture.current.path()).await, current_hash);
}

#[tokio::test]
async fn restore_rejects_manifest_scope_mismatch_without_switching_current_directory() {
    let fixture = snapshot_fixture("scope-mismatch").await;
    let foreign_scope = MemoryScope::new(jc_memory_protocol::NamespaceId::new());
    let foreign_context = common::access_context(&foreign_scope);
    let current_hash = authority_hash(fixture.current.path()).await;

    let result = MemoryEngine::restore_snapshot(
        &foreign_context,
        MemoryConfig::local(fixture.current.path()),
        &fixture.snapshot,
    )
    .await;
    assert!(matches!(
        result,
        Err(jc_memory_core::MemoryError::Recovery(
            RecoveryError::Snapshot(SnapshotError::ScopeMismatch)
        ))
    ));
    assert_eq!(authority_hash(fixture.current.path()).await, current_hash);
}

#[tokio::test]
async fn rebuild_restores_fts_in_required_order_without_changing_authority() {
    let data = tempfile::tempdir().expect("创建重建目录");
    let scope = MemoryScope::new(jc_memory_protocol::NamespaceId::new());
    let context = common::access_context(&scope);
    let engine = MemoryEngine::open(MemoryConfig::local(data.path()))
        .await
        .expect("打开重建引擎");
    engine
        .observe(
            &context,
            common::observation_in_scope(scope.clone(), "可重建 FTS 正文"),
        )
        .await
        .expect("写入权威记录");
    let before = authority_hash(data.path()).await;
    let store = open_store(data.path()).await;
    sqlx::query("DELETE FROM fragment_fts")
        .execute(store.pool())
        .await
        .expect("删除派生 FTS");

    let report = engine
        .rebuild_derived(&context)
        .await
        .expect("按阶段重建派生层");
    assert_eq!(report.completed_stages, RebuildStage::ORDER);
    let indexed: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM fragment_fts")
        .fetch_one(store.pool())
        .await
        .expect("统计重建 FTS");
    assert_eq!(indexed, 1);
    assert_eq!(authority_hash(data.path()).await, before);
}

#[tokio::test]
async fn rebuild_resumes_from_durable_checkpoint_after_completed_stage() {
    let data = tempfile::tempdir().expect("创建断点目录");
    let scope = MemoryScope::new(jc_memory_protocol::NamespaceId::new());
    let store = open_store(data.path()).await;
    let blobs = BlobStore::open(data.path().join("blobs"), BlobOptions::plain())
        .await
        .expect("打开 Blob 仓储");
    let interrupted =
        RecoveryManager::from_stores(&store, blobs.clone()).with_failpoint(Arc::new(StopAfterFts));
    assert!(matches!(
        interrupted.rebuild_derived(&scope).await,
        Err(RecoveryError::InjectedFailure { point: "after_fts" })
    ));

    let resumed = RecoveryManager::from_stores(&store, blobs)
        .rebuild_derived(&scope)
        .await
        .expect("从持久化 Checkpoint 续跑");
    assert_eq!(resumed.resumed_after, Some(RebuildStage::Fts));
    assert_eq!(resumed.completed_stages, RebuildStage::ORDER);
}

#[tokio::test]
async fn failed_snapshot_publish_leaves_no_staging_directory() {
    let data = tempfile::tempdir().expect("创建快照源目录");
    let snapshots = tempfile::tempdir().expect("创建快照发布目录");
    let scope = MemoryScope::new(jc_memory_protocol::NamespaceId::new());
    let context = common::access_context(&scope);
    let engine = MemoryEngine::open(MemoryConfig::local(data.path()))
        .await
        .expect("打开快照源引擎");
    let blob_store = BlobStore::open(data.path().join("blobs"), BlobOptions::plain())
        .await
        .expect("打开 Blob 仓储");
    let missing = blob_store
        .put(b"missing during snapshot")
        .await
        .expect("写 Blob");
    let mut fragment = common::observation_in_scope(scope, "Blob 占位正文");
    fragment.content = FragmentContent::Blob {
        blob_id: missing.id.clone(),
        media_type: "application/octet-stream".to_owned(),
        byte_len: 23,
    };
    engine
        .observe(&context, fragment)
        .await
        .expect("写 Blob 引用");
    tokio::fs::remove_file(blob_file(data.path(), missing.id.as_str()))
        .await
        .expect("制造缺失 Blob");

    assert!(matches!(
        engine
            .create_snapshot(&context, snapshots.path(), "incomplete")
            .await,
        Err(jc_memory_core::MemoryError::Recovery(
            RecoveryError::Snapshot(SnapshotError::MissingBlob { .. })
        ))
    ));
    let entries = std::fs::read_dir(snapshots.path())
        .expect("枚举快照目录")
        .collect::<Result<Vec<_>, _>>()
        .expect("读取快照目录项");
    assert!(entries.is_empty());
}

#[tokio::test]
async fn snapshot_rejects_foreign_scope_metadata_without_publishing() {
    let data = tempfile::tempdir().expect("创建快照源目录");
    let snapshots = tempfile::tempdir().expect("创建快照发布目录");
    let scope = MemoryScope::new(jc_memory_protocol::NamespaceId::new());
    let context = common::access_context(&scope);
    let engine = MemoryEngine::open(MemoryConfig::local(data.path()))
        .await
        .expect("打开快照源引擎");
    engine
        .observe(
            &context,
            common::observation_in_scope(scope, "当前 Scope 的权威记录"),
        )
        .await
        .expect("写入当前 Scope");
    let store = open_store(data.path()).await;
    sqlx::query(
        "INSERT INTO audit_logs
         (id, scope_key, principal_id, operation, target_type, target_id,
          detail_digest, created_at_us)
         VALUES ('foreign-audit', ?1, 'foreign-principal', 'recall',
                 'fragment', 'foreign-target', ?2, 1)",
    )
    .bind("f".repeat(64))
    .bind("0".repeat(64))
    .execute(store.pool())
    .await
    .expect("写入其他 Scope 的审计元数据");

    assert!(matches!(
        engine
            .create_snapshot(&context, snapshots.path(), "scope-boundary")
            .await,
        Err(jc_memory_core::MemoryError::Recovery(
            RecoveryError::Snapshot(SnapshotError::ScopeContentsMismatch)
        ))
    ));
    let entries = std::fs::read_dir(snapshots.path())
        .expect("枚举快照目录")
        .collect::<Result<Vec<_>, _>>()
        .expect("读取快照目录项");
    assert!(entries.is_empty());
}

struct SnapshotFixture {
    current: tempfile::TempDir,
    _snapshots: tempfile::TempDir,
    snapshot: std::path::PathBuf,
    context: jc_memory_protocol::AccessContext,
}

async fn snapshot_fixture(name: &str) -> SnapshotFixture {
    let source = tempfile::tempdir().expect("创建快照源目录");
    let snapshots = tempfile::tempdir().expect("创建快照父目录");
    let current = tempfile::tempdir().expect("创建当前数据目录");
    let scope = MemoryScope::new(jc_memory_protocol::NamespaceId::new());
    let context = common::access_context(&scope);
    let source_engine = MemoryEngine::open(MemoryConfig::local(source.path()))
        .await
        .expect("打开快照源引擎");
    source_engine
        .observe(
            &context,
            common::observation_in_scope(scope.clone(), "快照中的权威记录"),
        )
        .await
        .expect("写入快照源");
    let current_engine = MemoryEngine::open(MemoryConfig::local(current.path()))
        .await
        .expect("打开当前引擎");
    current_engine
        .observe(
            &context,
            common::observation_in_scope(scope, "必须保留的当前记录"),
        )
        .await
        .expect("写入当前数据");
    let receipt = source_engine
        .create_snapshot(&context, snapshots.path(), name)
        .await
        .expect("创建测试快照");
    drop(source_engine);
    drop(current_engine);
    let snapshot = receipt.path;
    SnapshotFixture {
        current,
        _snapshots: snapshots,
        snapshot,
        context,
    }
}

async fn snapshot_fixture_with_blob() -> SnapshotFixture {
    let data = tempfile::tempdir().expect("创建 Blob 快照源");
    let snapshots = tempfile::tempdir().expect("创建 Blob 快照目录");
    let current = tempfile::tempdir().expect("创建当前数据目录");
    let scope = MemoryScope::new(jc_memory_protocol::NamespaceId::new());
    let context = common::access_context(&scope);
    let engine = MemoryEngine::open(MemoryConfig::local(data.path()))
        .await
        .expect("打开 Blob 快照源");
    let blobs = BlobStore::open(data.path().join("blobs"), BlobOptions::plain())
        .await
        .expect("打开 Blob 仓储");
    let blob = blobs.put(b"blob snapshot payload").await.expect("写 Blob");
    let mut fragment = common::observation_in_scope(scope.clone(), "Blob 占位正文");
    fragment.content = FragmentContent::Blob {
        blob_id: blob.id,
        media_type: "application/octet-stream".to_owned(),
        byte_len: 21,
    };
    engine
        .observe(&context, fragment)
        .await
        .expect("写 Blob Fragment");
    let current_engine = MemoryEngine::open(MemoryConfig::local(current.path()))
        .await
        .expect("打开当前引擎");
    current_engine
        .observe(
            &context,
            common::observation_in_scope(scope, "当前保留记录"),
        )
        .await
        .expect("写当前记录");
    let receipt = engine
        .create_snapshot(&context, snapshots.path(), "with-blob")
        .await
        .expect("创建 Blob 快照");
    drop(engine);
    drop(current_engine);
    SnapshotFixture {
        current,
        _snapshots: snapshots,
        snapshot: receipt.path,
        context,
    }
}

#[derive(Debug)]
struct StopAfterFts;

impl RecoveryFailpoint for StopAfterFts {
    fn before_fts_swap(&self) -> Result<(), RecoveryError> {
        Ok(())
    }

    fn after_rebuild_stage(&self, stage: RebuildStage) -> Result<(), RecoveryError> {
        if stage == RebuildStage::Fts {
            Err(RecoveryError::InjectedFailure { point: "after_fts" })
        } else {
            Ok(())
        }
    }
}

async fn authority_hash(data_dir: &Path) -> String {
    let store = open_store(data_dir).await;
    let mut digest = Sha256::new();
    for query in [
        "SELECT json_array(id, storage_kind, hex(inline_value), blob_id, media_type,
                byte_len, sha256, created_at_us) FROM fragment_contents ORDER BY id",
        "SELECT json_array(id, scope_key, namespace_id, tenant_id, user_id, agent_id,
                project_id, session_id, trace_id, kind, content_id, source_json, perspective,
                occurred_at_us, observed_at_us, confidence, importance, validity, security_label,
                metadata_json, created_at_us) FROM fragments ORDER BY id",
        "SELECT json_array(id, fragment_id, scope_key, namespace_id, tenant_id,
                stage, sequence, recorded_at_us, content_hash, payload_json)
                FROM events ORDER BY id",
        "SELECT json_array(id, owner_fragment_id, referenced_fragment_id, referenced_event_id,
                verification_method, note, created_at_us) FROM evidence_refs ORDER BY id",
    ] {
        let rows: Vec<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(query))
            .fetch_all(store.pool())
            .await
            .expect("读取权威行");
        for row in rows {
            digest.update(row.as_bytes());
            digest.update([0]);
        }
    }
    hex::encode(digest.finalize())
}

async fn open_store(data_dir: &Path) -> SqliteStore {
    SqliteStore::open(&SqliteConfig::new(data_dir.join("memory.sqlite3")))
        .await
        .expect("打开测试 SQLite")
}

async fn first_snapshot_blob(snapshot: &Path) -> std::path::PathBuf {
    let manifest = SnapshotManifest::read(snapshot)
        .await
        .expect("读取 Snapshot Manifest");
    snapshot.join(&manifest.blobs[0].path)
}

fn blob_file(data_dir: &Path, digest: &str) -> std::path::PathBuf {
    data_dir
        .join("blobs")
        .join(&digest[..2])
        .join(&digest[2..4])
        .join(format!("{digest}.jcb"))
}
