//! Snapshot、Restore 与派生层重建验收测试。

#[allow(dead_code)]
mod common;

use std::{path::Path, sync::Arc};

use chrono::{Duration as ChronoDuration, Utc};
use jc_memory_core::{
    MemoryConfig, MemoryEngine, MemoryError, MutationCommand, RecallLimits, RecallQuery,
};
use jc_memory_protocol::{
    EvidenceId, EvidenceRef, FragmentContent, FragmentId, FragmentKind, FragmentSource, Grant,
    MemoryPacket, MemoryScope, NamespaceId, Operation, Perspective, RecallMode, SourceType,
    StateDeclaration, VerificationMethod,
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
async fn rebuild_and_snapshot_restore_keep_trusted_state_and_graph() {
    let data = tempfile::tempdir().expect("创建可信投影源目录");
    let snapshots = tempfile::tempdir().expect("创建可信投影快照目录");
    let scope = MemoryScope::new(NamespaceId::new());
    let context = common::access_context(&scope);
    let engine = MemoryEngine::open(MemoryConfig::local(data.path()))
        .await
        .expect("打开可信投影引擎");
    let fixture = seed_trusted_projection_fixture(&engine, &context, &scope).await;

    let before_packet = recall_current_state(&engine, &context, &scope).await;
    let authority_before = authority_hash(data.path()).await;
    let store = open_store(data.path()).await;
    let contradiction_id = contradiction_edge_id(&store, fixture.contradiction_id).await;
    clear_trusted_projection_rows(&store, fixture.blue_id).await;

    let report = engine
        .rebuild_derived(&context)
        .await
        .expect("重建可信 State 与 Graph 投影");
    assert_eq!(report.completed_stages, RebuildStage::ORDER);
    let rebuilt_packet = recall_current_state(&engine, &context, &scope).await;
    assert_trusted_packet_eq(&rebuilt_packet, &before_packet);
    assert_eq!(
        contradiction_edge_id(&store, fixture.contradiction_id).await,
        contradiction_id
    );
    assert_projection_checkpoints(&store, fixture.blue_id).await;
    assert_eq!(authority_hash(data.path()).await, authority_before);

    let snapshot = engine
        .create_snapshot(&context, snapshots.path(), "trusted-projection")
        .await
        .expect("创建可信投影快照");
    engine
        .observe(
            &context,
            common::state_fragment(
                scope.clone(),
                "project.deployment_strategy",
                serde_json::json!("rolling"),
                "快照后切换为滚动发布",
                fixture.next_state_at,
            ),
        )
        .await
        .expect("快照后追加第三版 State");
    let changed = recall_current_state(&engine, &context, &scope).await;
    assert_eq!(
        changed.current_states[0].value,
        serde_json::json!("rolling")
    );
    drop(store);
    engine.close().await;
    drop(engine);

    let restore =
        MemoryEngine::restore_snapshot(&context, MemoryConfig::local(data.path()), &snapshot.path)
            .await
            .expect("恢复可信投影快照");
    let restored = MemoryEngine::open(MemoryConfig::local(data.path()))
        .await
        .expect("打开恢复后的可信投影引擎");
    let restored_packet = recall_current_state(&restored, &context, &scope).await;
    assert_trusted_packet_eq(&restored_packet, &rebuilt_packet);
    restored
        .rebuild_derived(&context)
        .await
        .expect("恢复后再次重建可信投影");
    let rebuilt_after_restore = recall_current_state(&restored, &context, &scope).await;
    assert_trusted_packet_eq(&rebuilt_after_restore, &rebuilt_packet);
    restored.close().await;
    if let Some(backup) = restore.backup_dir {
        tokio::fs::remove_dir_all(backup)
            .await
            .expect("清理 Restore 测试备份");
    }
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
    let interrupted = RecoveryManager::from_stores(&store, blobs.clone())
        .with_failpoint(Arc::new(StopAfterStateProjection));
    assert!(matches!(
        interrupted.rebuild_derived(&scope).await,
        Err(RecoveryError::InjectedFailure {
            point: "after_state_projection"
        })
    ));

    let resumed = RecoveryManager::from_stores(&store, blobs)
        .rebuild_derived(&scope)
        .await
        .expect("从持久化 Checkpoint 续跑");
    assert_eq!(resumed.resumed_after, Some(RebuildStage::StateProjection));
    assert_eq!(resumed.completed_stages, RebuildStage::ORDER);
}

#[tokio::test]
async fn rebuild_requires_exact_scope_consolidate_grant() {
    let data = tempfile::tempdir().expect("创建重建授权目录");
    let scope = MemoryScope::new(NamespaceId::new());
    let engine = MemoryEngine::open(MemoryConfig::local(data.path()))
        .await
        .expect("打开重建授权引擎");
    let mut allowed = common::access_context(&scope);
    allowed.grants.clear();
    allowed
        .grants
        .insert(Grant::for_scope(Operation::Consolidate, &scope));
    engine
        .rebuild_derived(&allowed)
        .await
        .expect("仅凭精确 Scope Consolidate Grant 重建");
    let mut denied = common::access_context(&scope);
    denied.grants.clear();
    denied.grants.insert(Grant::for_scope(
        Operation::Consolidate,
        &MemoryScope::new(NamespaceId::new()),
    ));

    assert!(matches!(
        engine.rebuild_derived(&denied).await,
        Err(MemoryError::AccessDenied {
            operation: Operation::Consolidate,
            ..
        })
    ));
}

#[tokio::test]
async fn snapshot_rejects_broken_trusted_projection_foreign_keys() {
    let data = tempfile::tempdir().expect("创建派生引用损坏目录");
    let snapshots = tempfile::tempdir().expect("创建派生引用快照目录");
    let scope = MemoryScope::new(NamespaceId::new());
    let context = common::access_context(&scope);
    let engine = MemoryEngine::open(MemoryConfig::local(data.path()))
        .await
        .expect("打开派生引用测试引擎");
    let mut fact = common::observation_in_scope(scope.clone(), "需要验证的派生引用");
    fact.kind = FragmentKind::Fact;
    engine
        .observe(&context, fact)
        .await
        .expect("写入 Graph Point 来源");
    engine
        .recall(
            &context,
            recall_query(scope, RecallMode::Semantic, "需要验证的派生引用"),
        )
        .await
        .expect("生成 Graph Point");
    let store = open_store(data.path()).await;
    let mut connection = store.pool().acquire().await.expect("获取损坏测试连接");
    sqlx::query("PRAGMA foreign_keys = OFF")
        .execute(&mut *connection)
        .await
        .expect("关闭测试连接外键约束");
    sqlx::query("UPDATE points SET source_fragment_id = ?1")
        .bind(FragmentId::new().to_string())
        .execute(&mut *connection)
        .await
        .expect("构造损坏的 Graph 来源引用");
    drop(connection);

    assert!(matches!(
        engine
            .create_snapshot(&context, snapshots.path(), "broken-projection")
            .await,
        Err(MemoryError::Recovery(RecoveryError::Snapshot(
            SnapshotError::IntegrityCheck { .. }
        )))
    ));
    assert!(
        std::fs::read_dir(snapshots.path())
            .expect("读取失败后的快照目录")
            .next()
            .is_none()
    );
}

#[tokio::test]
async fn snapshot_rejects_cross_state_current_version_pointer() {
    let data = tempfile::tempdir().expect("创建 State 语义损坏目录");
    let snapshots = tempfile::tempdir().expect("创建 State 语义快照目录");
    let scope = MemoryScope::new(NamespaceId::new());
    let context = common::access_context(&scope);
    let engine = MemoryEngine::open(MemoryConfig::local(data.path()))
        .await
        .expect("打开 State 语义测试引擎");
    for (key, value) in [("state.alpha", "a"), ("state.beta", "b")] {
        engine
            .observe(
                &context,
                common::state_fragment(
                    scope.clone(),
                    key,
                    serde_json::json!(value),
                    "构造两条独立 State 链",
                    Utc::now(),
                ),
            )
            .await
            .expect("写入独立 State");
    }
    engine
        .recall(
            &context,
            recall_query(scope, RecallMode::CurrentState, "state.alpha"),
        )
        .await
        .expect("生成两条 State 投影");
    let store = open_store(data.path()).await;
    sqlx::query(
        "UPDATE states
         SET current_version_id = (
             SELECT current_version_id FROM states WHERE state_key = 'state.beta'
         )
         WHERE state_key = 'state.alpha'",
    )
    .execute(store.pool())
    .await
    .expect("构造跨 State 当前版本指针");

    assert!(matches!(
        engine
            .create_snapshot(&context, snapshots.path(), "cross-state-current")
            .await,
        Err(MemoryError::Recovery(RecoveryError::Snapshot(
            SnapshotError::IntegrityCheck { .. }
        )))
    ));
}

#[tokio::test]
async fn rebuild_preserves_surface_and_recall_rows_bound_to_graph_ids() {
    let data = tempfile::tempdir().expect("创建 Graph 依赖保留目录");
    let scope = MemoryScope::new(NamespaceId::new());
    let context = common::access_context(&scope);
    let engine = MemoryEngine::open(MemoryConfig::local(data.path()))
        .await
        .expect("打开 Graph 依赖保留引擎");
    let fixture = seed_trusted_projection_fixture(&engine, &context, &scope).await;
    recall_current_state(&engine, &context, &scope).await;
    let store = open_store(data.path()).await;
    let edge_id = contradiction_edge_id(&store, fixture.contradiction_id).await;
    insert_graph_dependents(&store, fixture.graph_target_id, &edge_id).await;

    engine
        .rebuild_derived(&context)
        .await
        .expect("保留 Graph 确定性标识的下游派生行");
    let members: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM surface_members WHERE surface_id = 'rebuild-surface'",
    )
    .fetch_one(store.pool())
    .await
    .expect("统计保留的 Surface Member");
    let activation_edge: Option<String> = sqlx::query_scalar(
        "SELECT via_edge_id FROM recall_activations
         WHERE recall_session_id = 'rebuild-recall'",
    )
    .fetch_one(store.pool())
    .await
    .expect("读取保留的 Recall Activation");
    assert_eq!(members, 1);
    assert_eq!(activation_edge.as_deref(), Some(edge_id.as_str()));
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
struct StopAfterStateProjection;

impl RecoveryFailpoint for StopAfterStateProjection {
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

struct TrustedProjectionFixture {
    blue_id: FragmentId,
    contradiction_id: FragmentId,
    graph_target_id: FragmentId,
    next_state_at: chrono::DateTime<Utc>,
}

async fn seed_trusted_projection_fixture(
    engine: &MemoryEngine,
    context: &jc_memory_protocol::AccessContext,
    scope: &MemoryScope,
) -> TrustedProjectionFixture {
    let now = Utc::now();
    let mut blue = common::state_fragment(
        scope.clone(),
        "project.deployment_strategy",
        serde_json::json!("blue_green"),
        "初始使用蓝绿发布",
        now - ChronoDuration::seconds(40),
    );
    blue.content = FragmentContent::Text {
        text: "部署策略 blue_green".to_owned(),
    };
    let blue_id = blue.id;
    engine.observe(context, blue).await.expect("写入初始 State");
    engine
        .correct(
            context,
            mutation(
                blue_id,
                "部署策略 canary",
                Some(StateDeclaration {
                    key: "project.deployment_strategy".to_owned(),
                    value: serde_json::json!("canary"),
                    reason: "降低全量发布风险".to_owned(),
                }),
                now - ChronoDuration::seconds(30),
            ),
        )
        .await
        .expect("修正为 canary State");
    let mut target = common::observation_in_scope(scope.clone(), "生产数据库使用 SQLite");
    target.kind = FragmentKind::Fact;
    let target_id = target.id;
    engine
        .observe(context, target)
        .await
        .expect("写入 Graph 目标事实");
    let contradiction = engine
        .contradict(
            context,
            mutation(
                target_id,
                "生产数据库配置与记录冲突",
                None,
                now - ChronoDuration::seconds(20),
            ),
        )
        .await
        .expect("写入 Contradicts Edge 来源 Event");
    TrustedProjectionFixture {
        blue_id,
        contradiction_id: contradiction.fragment_id,
        graph_target_id: target_id,
        next_state_at: now - ChronoDuration::seconds(10),
    }
}

async fn insert_graph_dependents(store: &SqliteStore, point_id: FragmentId, edge_id: &str) {
    let scope_key: String = sqlx::query_scalar("SELECT scope_key FROM points WHERE id = ?1")
        .bind(point_id.to_string())
        .fetch_one(store.pool())
        .await
        .expect("读取 Graph 依赖 Scope hash");
    sqlx::query(
        "INSERT INTO surfaces (
            id, scope_key, kind, status, semantic_center_json, activation_threshold,
            confidence, model_version_id, version, valid_from_us, valid_to_us,
            source_evidence_json, created_at_us
         ) VALUES ('rebuild-surface', ?1, 'episodic', 'active', '{}', 0.5,
                   0.8, NULL, 1, 1, NULL, '[]', 1)",
    )
    .bind(&scope_key)
    .execute(store.pool())
    .await
    .expect("写入 Surface Fixture");
    sqlx::query(
        "INSERT INTO surface_members
            (surface_id, point_id, member_weight, version, created_at_us)
         VALUES ('rebuild-surface', ?1, 1.0, 1, 1)",
    )
    .bind(point_id.to_string())
    .execute(store.pool())
    .await
    .expect("写入 Surface Member Fixture");
    sqlx::query(
        "INSERT INTO recall_sessions (
            id, scope_key, intent, query_json, limits_json,
            started_at_us, completed_at_us, status
         ) VALUES ('rebuild-recall', ?1, 'fixture', '{}', '{}', 1, 2, 'completed')",
    )
    .bind(scope_key)
    .execute(store.pool())
    .await
    .expect("写入 Recall Session Fixture");
    sqlx::query(
        "INSERT INTO recall_activations (
            recall_session_id, point_id, activation, depth, via_edge_id, created_at_us
         ) VALUES ('rebuild-recall', ?1, 1.0, 0, ?2, 1)",
    )
    .bind(point_id.to_string())
    .bind(edge_id)
    .execute(store.pool())
    .await
    .expect("写入 Recall Activation Fixture");
}

fn mutation(
    target_fragment_id: FragmentId,
    text: &str,
    state: Option<StateDeclaration>,
    occurred_at: chrono::DateTime<Utc>,
) -> MutationCommand {
    MutationCommand {
        target_fragment_id,
        content: FragmentContent::Text {
            text: text.to_owned(),
        },
        state,
        source: FragmentSource {
            source_type: SourceType::User,
            source_id: "snapshot-restore-test".to_owned(),
            actor_id: Some("operator".to_owned()),
            observer_id: None,
            tool_id: None,
            uri: None,
        },
        perspective: Perspective::FirstPerson,
        occurred_at,
        verification_method: VerificationMethod::Manual,
        reason: "验证可信投影恢复".to_owned(),
    }
}

fn recall_query(scope: MemoryScope, mode: RecallMode, text: &str) -> RecallQuery {
    RecallQuery {
        scope,
        mode,
        text: text.to_owned(),
        entities: Vec::new(),
        at_time: None,
        limits: RecallLimits {
            max_seeds: 16,
            max_neighbors: 16,
            max_hops: 2,
            max_nodes: 32,
            timeout_ms: 1_000,
        },
    }
}

async fn recall_current_state(
    engine: &MemoryEngine,
    context: &jc_memory_protocol::AccessContext,
    scope: &MemoryScope,
) -> MemoryPacket {
    engine
        .recall(
            context,
            recall_query(
                scope.clone(),
                RecallMode::CurrentState,
                "project.deployment_strategy",
            ),
        )
        .await
        .expect("召回当前可信 State")
}

fn assert_trusted_packet_eq(actual: &MemoryPacket, expected: &MemoryPacket) {
    assert_eq!(actual.current_states, expected.current_states);
    assert_eq!(actual.historical_states, expected.historical_states);
    assert_eq!(actual.conflicts, expected.conflicts);
    assert_eq!(actual.evidence, expected.evidence);
}

async fn clear_trusted_projection_rows(store: &SqliteStore, source_fragment_id: FragmentId) {
    let scope_key: String = sqlx::query_scalar("SELECT scope_key FROM fragments WHERE id = ?1")
        .bind(source_fragment_id.to_string())
        .fetch_one(store.pool())
        .await
        .expect("读取投影测试 Scope hash");
    let mut transaction = store.pool().begin().await.expect("开始清理派生投影事务");
    sqlx::query("UPDATE states SET current_version_id = NULL WHERE scope_key = ?1")
        .bind(&scope_key)
        .execute(&mut *transaction)
        .await
        .expect("清空当前 State 指针");
    sqlx::query(
        "UPDATE state_versions SET previous_version_id = NULL
         WHERE state_id IN (SELECT id FROM states WHERE scope_key = ?1)",
    )
    .bind(&scope_key)
    .execute(&mut *transaction)
    .await
    .expect("断开 StateVersion 自引用");
    for statement in [
        "DELETE FROM edges WHERE scope_key = ?1",
        "DELETE FROM points WHERE scope_key = ?1",
        "DELETE FROM state_versions WHERE state_id IN (SELECT id FROM states WHERE scope_key = ?1)",
        "DELETE FROM states WHERE scope_key = ?1",
        "DELETE FROM projection_checkpoints WHERE scope_key = ?1",
        "DELETE FROM fragment_fts WHERE scope_key = ?1",
    ] {
        sqlx::query(statement)
            .bind(&scope_key)
            .execute(&mut *transaction)
            .await
            .expect("清理可信派生行");
    }
    transaction.commit().await.expect("提交派生投影清理");
}

async fn assert_projection_checkpoints(store: &SqliteStore, source_fragment_id: FragmentId) {
    let scope_key: String = sqlx::query_scalar("SELECT scope_key FROM fragments WHERE id = ?1")
        .bind(source_fragment_id.to_string())
        .fetch_one(store.pool())
        .await
        .expect("读取 Checkpoint 测试 Scope hash");
    let target: i64 =
        sqlx::query_scalar("SELECT COALESCE(MAX(sequence), 0) FROM events WHERE scope_key = ?1")
            .bind(&scope_key)
            .fetch_one(store.pool())
            .await
            .expect("读取权威 Event 目标序号");
    let checkpoints: Vec<(String, i64)> = sqlx::query_as(
        "SELECT projector, sequence FROM projection_checkpoints
         WHERE scope_key = ?1 ORDER BY projector",
    )
    .bind(scope_key)
    .fetch_all(store.pool())
    .await
    .expect("读取可信投影 Checkpoint");
    assert_eq!(
        checkpoints,
        vec![("graph".to_owned(), target), ("state".to_owned(), target)]
    );
}

async fn contradiction_edge_id(store: &SqliteStore, source_fragment_id: FragmentId) -> String {
    sqlx::query_scalar(
        "SELECT id FROM edges WHERE source_fragment_id = ?1 AND kind = 'contradicts'",
    )
    .bind(source_fragment_id.to_string())
    .fetch_one(store.pool())
    .await
    .expect("读取确定性 Contradicts Edge")
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
