//! 治理删除、审计、Retention 与 Export 验收测试。

use chrono::Utc;
use jc_memory_events::factory::EventFactory;
use jc_memory_protocol::{
    AccessContext, CognitiveFragment, EvidenceId, EvidenceRef, FragmentContent, FragmentId,
    FragmentKind, FragmentSource, Grant, MemoryScope, NamespaceId, Operation, Perspective,
    SecurityLabel, SourceType, ValidityStatus, VerificationMethod,
};
use jc_memory_security::{
    GovernanceError, GovernanceService, InMemoryKeyProvider, KeyProvider, RetentionPolicy,
};
use sha2::{Digest as _, Sha256};
use zeroize::Zeroizing;

#[test]
fn key_provider_returns_zeroizing_material_and_tracks_rotation_metadata() {
    let mut provider = InMemoryKeyProvider::new();
    provider
        .insert("key-v1", [1_u8; 32], None)
        .expect("写入首个密钥");
    provider
        .insert("key-v2", [2_u8; 32], Some("key-v1"))
        .expect("轮换密钥");

    let key: Zeroizing<[u8; 32]> = provider.get_key("key-v2").expect("读取轮换密钥");
    assert_eq!(*key, [2_u8; 32]);
    let metadata = provider.metadata("key-v2").expect("读取轮换元数据");
    assert_eq!(metadata.rotated_from.as_deref(), Some("key-v1"));
    assert!(metadata.version > provider.metadata("key-v1").expect("读取旧元数据").version);
    let debug_output = format!("{provider:?}");
    assert!(debug_output.contains("[REDACTED]"));
    assert!(!debug_output.contains("1, 1, 1"));
    assert!(!debug_output.contains("2, 2, 2"));
}

#[tokio::test]
async fn retention_requires_a_persisted_plan_before_it_deletes_candidates() {
    let data = tempfile::tempdir().expect("创建 Retention 数据目录");
    let store = SqliteStore::open(&SqliteConfig::new(data.path().join("memory.sqlite3")))
        .await
        .expect("打开 Retention 数据库");
    let blobs = BlobStore::open(data.path().join("blobs"), BlobOptions::plain())
        .await
        .expect("打开 Blob 仓储");
    let repository = SqliteMemoryRepository::from_store(&store);
    let scope = MemoryScope::new(NamespaceId::new());
    let candidate = fragment(
        scope.clone(),
        FragmentContent::Text {
            text: "Retention 候选正文不得写入计划".to_owned(),
        },
        Vec::new(),
    );
    append(&repository, candidate.clone()).await;
    let mut context = AccessContext::new("retention-admin", scope.clone());
    context
        .grants
        .insert(Grant::for_scope(Operation::Delete, &scope));
    let service = GovernanceService::from_stores(&store, blobs);

    let plan = service
        .plan_retention(&context, RetentionPolicy::before(Utc::now()))
        .await
        .expect("生成 Retention 计划");
    assert_eq!(plan.candidates, vec![candidate.id]);
    assert_eq!(
        count(&store, "fragments", "id", &candidate.id.to_string()).await,
        1
    );
    let stored: String = sqlx::query_scalar("SELECT plan_json FROM retention_plans WHERE id = ?1")
        .bind(plan.id.to_string())
        .fetch_one(store.pool())
        .await
        .expect("读取 Retention 计划");
    assert!(!stored.contains("候选正文"));

    let receipts = service
        .execute_retention(&context, &plan)
        .await
        .expect("执行 Retention 计划");
    assert_eq!(receipts.len(), 1);
    assert_eq!(
        count(&store, "fragments", "id", &candidate.id.to_string()).await,
        0
    );
}

#[tokio::test]
async fn retention_execution_requires_delete_grant_before_changing_plan_state() {
    let data = tempfile::tempdir().expect("创建 Retention 授权目录");
    let store = SqliteStore::open(&SqliteConfig::new(data.path().join("memory.sqlite3")))
        .await
        .expect("打开 Retention 授权数据库");
    let blobs = BlobStore::open(data.path().join("blobs"), BlobOptions::plain())
        .await
        .expect("打开 Blob 仓储");
    let scope = MemoryScope::new(NamespaceId::new());
    let mut planner = AccessContext::new("retention-planner", scope.clone());
    planner
        .grants
        .insert(Grant::for_scope(Operation::Delete, &scope));
    let service = GovernanceService::from_stores(&store, blobs);
    let plan = service
        .plan_retention(&planner, RetentionPolicy::before(Utc::now()))
        .await
        .expect("生成空 Retention 计划");
    assert!(plan.candidates.is_empty());
    let unauthorized = AccessContext::new("unauthorized-executor", scope);

    assert!(matches!(
        service.execute_retention(&unauthorized, &plan).await,
        Err(GovernanceError::AccessDenied { .. })
    ));
    let status: String = sqlx::query_scalar("SELECT status FROM retention_plans WHERE id = ?1")
        .bind(plan.id.to_string())
        .fetch_one(store.pool())
        .await
        .expect("读取未授权执行后的计划状态");
    assert_eq!(status, "planned");
}

#[tokio::test]
async fn retention_execution_cannot_read_or_change_another_scope_plan() {
    let data = tempfile::tempdir().expect("创建 Retention 跨域目录");
    let store = SqliteStore::open(&SqliteConfig::new(data.path().join("memory.sqlite3")))
        .await
        .expect("打开 Retention 跨域数据库");
    let blobs = BlobStore::open(data.path().join("blobs"), BlobOptions::plain())
        .await
        .expect("打开 Blob 仓储");
    let scope_a = MemoryScope::new(NamespaceId::new());
    let scope_b = MemoryScope::new(NamespaceId::new());
    let mut planner = AccessContext::new("scope-a-planner", scope_a.clone());
    planner
        .grants
        .insert(Grant::for_scope(Operation::Delete, &scope_a));
    let service = GovernanceService::from_stores(&store, blobs);
    let plan = service
        .plan_retention(&planner, RetentionPolicy::before(Utc::now()))
        .await
        .expect("生成 Scope A 的空计划");
    let mut foreign_executor = AccessContext::new("scope-b-executor", scope_b.clone());
    foreign_executor
        .grants
        .insert(Grant::for_scope(Operation::Delete, &scope_b));

    assert!(matches!(
        service.execute_retention(&foreign_executor, &plan).await,
        Err(GovernanceError::InvalidRetentionPlan)
    ));
    let status: String = sqlx::query_scalar("SELECT status FROM retention_plans WHERE id = ?1")
        .bind(plan.id.to_string())
        .fetch_one(store.pool())
        .await
        .expect("读取跨域执行后的计划状态");
    assert_eq!(status, "planned");
}

#[tokio::test]
async fn retention_rejects_candidates_changed_after_the_plan_was_persisted() {
    let data = tempfile::tempdir().expect("创建 Retention 篡改目录");
    let store = SqliteStore::open(&SqliteConfig::new(data.path().join("memory.sqlite3")))
        .await
        .expect("打开 Retention 数据库");
    let blobs = BlobStore::open(data.path().join("blobs"), BlobOptions::plain())
        .await
        .expect("打开 Blob 仓储");
    let repository = SqliteMemoryRepository::from_store(&store);
    let scope = MemoryScope::new(NamespaceId::new());
    let original = fragment(
        scope.clone(),
        FragmentContent::Text {
            text: "原候选".into(),
        },
        Vec::new(),
    );
    let injected = fragment(
        scope.clone(),
        FragmentContent::Text {
            text: "不得删除".into(),
        },
        Vec::new(),
    );
    append(&repository, original).await;
    append(&repository, injected.clone()).await;
    let mut context = AccessContext::new("retention-admin", scope.clone());
    context
        .grants
        .insert(Grant::for_scope(Operation::Delete, &scope));
    let service = GovernanceService::from_stores(&store, blobs);
    let mut plan = service
        .plan_retention(&context, RetentionPolicy::before(Utc::now()))
        .await
        .expect("生成原始计划");
    plan.candidates = vec![injected.id];

    assert!(matches!(
        service.execute_retention(&context, &plan).await,
        Err(GovernanceError::InvalidRetentionPlan)
    ));
    assert_eq!(
        count(&store, "fragments", "id", &injected.id.to_string()).await,
        1
    );
}

#[tokio::test]
async fn retention_persists_partial_failure_checkpoint_and_resumes_remaining_candidates() {
    let data = tempfile::tempdir().expect("创建 Retention 恢复目录");
    let store = SqliteStore::open(&SqliteConfig::new(data.path().join("memory.sqlite3")))
        .await
        .expect("打开 Retention 恢复数据库");
    let blobs = BlobStore::open(data.path().join("blobs"), BlobOptions::plain())
        .await
        .expect("打开 Blob 仓储");
    let repository = SqliteMemoryRepository::from_store(&store);
    let scope = MemoryScope::new(NamespaceId::new());
    let first = fragment(
        scope.clone(),
        FragmentContent::Text {
            text: "Retention 第一候选正文".into(),
        },
        Vec::new(),
    );
    let second = fragment(
        scope.clone(),
        FragmentContent::Text {
            text: "Retention 第二候选正文".into(),
        },
        Vec::new(),
    );
    append(&repository, first.clone()).await;
    append(&repository, second.clone()).await;
    let mut context = AccessContext::new("retention-admin", scope.clone());
    context
        .grants
        .insert(Grant::for_scope(Operation::Delete, &scope));
    let service = GovernanceService::from_stores(&store, blobs);
    let plan = service
        .plan_retention(&context, RetentionPolicy::before(Utc::now()))
        .await
        .expect("生成多候选 Retention 计划");
    assert_eq!(plan.candidates.len(), 2);
    let missing_id = plan.candidates[1];
    let missing_fragment = if first.id == missing_id {
        first
    } else {
        second
    };
    let mut setup_context = AccessContext::new("retention-setup", scope.clone());
    setup_context
        .grants
        .insert(Grant::for_scope(Operation::Delete, &scope));
    service
        .hard_delete(&setup_context, missing_id)
        .await
        .expect("构造第二候选暂时缺失");

    assert!(matches!(
        service.execute_retention(&context, &plan).await,
        Err(GovernanceError::TargetNotFound)
    ));
    let (status, progress_json): (String, String) =
        sqlx::query_as("SELECT status, progress_json FROM retention_plans WHERE id = ?1")
            .bind(plan.id.to_string())
            .fetch_one(store.pool())
            .await
            .expect("读取 Retention 失败检查点");
    assert_eq!(status, "failed");
    let progress: serde_json::Value =
        serde_json::from_str(&progress_json).expect("解析 Retention 失败检查点");
    assert_eq!(progress["completed_count"], 1);
    assert_eq!(progress["next_candidate_index"], 1);
    assert_eq!(
        progress["failed_target_digest"].as_str().map(str::len),
        Some(64)
    );
    assert!(!progress_json.contains("候选正文"));
    assert!(!progress_json.contains(&missing_id.to_string()));

    append(&repository, missing_fragment).await;
    let receipts = service
        .execute_retention(&context, &plan)
        .await
        .expect("从失败检查点恢复 Retention");
    assert_eq!(receipts.len(), 1);
    let (status, progress_json): (String, String) =
        sqlx::query_as("SELECT status, progress_json FROM retention_plans WHERE id = ?1")
            .bind(plan.id.to_string())
            .fetch_one(store.pool())
            .await
            .expect("读取 Retention 完成检查点");
    assert_eq!(status, "completed");
    let progress: serde_json::Value =
        serde_json::from_str(&progress_json).expect("解析 Retention 完成检查点");
    assert_eq!(progress["completed_count"], 2);
    assert_eq!(progress["next_candidate_index"], 2);
    assert!(progress["failed_target_digest"].is_null());
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn export_writes_manifest_canonical_jsonl_and_referenced_blobs() {
    let data = tempfile::tempdir().expect("创建 Export 数据目录");
    let output = tempfile::tempdir().expect("创建 Export 输出目录");
    let store = SqliteStore::open(&SqliteConfig::new(data.path().join("memory.sqlite3")))
        .await
        .expect("打开 Export 数据库");
    let blobs = BlobStore::open(data.path().join("blobs"), BlobOptions::plain())
        .await
        .expect("打开 Blob 仓储");
    let repository = SqliteMemoryRepository::from_store(&store);
    let scope = MemoryScope::new(NamespaceId::new());
    let blob = blobs.put(b"export blob").await.expect("写 Export Blob");
    let target = fragment(
        scope.clone(),
        FragmentContent::Blob {
            blob_id: blob.id.clone(),
            media_type: "application/octet-stream".to_owned(),
            byte_len: 11,
        },
        Vec::new(),
    );
    append(&repository, target.clone()).await;
    let mut context = AccessContext::new("export-admin", scope.clone());
    context
        .grants
        .insert(Grant::for_scope(Operation::Export, &scope));
    let service = GovernanceService::from_stores(&store, blobs);

    let receipt = service
        .export_scope(&context, output.path(), "authority-export")
        .await
        .expect("导出权威数据");
    let manifest = tokio::fs::read_to_string(receipt.path.join("manifest.json"))
        .await
        .expect("读取 Export Manifest");
    assert!(manifest.contains("authority.jsonl"));
    assert!(manifest.contains(blob.id.as_str()));
    let manifest_value: serde_json::Value = serde_json::from_str(&manifest).expect("解析 Manifest");
    let exported_blob_path = receipt
        .path
        .join(blob_path(std::path::Path::new("blobs"), blob.id.as_str()));
    let exported_blob_bytes = tokio::fs::read(&exported_blob_path)
        .await
        .expect("读取导出 Blob 字节");
    assert_eq!(manifest_value["blobs"][0]["id"], blob.id.as_str());
    let expected_manifest_path = format!(
        "blobs/{}/{}/{}.jcb",
        &blob.id.as_str()[..2],
        &blob.id.as_str()[2..4],
        blob.id.as_str()
    );
    assert_eq!(manifest_value["blobs"][0]["path"], expected_manifest_path);
    assert_eq!(
        manifest_value["blobs"][0]["sha256"],
        hex::encode(Sha256::digest(&exported_blob_bytes))
    );
    assert_eq!(
        manifest_value["blobs"][0]["byte_len"],
        exported_blob_bytes.len()
    );
    let jsonl = tokio::fs::read_to_string(receipt.path.join("authority.jsonl"))
        .await
        .expect("读取 canonical JSONL");
    assert!(
        jsonl
            .lines()
            .all(|line| serde_json::from_str::<serde_json::Value>(line).is_ok())
    );
    assert!(jsonl.contains(&target.id.to_string()));
    let records = jsonl
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("解析 JSONL 行"))
        .collect::<Vec<_>>();
    assert!(records.iter().any(|row| {
        row["table"] == "fragments" && row["record"]["id"] == target.id.to_string()
    }));
    assert!(records.iter().any(|row| {
        row["table"] == "events" && row["record"]["fragment_id"] == target.id.to_string()
    }));
    let fragment_record = records
        .iter()
        .find(|row| row["table"] == "fragments" && row["record"]["id"] == target.id.to_string())
        .map(|row| &row["record"])
        .expect("读取完整 Fragment 记录");
    for field in [
        "id",
        "scope_key",
        "namespace_id",
        "tenant_id",
        "user_id",
        "agent_id",
        "project_id",
        "session_id",
        "trace_id",
        "kind",
        "content_id",
        "source_json",
        "perspective",
        "occurred_at_us",
        "observed_at_us",
        "confidence",
        "importance",
        "validity",
        "security_label",
        "metadata_json",
        "created_at_us",
    ] {
        assert!(
            fragment_record.get(field).is_some(),
            "Fragment 缺少 {field}"
        );
    }
    assert!(fragment_record["source_json"].is_object());
    assert!(fragment_record["metadata_json"].is_object());
    let event_record = records
        .iter()
        .find(|row| {
            row["table"] == "events" && row["record"]["fragment_id"] == target.id.to_string()
        })
        .map(|row| &row["record"])
        .expect("读取完整 Event 记录");
    for field in [
        "id",
        "fragment_id",
        "scope_key",
        "namespace_id",
        "tenant_id",
        "stage",
        "sequence",
        "recorded_at_us",
        "content_hash",
        "payload_json",
    ] {
        assert!(event_record.get(field).is_some(), "Event 缺少 {field}");
    }
    assert!(event_record["payload_json"].is_object());
    let content_record = records
        .iter()
        .find(|row| row["table"] == "fragment_contents")
        .map(|row| &row["record"])
        .expect("读取完整 FragmentContent 记录");
    assert!(content_record["inline_hex"].is_null());
    assert!(
        tokio::fs::try_exists(exported_blob_path)
            .await
            .expect("检查导出 Blob")
    );
}

#[tokio::test]
async fn failed_export_removes_staging_directory() {
    let data = tempfile::tempdir().expect("创建 Export 失败数据目录");
    let output = tempfile::tempdir().expect("创建 Export 失败输出目录");
    let store = SqliteStore::open(&SqliteConfig::new(data.path().join("memory.sqlite3")))
        .await
        .expect("打开 Export 失败数据库");
    let blobs = BlobStore::open(data.path().join("blobs"), BlobOptions::plain())
        .await
        .expect("打开 Blob 仓储");
    let repository = SqliteMemoryRepository::from_store(&store);
    let scope = MemoryScope::new(NamespaceId::new());
    let blob = blobs
        .put(b"missing export blob")
        .await
        .expect("写待损坏 Blob");
    append(
        &repository,
        fragment(
            scope.clone(),
            FragmentContent::Blob {
                blob_id: blob.id.clone(),
                media_type: "application/octet-stream".to_owned(),
                byte_len: 19,
            },
            Vec::new(),
        ),
    )
    .await;
    tokio::fs::remove_file(blob_path(
        data.path().join("blobs").as_path(),
        blob.id.as_str(),
    ))
    .await
    .expect("移除导出引用 Blob");
    let mut context = AccessContext::new("export-admin", scope.clone());
    context
        .grants
        .insert(Grant::for_scope(Operation::Export, &scope));
    let service = GovernanceService::from_stores(&store, blobs);

    service
        .export_scope(&context, output.path(), "failed-export")
        .await
        .expect_err("引用 Blob 缺失必须中断导出");

    let mut entries = tokio::fs::read_dir(output.path())
        .await
        .expect("读取 Export 输出目录");
    assert!(entries.next_entry().await.expect("读取输出项").is_none());
    let audit_result: String = sqlx::query_scalar(
        "SELECT result FROM audit_logs WHERE request_id = ?1 AND operation = 'export'",
    )
    .bind(&context.request_id)
    .fetch_one(store.pool())
    .await
    .expect("读取 Export 失败审计");
    assert_eq!(audit_result, "failed");
}

#[tokio::test]
async fn empty_scope_export_writes_empty_canonical_jsonl() {
    let data = tempfile::tempdir().expect("创建空 Scope Export 数据目录");
    let output = tempfile::tempdir().expect("创建空 Scope Export 输出目录");
    let store = SqliteStore::open(&SqliteConfig::new(data.path().join("memory.sqlite3")))
        .await
        .expect("打开空 Scope Export 数据库");
    let blobs = BlobStore::open(data.path().join("blobs"), BlobOptions::plain())
        .await
        .expect("打开 Blob 仓储");
    let scope = MemoryScope::new(NamespaceId::new());
    let mut context = AccessContext::new("empty-export-admin", scope.clone());
    context
        .grants
        .insert(Grant::for_scope(Operation::Export, &scope));
    let service = GovernanceService::from_stores(&store, blobs);

    let receipt = service
        .export_scope(&context, output.path(), "empty-export")
        .await
        .expect("导出空 Scope");
    let jsonl = tokio::fs::read(receipt.path.join("authority.jsonl"))
        .await
        .expect("读取空 Scope canonical JSONL");
    assert!(jsonl.is_empty());
}

#[tokio::test]
async fn export_without_grant_is_rejected_and_audited_without_content() {
    let data = tempfile::tempdir().expect("创建 Export 拒绝数据目录");
    let output = tempfile::tempdir().expect("创建 Export 拒绝输出目录");
    let store = SqliteStore::open(&SqliteConfig::new(data.path().join("memory.sqlite3")))
        .await
        .expect("打开 Export 拒绝数据库");
    let blobs = BlobStore::open(data.path().join("blobs"), BlobOptions::plain())
        .await
        .expect("打开 Blob 仓储");
    let scope = MemoryScope::new(NamespaceId::new());
    let context = AccessContext::new("unauthorized-exporter", scope);
    let service = GovernanceService::from_stores(&store, blobs);

    assert!(matches!(
        service
            .export_scope(&context, output.path(), "sensitive-export-name")
            .await,
        Err(GovernanceError::AccessDenied { .. })
    ));

    let row: (String, String, String, String) = sqlx::query_as(
        "SELECT principal_id, target_digest, request_id, result
         FROM audit_logs WHERE operation = 'export'",
    )
    .fetch_one(store.pool())
    .await
    .expect("读取 Export 拒绝审计");
    assert_eq!(row.0, "unauthorized-exporter");
    assert_eq!(row.1.len(), 64);
    assert_eq!(row.2, context.request_id);
    assert_eq!(row.3, "denied");
    assert!(!format!("{row:?}").contains("sensitive-export-name"));
}
use jc_memory_storage::{
    BlobOptions, BlobStore, MemoryRepository, SqliteConfig, SqliteMemoryRepository, SqliteStore,
};

#[tokio::test]
async fn delete_without_grant_is_rejected_and_audited_without_content() {
    let data = tempfile::tempdir().expect("创建治理数据目录");
    let store = SqliteStore::open(&SqliteConfig::new(data.path().join("memory.sqlite3")))
        .await
        .expect("打开治理数据库");
    let blobs = BlobStore::open(data.path().join("blobs"), BlobOptions::plain())
        .await
        .expect("打开 Blob 仓储");
    let service = GovernanceService::from_stores(&store, blobs);
    let scope = MemoryScope::new(NamespaceId::new());
    let context = AccessContext::new("unauthorized-principal", scope);
    let target = FragmentId::new();

    let error = service
        .hard_delete(&context, target)
        .await
        .expect_err("无 Delete Grant 必须拒绝");
    assert!(
        matches!(error, GovernanceError::AccessDenied { .. }),
        "{error:?}"
    );

    let row: (String, String, String, String, String) = sqlx::query_as(
        "SELECT principal_id, operation, target_digest, request_id, result
         FROM audit_logs WHERE operation = 'delete'",
    )
    .fetch_one(store.pool())
    .await
    .expect("读取拒绝审计");
    assert_eq!(row.0, "unauthorized-principal");
    assert_eq!(row.1, "delete");
    assert_eq!(row.2.len(), 64);
    assert_eq!(row.3, context.request_id);
    assert_eq!(row.4, "denied");
    assert!(!format!("{row:?}").contains("正文"));
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn authorized_delete_removes_authority_blob_fts_and_graph_then_queues_rebuild() {
    let data = tempfile::tempdir().expect("创建治理数据目录");
    let store = SqliteStore::open(&SqliteConfig::new(data.path().join("memory.sqlite3")))
        .await
        .expect("打开治理数据库");
    let blobs = BlobStore::open(data.path().join("blobs"), BlobOptions::plain())
        .await
        .expect("打开 Blob 仓储");
    let repository = SqliteMemoryRepository::from_store(&store);
    let scope = MemoryScope::new(NamespaceId::new());
    let secret = "删除后不能恢复的正文";
    let blob = blobs.put(secret.as_bytes()).await.expect("写入目标 Blob");
    let target = fragment(
        scope.clone(),
        FragmentContent::Blob {
            blob_id: blob.id.clone(),
            media_type: "text/plain".to_owned(),
            byte_len: u64::try_from(secret.len()).expect("正文长度"),
        },
        Vec::new(),
    );
    append(&repository, target.clone()).await;
    let supporting = fragment(
        scope.clone(),
        FragmentContent::Text {
            text: "保留的独立记录".to_owned(),
        },
        vec![EvidenceRef {
            id: EvidenceId::new(),
            fragment_id: Some(target.id),
            event_id: None,
            method: VerificationMethod::Manual,
            note: Some("目标删除后该引用也必须消失".to_owned()),
        }],
    );
    append(&repository, supporting.clone()).await;
    seed_derived_rows(&store, target.id, supporting.id, secret).await;
    let blob_path = blob_path(&data.path().join("blobs"), blob.id.as_str());
    let target_content_id: String =
        sqlx::query_scalar("SELECT content_id FROM fragments WHERE id = ?1")
            .bind(target.id.to_string())
            .fetch_one(store.pool())
            .await
            .expect("读取目标 Content ID");

    let mut context = AccessContext::new("delete-admin", scope.clone());
    context
        .grants
        .insert(Grant::for_scope(Operation::Delete, &scope));
    let service = GovernanceService::from_stores(&store, blobs);
    let receipt = service
        .hard_delete(&context, target.id)
        .await
        .expect("执行授权 Hard Delete");

    assert_eq!(receipt.target_digest.len(), 64);
    assert_eq!(receipt.request_id, context.request_id);
    assert!(receipt.reconsolidation_queued);
    for (table, column, value) in [
        ("fragments", "id", target.id.to_string()),
        ("events", "fragment_id", target.id.to_string()),
        (
            "evidence_refs",
            "referenced_fragment_id",
            target.id.to_string(),
        ),
        ("points", "source_fragment_id", target.id.to_string()),
        ("edges", "source_fragment_id", target.id.to_string()),
        ("fragment_fts", "fragment_id", target.id.to_string()),
    ] {
        assert_eq!(count(&store, table, column, &value).await, 0, "{table}");
    }
    assert_eq!(
        count(&store, "fragment_contents", "id", &target_content_id).await,
        0
    );
    assert!(
        !tokio::fs::try_exists(&blob_path)
            .await
            .expect("检查 Blob 文件")
    );
    let queued: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM consolidation_jobs
         WHERE scope_key = (SELECT scope_key FROM fragments WHERE id = ?1)
           AND kind = 'rebuild' AND status = 'pending'",
    )
    .bind(supporting.id.to_string())
    .fetch_one(store.pool())
    .await
    .expect("检查重建 Job");
    assert_eq!(queued, 1);
    let audit_json: String = sqlx::query_scalar(
        "SELECT json_object('principal', principal_id, 'operation', operation,
                'target', target_digest, 'request', request_id, 'result', result)
         FROM audit_logs WHERE request_id = ?1 AND result = 'succeeded'",
    )
    .bind(&context.request_id)
    .fetch_one(store.pool())
    .await
    .expect("读取成功审计");
    assert!(!audit_json.contains(secret));
    assert!(!audit_json.contains(blob.id.as_str()));
    let receipt_json: String =
        sqlx::query_scalar("SELECT receipt_json FROM governance_receipts WHERE request_id = ?1")
            .bind(&context.request_id)
            .fetch_one(store.pool())
            .await
            .expect("读取无正文凭证");
    assert!(!receipt_json.contains(secret));
    assert!(!receipt_json.contains(blob.id.as_str()));
}

#[tokio::test]
async fn blob_cleanup_failure_records_recoverable_failed_delete_without_succeeded_receipt() {
    let data = tempfile::tempdir().expect("创建治理失败数据目录");
    let store = SqliteStore::open(&SqliteConfig::new(data.path().join("memory.sqlite3")))
        .await
        .expect("打开治理失败数据库");
    let blobs = BlobStore::open(data.path().join("blobs"), BlobOptions::plain())
        .await
        .expect("打开 Blob 仓储");
    let repository = SqliteMemoryRepository::from_store(&store);
    let scope = MemoryScope::new(NamespaceId::new());
    let secret = "失败凭证不得包含的正文";
    let blob = blobs.put(secret.as_bytes()).await.expect("写待失败 Blob");
    let target = fragment(
        scope.clone(),
        FragmentContent::Blob {
            blob_id: blob.id.clone(),
            media_type: "text/plain".to_owned(),
            byte_len: u64::try_from(secret.len()).expect("正文长度"),
        },
        Vec::new(),
    );
    append(&repository, target.clone()).await;
    let original_path = blob_path(data.path().join("blobs").as_path(), blob.id.as_str());
    tokio::fs::remove_file(&original_path)
        .await
        .expect("移除 Blob 文件以构造目录故障");
    tokio::fs::create_dir(&original_path)
        .await
        .expect("创建不可 remove_file 的 Blob 目录");
    let mut context = AccessContext::new("delete-admin", scope.clone());
    context
        .grants
        .insert(Grant::for_scope(Operation::Delete, &scope));
    let service = GovernanceService::from_stores(&store, blobs);

    assert!(matches!(
        service.hard_delete(&context, target.id).await,
        Err(GovernanceError::Io(_))
    ));

    let (result, receipt_json): (String, String) = sqlx::query_as(
        "SELECT result, receipt_json FROM governance_receipts WHERE request_id = ?1",
    )
    .bind(&context.request_id)
    .fetch_one(store.pool())
    .await
    .expect("读取清理失败凭证");
    assert_eq!(result, "failed");
    assert!(receipt_json.contains("cleanup_pending"));
    assert!(!receipt_json.contains(secret));
    assert!(!receipt_json.contains(blob.id.as_str()));
    let audit_results: Vec<String> = sqlx::query_scalar(
        "SELECT result FROM audit_logs WHERE request_id = ?1 AND operation = 'delete'",
    )
    .bind(&context.request_id)
    .fetch_all(store.pool())
    .await
    .expect("读取清理失败审计");
    assert_eq!(audit_results, vec!["failed"]);
    assert_eq!(
        count(&store, "fragments", "id", &target.id.to_string()).await,
        0
    );
    assert!(
        !tokio::fs::try_exists(&original_path)
            .await
            .expect("检查原 Blob 路径")
    );
    assert!(
        tokio::fs::try_exists(data.path().join(".governance-trash"))
            .await
            .expect("检查可恢复治理暂存区")
    );
}

fn fragment(
    scope: MemoryScope,
    content: FragmentContent,
    evidence: Vec<EvidenceRef>,
) -> CognitiveFragment {
    let now = Utc::now();
    CognitiveFragment {
        id: FragmentId::new(),
        scope,
        session_id: None,
        trace_id: None,
        kind: FragmentKind::Observation,
        content,
        source: FragmentSource {
            source_type: SourceType::System,
            source_id: "governance-test".to_owned(),
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
        evidence,
        metadata: serde_json::Map::new(),
    }
}

async fn append(repository: &SqliteMemoryRepository, fragment: CognitiveFragment) {
    let draft = EventFactory::new()
        .from_fragment(fragment)
        .expect("构造权威 Event");
    repository.append(&draft).await.expect("写入权威 Event");
}

async fn seed_derived_rows(
    store: &SqliteStore,
    target: FragmentId,
    supporting: FragmentId,
    secret: &str,
) {
    let target_scope: String = sqlx::query_scalar("SELECT scope_key FROM fragments WHERE id = ?1")
        .bind(target.to_string())
        .fetch_one(store.pool())
        .await
        .expect("读取 Scope key");
    for (id, source) in [("point-target", target), ("point-support", supporting)] {
        sqlx::query(
            "INSERT INTO points
             (id, scope_key, kind, source_fragment_id, valid_from_us, valid_to_us,
              confidence, created_at_us)
             VALUES (?1, ?2, 'observation', ?3, 1, NULL, 1.0, 1)",
        )
        .bind(id)
        .bind(&target_scope)
        .bind(source.to_string())
        .execute(store.pool())
        .await
        .expect("写入派生 Point");
    }
    sqlx::query(
        "INSERT INTO edges
         (id, scope_key, source_point_id, target_point_id, kind, weight, confidence,
          source_fragment_id, valid_from_us, valid_to_us, model_version_id,
          human_confirmed, tool_verified, created_at_us)
         VALUES ('edge-target', ?1, 'point-target', 'point-support', 'co_activated',
                 0.5, 0.5, ?2, 1, NULL, NULL, 0, 0, 1)",
    )
    .bind(&target_scope)
    .bind(target.to_string())
    .execute(store.pool())
    .await
    .expect("写入派生 Edge");
    sqlx::query(
        "INSERT INTO surfaces
         (id, scope_key, kind, status, semantic_center_json, activation_threshold,
          confidence, model_version_id, version, valid_from_us, valid_to_us,
          source_evidence_json, created_at_us)
         VALUES ('surface-target', ?1, 'episode', 'active', '[]', 0.5, 0.5,
                 NULL, 1, 1, NULL, '[]', 1)",
    )
    .bind(&target_scope)
    .execute(store.pool())
    .await
    .expect("写入派生 Surface");
    sqlx::query(
        "INSERT INTO surface_members
         (surface_id, point_id, member_weight, version, created_at_us)
         VALUES ('surface-target', 'point-target', 1.0, 1, 1)",
    )
    .execute(store.pool())
    .await
    .expect("写入 Surface member");
    sqlx::query("INSERT INTO fragment_fts (fragment_id, scope_key, body) VALUES (?1, ?2, ?3)")
        .bind(target.to_string())
        .bind(target_scope)
        .bind(secret)
        .execute(store.pool())
        .await
        .expect("写入派生 FTS");
}

async fn count(store: &SqliteStore, table: &str, column: &str, value: &str) -> i64 {
    let query = format!("SELECT COUNT(*) FROM {table} WHERE {column} = ?1");
    sqlx::query_scalar(sqlx::AssertSqlSafe(query))
        .bind(value)
        .fetch_one(store.pool())
        .await
        .expect("统计治理删除残留")
}

fn blob_path(root: &std::path::Path, id: &str) -> std::path::PathBuf {
    root.join(&id[..2])
        .join(&id[2..4])
        .join(format!("{id}.jcb"))
}
