//! M2 核心 API 验收测试。

mod common;

use chrono::Utc;
use jc_memory_core::{
    MemoryConfig, MemoryEngine, MemoryError, MutationCommand, MutationKind, RecallLimits,
    RecallQuery,
};
use jc_memory_protocol::{
    FragmentContent, FragmentKind, FragmentSource, Perspective, RecallMode, SourceType,
    StateDeclaration, ValidityStatus, VerificationMethod,
};

#[tokio::test]
async fn recall_explain_and_stats_use_authoritative_storage() {
    let data_dir = tempfile::tempdir().expect("创建引擎数据目录");
    let engine = MemoryEngine::open(MemoryConfig::local(data_dir.path()))
        .await
        .expect("打开记忆引擎");
    let mut fragment = common::observation("PostgreSQL migration evidence");
    fragment.kind = FragmentKind::Fact;
    let scope = fragment.scope.clone();
    let context = common::access_context(&scope);
    let fragment_id = fragment.id;
    engine
        .observe(&context, fragment)
        .await
        .expect("写入权威事实");
    let query = RecallQuery {
        scope: scope.clone(),
        mode: RecallMode::Exact,
        text: "PostgreSQL migration evidence".to_owned(),
        entities: Vec::new(),
        at_time: None,
        limits: RecallLimits {
            max_seeds: 4,
            max_neighbors: 4,
            max_hops: 2,
            max_nodes: 8,
            timeout_ms: 100,
        },
    };

    let packet = engine
        .recall(&context, query.clone())
        .await
        .expect("召回记忆");
    let explain = engine.explain(&context, query).await.expect("解释召回");
    let stats = engine.stats(&context, &scope).await.expect("读取统计");

    assert_eq!(packet.points.len(), 1);
    assert_eq!(packet.points[0].source_fragments, vec![fragment_id]);
    assert_eq!(packet.evidence[0].fragment_id, fragment_id);
    assert!(!packet.recommended_context.is_empty());
    assert_eq!(explain.seeds.len(), 1);
    assert_eq!(stats.fragment_count, 1);
    assert_eq!(stats.event_count, 1);
}

#[tokio::test]
async fn mutations_append_events_and_forget_never_rewrites_the_original() {
    let data_dir = tempfile::tempdir().expect("创建引擎数据目录");
    let engine = MemoryEngine::open(MemoryConfig::local(data_dir.path()))
        .await
        .expect("打开记忆引擎");
    let mut original = common::observation("需要保留的原始事实");
    original.kind = FragmentKind::Fact;
    let scope = original.scope.clone();
    let context = common::access_context(&scope);
    let original_id = original.id;
    engine
        .observe(&context, original.clone())
        .await
        .expect("写入原始事实");

    let correct = engine
        .correct(&context, mutation(original_id, "纠正后的事实"))
        .await
        .expect("纠正事实");
    let reinforce = engine
        .reinforce(&context, mutation(original_id, "补强证据"))
        .await
        .expect("补强事实");
    let weaken = engine
        .weaken(&context, mutation(original_id, "降低可信度"))
        .await
        .expect("弱化事实");
    let supersede = engine
        .supersede(&context, mutation(original_id, "替代事实"))
        .await
        .expect("替代事实");
    let contradict = engine
        .contradict(&context, mutation(original_id, "冲突证据"))
        .await
        .expect("记录冲突");
    let forget = engine
        .forget(&context, mutation(original_id, "请求遗忘"))
        .await
        .expect("追加遗忘事件");

    assert_eq!(correct.kind, MutationKind::Correct);
    assert_eq!(reinforce.kind, MutationKind::Reinforce);
    assert_eq!(weaken.kind, MutationKind::Weaken);
    assert_eq!(supersede.kind, MutationKind::Supersede);
    assert_eq!(contradict.kind, MutationKind::Contradict);
    assert_eq!(forget.kind, MutationKind::Forget);
    let forgotten = engine
        .fragment(&context, forget.fragment_id)
        .await
        .expect("读取遗忘记录");
    assert_eq!(forgotten.kind, FragmentKind::Correction);
    assert_eq!(forgotten.validity, ValidityStatus::Invalidated);
    assert_eq!(forgotten.evidence[0].fragment_id, Some(original_id));
    assert_eq!(
        engine
            .fragment(&context, original_id)
            .await
            .expect("重新读取原始事实"),
        original
    );
    let stats = engine
        .stats(&context, &scope)
        .await
        .expect("读取变更后统计");
    assert_eq!(stats.fragment_count, 7);
    assert_eq!(stats.event_count, 7);

    let explain = engine
        .explain(
            &context,
            RecallQuery {
                scope,
                mode: RecallMode::Semantic,
                text: "补强证据".to_owned(),
                entities: Vec::new(),
                at_time: None,
                limits: RecallLimits {
                    max_seeds: 1,
                    max_neighbors: 4,
                    max_hops: 1,
                    max_nodes: 4,
                    timeout_ms: 100,
                },
            },
        )
        .await
        .expect("解释补强关系");
    assert_eq!(explain.traversals.len(), 1);
    assert_eq!(
        explain.traversals[0].kind,
        jc_memory_protocol::EdgeKind::Supports
    );
}

#[tokio::test]
async fn invalid_state_mutations_are_rejected_before_authoritative_append() {
    let data_dir = tempfile::tempdir().expect("创建 State Mutation 数据目录");
    let engine = MemoryEngine::open(MemoryConfig::local(data_dir.path()))
        .await
        .expect("打开记忆引擎");
    let mut state = common::state_fragment(
        jc_memory_protocol::MemoryScope::new(jc_memory_protocol::NamespaceId::new()),
        "project.deployment_strategy",
        serde_json::json!("blue_green"),
        "初始部署策略",
        Utc::now(),
    );
    state.content = FragmentContent::Text {
        text: "部署策略 blue_green".to_owned(),
    };
    let scope = state.scope.clone();
    let context = common::access_context(&scope);
    let state_id = state.id;
    engine
        .observe(&context, state)
        .await
        .expect("写入初始 State");
    let before = engine
        .stats(&context, &scope)
        .await
        .expect("读取拒绝前统计");

    let mut mismatched = mutation(state_id, "错误 key 的状态变更");
    mismatched.state = Some(StateDeclaration {
        key: "project.database".to_owned(),
        value: serde_json::json!("sqlite"),
        reason: "不得跨 key 修正状态".to_owned(),
    });
    let error = engine
        .correct(&context, mismatched)
        .await
        .expect_err("拒绝 State key 不一致的 Mutation");
    assert!(matches!(error, MemoryError::InvalidStateMutation { .. }));
    assert_eq!(engine.stats(&context, &scope).await.unwrap(), before);

    let mut ordinary = common::observation_in_scope(scope.clone(), "普通事实目标");
    ordinary.kind = FragmentKind::Fact;
    let ordinary_id = ordinary.id;
    engine
        .observe(&context, ordinary)
        .await
        .expect("写入普通事实");
    let before = engine
        .stats(&context, &scope)
        .await
        .expect("读取第二次拒绝前统计");
    let mut invented = mutation(ordinary_id, "不得凭空附加 State");
    invented.state = Some(StateDeclaration {
        key: "project.deployment_strategy".to_owned(),
        value: serde_json::json!("canary"),
        reason: "普通事实不是 State 目标".to_owned(),
    });
    let error = engine
        .correct(&context, invented)
        .await
        .expect_err("拒绝对非 State 目标附加 State");
    assert!(matches!(error, MemoryError::InvalidStateMutation { .. }));
    assert_eq!(engine.stats(&context, &scope).await.unwrap(), before);
}

fn mutation(target_fragment_id: jc_memory_protocol::FragmentId, text: &str) -> MutationCommand {
    MutationCommand {
        target_fragment_id,
        content: FragmentContent::Text {
            text: text.to_owned(),
        },
        state: None,
        source: FragmentSource {
            source_type: SourceType::User,
            source_id: "memory-api-test".to_owned(),
            actor_id: Some("test-user".to_owned()),
            observer_id: None,
            tool_id: None,
            uri: None,
        },
        perspective: Perspective::FirstPerson,
        occurred_at: Utc::now(),
        verification_method: VerificationMethod::Manual,
        reason: text.to_owned(),
    }
}
