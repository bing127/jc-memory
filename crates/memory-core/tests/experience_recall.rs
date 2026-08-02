//! Core 巩固经验与认知 Surface 召回契约。

use chrono::Utc;
use jc_memory_core::{
    JobKind, JobSpec, MemoryConfig, MemoryEngine, RecallLimits, RecallQuery, ResourceLimits,
};
use jc_memory_protocol::{
    AccessContext, CognitiveFragment, EvidenceId, EvidenceRef, FragmentContent, FragmentId,
    FragmentKind, FragmentSource, Grant, MemoryScope, NamespaceId, Operation, Perspective,
    RecallMode, SecurityLabel, SourceType, ValidityStatus, VerificationMethod,
};

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

async fn consolidated_fixture() -> (tempfile::TempDir, MemoryEngine, MemoryScope, AccessContext) {
    let directory = tempfile::tempdir().expect("创建经验召回测试目录");
    let engine = MemoryEngine::open(MemoryConfig::local(directory.path()))
        .await
        .expect("打开记忆引擎");
    let scope = MemoryScope::new(NamespaceId::new());
    let context = access_context(&scope);

    let support_a = fragment(
        &scope,
        "support-a",
        "deploy.guard 支持证据 A",
        FragmentKind::Fact,
        ValidityStatus::Unverified,
        Vec::new(),
    );
    let support_b = fragment(
        &scope,
        "support-b",
        "deploy.guard 支持证据 B",
        FragmentKind::Fact,
        ValidityStatus::Unverified,
        Vec::new(),
    );
    observe(&engine, &context, support_a.clone()).await;
    observe(&engine, &context, support_b.clone()).await;

    for item in [
        procedure(&scope, "procedure-a", FragmentKind::Success, true),
        procedure(&scope, "procedure-b", FragmentKind::Failure, false),
        verified(
            &scope,
            "constraint-a",
            "deploy.guard 部署前必须通过回归测试",
            FragmentKind::Constraint,
            &support_a,
        ),
        verified(
            &scope,
            "constraint-b",
            "deploy.guard 部署前必须通过回归测试",
            FragmentKind::Constraint,
            &support_b,
        ),
        verified(
            &scope,
            "preference-a",
            "deploy.guard 偏好渐进式发布",
            FragmentKind::Preference,
            &support_a,
        ),
        verified(
            &scope,
            "preference-b",
            "deploy.guard 偏好渐进式发布",
            FragmentKind::Preference,
            &support_b,
        ),
    ] {
        observe(&engine, &context, item).await;
    }
    for kind in [
        JobKind::Replay,
        JobKind::SurfaceDiscovery,
        JobKind::SemanticPromotion,
    ] {
        let job = engine
            .submit_consolidation(&context, job_spec(scope.clone(), kind))
            .await
            .expect("提交巩固 Job");
        engine
            .run_consolidation(&context, job.id)
            .await
            .expect("运行巩固 Job");
    }

    (directory, engine, scope, context)
}

fn access_context(scope: &MemoryScope) -> AccessContext {
    let mut context = AccessContext::new("experience-recall-test", scope.clone());
    context.grants.extend(
        [
            Operation::Observe,
            Operation::Recall,
            Operation::Consolidate,
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
