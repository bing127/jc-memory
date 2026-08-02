//! Core 巩固经验与认知 Surface 召回契约。

use std::str::FromStr as _;

use chrono::Utc;
use jc_memory_core::{
    JobKind, JobSpec, MemoryConfig, MemoryEngine, RecallLimits, RecallQuery, ResourceLimits,
};
use jc_memory_protocol::{
    AccessContext, CognitiveFragment, EvidenceId, EvidenceRef, FragmentContent, FragmentId,
    FragmentKind, FragmentSource, Grant, MemoryScope, NamespaceId, Operation, Perspective, PointId,
    RecallMode, SecurityLabel, SourceType, SurfaceId, TenantId, ValidityStatus, VerificationMethod,
};
use jc_memory_storage::{SqliteConfig, SqliteStore, verify_authority_hash};
use sha2::{Digest as _, Sha256};

#[tokio::test]
async fn packet_contains_consolidated_memory() {
    let (_directory, engine, scope, context) = consolidated_fixture().await;

    let packet = engine
        .recall(&context, query(scope.clone(), "deploy.guard", 64))
        .await
        .expect("召回巩固记忆");

    assert_eq!(packet.procedures[0].procedure_key, "deploy.guard");
    assert_eq!(packet.constraints.len(), 1);
    assert_eq!(packet.preferences.len(), 1);
    assert!(!packet.activated_surfaces.is_empty());
    assert_eq!(packet.past_failures.len(), 1);
    assert!(packet.recommended_context.contains("deploy.guard"));
    assert!(
        packet.procedures[0]
            .evidence
            .iter()
            .all(|evidence| evidence.event_id.is_some())
    );

    let limited = engine
        .recall(&context, query(scope, "deploy.guard", 10))
        .await
        .expect("按统一节点预算召回");
    let returned_nodes = limited.points.len()
        + limited
            .activated_surfaces
            .iter()
            .map(|surface| 1 + surface.member_points.len())
            .sum::<usize>()
        + limited.procedures.len()
        + limited.constraints.len()
        + limited.preferences.len();
    assert_eq!(limited.activated_surfaces.len(), 1);
    assert!(limited.procedures.is_empty());
    assert!(limited.constraints.is_empty());
    assert!(limited.preferences.is_empty());
    assert!(returned_nodes <= 10);
    assert!(limited.coverage.truncated);
}

#[tokio::test]
async fn identical_procedure_keys_remain_isolated_between_tenants() {
    let directory = tempfile::tempdir().expect("创建 Tenant 经验目录");
    let engine = MemoryEngine::open(MemoryConfig::local(directory.path()))
        .await
        .expect("打开 Tenant 经验引擎");
    let namespace = NamespaceId::new();
    let mut scope_a = MemoryScope::new(namespace);
    scope_a.tenant_id = Some(TenantId::new());
    let mut scope_b = MemoryScope::new(namespace);
    scope_b.tenant_id = Some(TenantId::new());
    let context_a = populate_consolidated_scope(&engine, &scope_a).await;
    let context_b = populate_consolidated_scope(&engine, &scope_b).await;

    let packet_a = engine
        .recall(&context_a, query(scope_a, "deploy.guard", 64))
        .await
        .expect("召回 Tenant A 经验");
    let packet_b = engine
        .recall(&context_b, query(scope_b, "deploy.guard", 64))
        .await
        .expect("召回 Tenant B 经验");
    let evidence_a = packet_a.procedures[0]
        .evidence
        .iter()
        .map(|evidence| evidence.fragment_id)
        .collect::<std::collections::BTreeSet<_>>();
    let evidence_b = packet_b.procedures[0]
        .evidence
        .iter()
        .map(|evidence| evidence.fragment_id)
        .collect::<std::collections::BTreeSet<_>>();

    assert_eq!(packet_a.procedures[0].procedure_key, "deploy.guard");
    assert_eq!(packet_b.procedures[0].procedure_key, "deploy.guard");
    assert!(evidence_a.is_disjoint(&evidence_b));
}

#[tokio::test]
async fn consolidated_packet_survives_restart_and_snapshot_restore() {
    let (directory, engine, scope, context) = consolidated_fixture().await;
    let expected = engine
        .recall(&context, query(scope.clone(), "deploy.guard", 64))
        .await
        .expect("召回重启前 Packet");
    engine.close().await;

    let config = MemoryConfig::local(directory.path());
    let reopened = MemoryEngine::open(config).await.expect("重启记忆引擎");
    let restarted = reopened
        .recall(&context, query(scope.clone(), "deploy.guard", 64))
        .await
        .expect("召回重启后 Packet");
    assert_packet_memory_eq(&restarted, &expected);

    let snapshots = tempfile::tempdir().expect("创建经验 Snapshot 目录");
    let snapshot = reopened
        .create_snapshot(&context, snapshots.path(), "consolidated-memory")
        .await
        .expect("创建经验 Snapshot");
    reopened.close().await;
    let restored_parent = tempfile::tempdir().expect("创建 Restore 目录");
    let restored_path = restored_parent.path().join("current");
    MemoryEngine::restore_snapshot(
        &context,
        MemoryConfig::local(&restored_path),
        &snapshot.path,
    )
    .await
    .expect("恢复经验 Snapshot");
    let restored = MemoryEngine::open(MemoryConfig::local(restored_path))
        .await
        .expect("打开 Restore 引擎");
    let restored_packet = restored
        .recall(&context, query(scope, "deploy.guard", 64))
        .await
        .expect("召回 Restore 后 Packet");
    assert_packet_memory_eq(&restored_packet, &expected);
}

#[tokio::test]
async fn legacy_consolidation_ids_fail_closed_and_explicit_rebuild_normalizes_them() {
    let (directory, engine, scope, context) = consolidated_fixture().await;
    let authority_before = authority_hash(directory.path()).await;
    let store = open_store(directory.path()).await;
    let scope_key: String = sqlx::query_scalar(
        "SELECT scope_key FROM fragments WHERE kind = 'failure' ORDER BY created_at_us LIMIT 1",
    )
    .fetch_one(store.pool())
    .await
    .expect("读取旧标识 Scope");
    let source_fragment_id: String = sqlx::query_scalar(
        "SELECT id FROM fragments WHERE scope_key = ?1 AND kind = 'failure'
         ORDER BY created_at_us LIMIT 1",
    )
    .bind(&scope_key)
    .fetch_one(store.pool())
    .await
    .expect("读取旧 Point 来源");
    let legacy_point_id = "a".repeat(64);
    let legacy_surface_id = "b".repeat(64);
    sqlx::query(
        "INSERT INTO points (
            id, scope_key, kind, source_fragment_id, valid_from_us,
            valid_to_us, confidence, created_at_us
         ) SELECT ?1, scope_key, 'result', id, occurred_at_us,
                  NULL, confidence, created_at_us
           FROM fragments WHERE id = ?2",
    )
    .bind(&legacy_point_id)
    .bind(&source_fragment_id)
    .execute(store.pool())
    .await
    .expect("写入旧 Point ID");
    sqlx::query(
        "INSERT INTO surfaces (
            id, scope_key, kind, status, semantic_center_json,
            activation_threshold, confidence, version, valid_from_us,
            valid_to_us, source_evidence_json, created_at_us
         ) SELECT ?1, scope_key, 'dynamic', 'active', '{}',
                  0.5, confidence, 1, occurred_at_us, NULL, '[]', created_at_us
           FROM fragments WHERE id = ?2",
    )
    .bind(&legacy_surface_id)
    .bind(&source_fragment_id)
    .execute(store.pool())
    .await
    .expect("写入旧 Surface ID");
    sqlx::query(
        "INSERT INTO surface_members (
            surface_id, point_id, member_weight, version, created_at_us
         ) VALUES (?1, ?2, 1.0, 1, 1)",
    )
    .bind(&legacy_surface_id)
    .bind(&legacy_point_id)
    .execute(store.pool())
    .await
    .expect("写入旧 Surface Member");

    assert!(
        engine
            .recall(&context, query(scope.clone(), "deploy.guard", 64))
            .await
            .is_err()
    );

    run_job(&engine, &context, scope.clone(), JobKind::Rebuild).await;
    run_job(&engine, &context, scope.clone(), JobKind::SurfaceDiscovery).await;
    run_job(&engine, &context, scope.clone(), JobKind::SemanticPromotion).await;
    let rebuilt = engine
        .recall(&context, query(scope, "deploy.guard", 64))
        .await
        .expect("召回重建后巩固记忆");
    assert_eq!(rebuilt.procedures[0].procedure_key, "deploy.guard");
    let point_ids: Vec<String> =
        sqlx::query_scalar("SELECT id FROM points WHERE scope_key = ?1 ORDER BY id")
            .bind(&scope_key)
            .fetch_all(store.pool())
            .await
            .expect("读取重建 Point ID");
    let surface_ids: Vec<String> =
        sqlx::query_scalar("SELECT id FROM surfaces WHERE scope_key = ?1 ORDER BY id")
            .bind(&scope_key)
            .fetch_all(store.pool())
            .await
            .expect("读取重建 Surface ID");
    assert!(point_ids.iter().all(|id| PointId::from_str(id).is_ok()));
    assert!(surface_ids.iter().all(|id| SurfaceId::from_str(id).is_ok()));
    let authority_after = authority_hash(directory.path()).await;
    verify_authority_hash(&authority_before, &authority_after).expect("权威哈希保持不变");
}

fn assert_packet_memory_eq(
    actual: &jc_memory_protocol::MemoryPacket,
    expected: &jc_memory_protocol::MemoryPacket,
) {
    assert_eq!(actual.activated_surfaces, expected.activated_surfaces);
    assert_eq!(actual.episodes, expected.episodes);
    assert_eq!(actual.points, expected.points);
    assert_eq!(actual.procedures, expected.procedures);
    assert_eq!(actual.constraints, expected.constraints);
    assert_eq!(actual.preferences, expected.preferences);
    assert_eq!(actual.past_failures, expected.past_failures);
    assert_eq!(actual.evidence, expected.evidence);
}

async fn consolidated_fixture() -> (tempfile::TempDir, MemoryEngine, MemoryScope, AccessContext) {
    let directory = tempfile::tempdir().expect("创建经验召回测试目录");
    let engine = MemoryEngine::open(MemoryConfig::local(directory.path()))
        .await
        .expect("打开记忆引擎");
    let scope = MemoryScope::new(NamespaceId::new());
    let context = populate_consolidated_scope(&engine, &scope).await;
    (directory, engine, scope, context)
}

async fn populate_consolidated_scope(engine: &MemoryEngine, scope: &MemoryScope) -> AccessContext {
    let context = access_context(scope);

    let support_a = fragment(
        scope,
        "support-a",
        "deploy.guard 支持证据 A",
        FragmentKind::Fact,
        ValidityStatus::Unverified,
        Vec::new(),
    );
    let support_b = fragment(
        scope,
        "support-b",
        "deploy.guard 支持证据 B",
        FragmentKind::Fact,
        ValidityStatus::Unverified,
        Vec::new(),
    );
    observe(engine, &context, support_a.clone()).await;
    observe(engine, &context, support_b.clone()).await;

    for item in [
        procedure(scope, "procedure-a", FragmentKind::Success, true),
        procedure(scope, "procedure-b", FragmentKind::Failure, false),
        verified(
            scope,
            "constraint-a",
            "deploy.guard 部署前必须通过回归测试",
            FragmentKind::Constraint,
            &support_a,
        ),
        verified(
            scope,
            "constraint-b",
            "deploy.guard 部署前必须通过回归测试",
            FragmentKind::Constraint,
            &support_b,
        ),
        verified(
            scope,
            "preference-a",
            "deploy.guard 偏好渐进式发布",
            FragmentKind::Preference,
            &support_a,
        ),
        verified(
            scope,
            "preference-b",
            "deploy.guard 偏好渐进式发布",
            FragmentKind::Preference,
            &support_b,
        ),
    ] {
        observe(engine, &context, item).await;
    }
    for kind in [
        JobKind::Replay,
        JobKind::SurfaceDiscovery,
        JobKind::SemanticPromotion,
    ] {
        run_job(engine, &context, scope.clone(), kind).await;
    }

    context
}

async fn run_job(
    engine: &MemoryEngine,
    context: &AccessContext,
    scope: MemoryScope,
    kind: JobKind,
) {
    let job = engine
        .submit_consolidation(context, job_spec(scope, kind))
        .await
        .expect("提交巩固 Job");
    engine
        .run_consolidation(context, job.id)
        .await
        .expect("运行巩固 Job");
}

fn access_context(scope: &MemoryScope) -> AccessContext {
    let mut context = AccessContext::new("experience-recall-test", scope.clone());
    context.grants.extend(
        [
            Operation::Observe,
            Operation::Recall,
            Operation::Consolidate,
            Operation::Export,
            Operation::Admin,
        ]
        .into_iter()
        .map(|operation| Grant::for_scope(operation, scope)),
    );
    context
}

async fn observe(
    engine: &MemoryEngine,
    context: &jc_memory_protocol::AccessContext,
    fragment: CognitiveFragment,
) {
    engine
        .observe(context, fragment)
        .await
        .expect("写入权威 Fragment");
}

fn procedure(
    scope: &MemoryScope,
    source_id: &str,
    kind: FragmentKind,
    succeeded: bool,
) -> CognitiveFragment {
    let mut fragment = fragment(
        scope,
        source_id,
        "deploy.guard 执行结果",
        kind,
        ValidityStatus::Unverified,
        Vec::new(),
    );
    fragment
        .metadata
        .insert("procedure_key".into(), "deploy.guard".into());
    fragment
        .metadata
        .insert("change_magnitude".into(), "small".into());
    fragment
        .metadata
        .insert("succeeded".into(), succeeded.into());
    fragment
}

fn verified(
    scope: &MemoryScope,
    source_id: &str,
    text: &str,
    kind: FragmentKind,
    support: &CognitiveFragment,
) -> CognitiveFragment {
    fragment(
        scope,
        source_id,
        text,
        kind,
        ValidityStatus::Verified,
        vec![EvidenceRef {
            id: EvidenceId::new(),
            fragment_id: Some(support.id),
            event_id: None,
            method: VerificationMethod::Tool,
            note: Some("独立权威支持".into()),
        }],
    )
}

fn fragment(
    scope: &MemoryScope,
    source_id: &str,
    text: &str,
    kind: FragmentKind,
    validity: ValidityStatus,
    evidence: Vec<EvidenceRef>,
) -> CognitiveFragment {
    let now = Utc::now();
    CognitiveFragment {
        id: FragmentId::new(),
        scope: scope.clone(),
        session_id: None,
        trace_id: None,
        kind,
        content: FragmentContent::Text { text: text.into() },
        source: FragmentSource {
            source_type: SourceType::Tool,
            source_id: source_id.into(),
            actor_id: None,
            observer_id: None,
            tool_id: Some(source_id.into()),
            uri: None,
        },
        perspective: Perspective::Tool,
        occurred_at: now,
        observed_at: now,
        confidence: 0.9,
        importance: 0.8,
        validity,
        security_label: SecurityLabel::Internal,
        evidence,
        metadata: serde_json::Map::new(),
    }
}

async fn authority_hash(data_dir: &std::path::Path) -> String {
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

async fn open_store(data_dir: &std::path::Path) -> SqliteStore {
    SqliteStore::open(&SqliteConfig::new(data_dir.join("memory.sqlite3")))
        .await
        .expect("打开测试 SQLite")
}

fn job_spec(scope: MemoryScope, kind: JobKind) -> JobSpec {
    JobSpec {
        kind,
        scope,
        cursor: 0,
        limits: ResourceLimits {
            max_batch_size: 32,
            max_cpu_ms: 1_000,
            max_io_ops: 1_000,
            max_runtime_ms: 5_000,
            max_attempts: 2,
        },
    }
}

fn query(scope: MemoryScope, text: &str, max_nodes: usize) -> RecallQuery {
    RecallQuery {
        scope,
        mode: RecallMode::Procedural,
        text: text.into(),
        entities: Vec::new(),
        at_time: None,
        limits: RecallLimits {
            max_seeds: 32,
            max_neighbors: 32,
            max_hops: 3,
            max_nodes,
            timeout_ms: 1_000,
        },
    }
}
