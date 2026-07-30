//! M2 核心 API 验收测试。

mod common;

use chrono::Utc;
use jc_memory_core::{
    MemoryConfig, MemoryEngine, MutationCommand, MutationKind, RecallLimits, RecallQuery,
};
use jc_memory_protocol::{
    FragmentContent, FragmentKind, FragmentSource, Perspective, RecallMode, SourceType,
    ValidityStatus, VerificationMethod,
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
    let fragment_id = fragment.id;
    engine.observe(fragment).await.expect("写入权威事实");
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

    let packet = engine.recall(query.clone()).await.expect("召回记忆");
    let explain = engine.explain(query).await.expect("解释召回");
    let stats = engine.stats(&scope).await.expect("读取统计");

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
    let original_id = original.id;
    engine
        .observe(original.clone())
        .await
        .expect("写入原始事实");

    let correct = engine
        .correct(mutation(original_id, "纠正后的事实"))
        .await
        .expect("纠正事实");
    let reinforce = engine
        .reinforce(mutation(original_id, "补强证据"))
        .await
        .expect("补强事实");
    let weaken = engine
        .weaken(mutation(original_id, "降低可信度"))
        .await
        .expect("弱化事实");
    let supersede = engine
        .supersede(mutation(original_id, "替代事实"))
        .await
        .expect("替代事实");
    let contradict = engine
        .contradict(mutation(original_id, "冲突证据"))
        .await
        .expect("记录冲突");
    let forget = engine
        .forget(mutation(original_id, "请求遗忘"))
        .await
        .expect("追加遗忘事件");

    assert_eq!(correct.kind, MutationKind::Correct);
    assert_eq!(reinforce.kind, MutationKind::Reinforce);
    assert_eq!(weaken.kind, MutationKind::Weaken);
    assert_eq!(supersede.kind, MutationKind::Supersede);
    assert_eq!(contradict.kind, MutationKind::Contradict);
    assert_eq!(forget.kind, MutationKind::Forget);
    let forgotten = engine
        .fragment(forget.fragment_id)
        .await
        .expect("读取遗忘记录");
    assert_eq!(forgotten.kind, FragmentKind::Correction);
    assert_eq!(forgotten.validity, ValidityStatus::Invalidated);
    assert_eq!(forgotten.evidence[0].fragment_id, Some(original_id));
    assert_eq!(
        engine
            .fragment(original_id)
            .await
            .expect("重新读取原始事实"),
        original
    );
    let stats = engine.stats(&scope).await.expect("读取变更后统计");
    assert_eq!(stats.fragment_count, 7);
    assert_eq!(stats.event_count, 7);

    let explain = engine
        .explain(RecallQuery {
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
        })
        .await
        .expect("解释补强关系");
    assert_eq!(explain.traversals.len(), 1);
    assert_eq!(
        explain.traversals[0].kind,
        jc_memory_protocol::EdgeKind::Supports
    );
}

fn mutation(target_fragment_id: jc_memory_protocol::FragmentId, text: &str) -> MutationCommand {
    MutationCommand {
        target_fragment_id,
        content: FragmentContent::Text {
            text: text.to_owned(),
        },
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
