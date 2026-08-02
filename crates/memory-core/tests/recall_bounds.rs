//! Core 与 `SQLite` 接通后的 Recall 硬预算验收测试。

mod common;

use chrono::{Duration, Utc};
use jc_memory_core::{MemoryConfig, MemoryEngine, RecallLimits, RecallQuery};
use jc_memory_protocol::{FragmentContent, FragmentKind, MemoryScope, NamespaceId, RecallMode};

#[tokio::test]
async fn core_recall_never_returns_more_than_the_node_budget() {
    let data_dir = tempfile::tempdir().expect("创建引擎数据目录");
    let engine = MemoryEngine::open(MemoryConfig::local(data_dir.path()))
        .await
        .expect("打开记忆引擎");
    let scope = MemoryScope::new(NamespaceId::new());
    let context = common::access_context(&scope);
    for index in 0..12 {
        let mut fragment = common::observation_in_scope(
            scope.clone(),
            &format!("deployment pool failure candidate {index}"),
        );
        fragment.kind = FragmentKind::Fact;
        engine
            .observe(&context, fragment)
            .await
            .expect("写入候选事实");
    }
    let mut foreign = common::observation("deployment pool failure foreign scope");
    foreign.kind = FragmentKind::Fact;
    let foreign_context = common::access_context(&foreign.scope);
    engine
        .observe(&foreign_context, foreign)
        .await
        .expect("写入其他 Scope 事实");

    let limits = RecallLimits {
        max_seeds: 3,
        max_neighbors: 2,
        max_hops: 1,
        max_nodes: 3,
        timeout_ms: 100,
    };
    let packet = engine
        .recall(
            &context,
            RecallQuery {
                scope: scope.clone(),
                mode: RecallMode::Semantic,
                text: "deployment pool failure".to_owned(),
                entities: Vec::new(),
                at_time: None,
                limits,
            },
        )
        .await
        .expect("执行有界召回");

    assert!(packet.points.len() <= limits.max_nodes);
    assert!(packet.coverage.seed_count <= limits.max_seeds);
    assert!(packet.coverage.visited_nodes <= limits.max_nodes);
    assert!(packet.coverage.max_depth <= limits.max_hops);
    assert_eq!(packet.episodes.len(), packet.points.len());
    assert!(packet.points.iter().all(|point| {
        point.source_fragments.iter().all(|fragment_id| {
            packet
                .episodes
                .iter()
                .any(|episode| episode.fragment_id == *fragment_id)
        })
    }));
}

#[tokio::test]
async fn graph_points_and_state_versions_share_the_node_budget() {
    let data_dir = tempfile::tempdir().expect("创建 State 预算数据目录");
    let engine = MemoryEngine::open(MemoryConfig::local(data_dir.path()))
        .await
        .expect("打开记忆引擎");
    let scope = MemoryScope::new(NamespaceId::new());
    let context = common::access_context(&scope);
    let now = Utc::now();
    for (index, value) in ["blue_green", "canary", "rolling"].into_iter().enumerate() {
        let mut fragment = common::state_fragment(
            scope.clone(),
            "project.deployment_strategy",
            serde_json::json!(value),
            "构造 State 与 Point 共享预算",
            now - Duration::seconds(30 - i64::try_from(index).unwrap() * 10),
        );
        fragment.content = FragmentContent::Text {
            text: "project.deployment_strategy".to_owned(),
        };
        engine
            .observe(&context, fragment)
            .await
            .expect("写入预算 State");
    }
    let limits = RecallLimits {
        max_seeds: 1,
        max_neighbors: 2,
        max_hops: 1,
        max_nodes: 2,
        timeout_ms: 500,
    };

    let packet = engine
        .recall(
            &context,
            RecallQuery {
                scope: scope.clone(),
                mode: RecallMode::Semantic,
                text: "project.deployment_strategy".to_owned(),
                entities: Vec::new(),
                at_time: None,
                limits,
            },
        )
        .await
        .expect("执行 State 共享预算 Recall");
    let returned_nodes =
        packet.points.len() + packet.current_states.len() + packet.historical_states.len();
    assert_eq!(packet.points.len(), 1);
    assert_eq!(packet.current_states.len(), 1);
    assert!(returned_nodes <= limits.max_nodes);
    assert!(packet.coverage.truncated);

    let state_packet = engine
        .recall(
            &context,
            RecallQuery {
                scope,
                mode: RecallMode::CurrentState,
                text: "project.deployment_strategy".to_owned(),
                entities: Vec::new(),
                at_time: None,
                limits,
            },
        )
        .await
        .expect("执行有界 CurrentState Recall");
    assert_eq!(
        state_packet.current_states.len() + state_packet.historical_states.len(),
        limits.max_nodes
    );
    assert!(state_packet.coverage.truncated);
}
