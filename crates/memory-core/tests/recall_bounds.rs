//! Core 与 `SQLite` 接通后的 Recall 硬预算验收测试。

mod common;

use jc_memory_core::{MemoryConfig, MemoryEngine, RecallLimits, RecallQuery};
use jc_memory_protocol::{FragmentKind, MemoryScope, NamespaceId, RecallMode};

#[tokio::test]
async fn core_recall_never_returns_more_than_the_node_budget() {
    let data_dir = tempfile::tempdir().expect("创建引擎数据目录");
    let engine = MemoryEngine::open(MemoryConfig::local(data_dir.path()))
        .await
        .expect("打开记忆引擎");
    let scope = MemoryScope::new(NamespaceId::new());
    for index in 0..12 {
        let mut fragment = common::observation_in_scope(
            scope.clone(),
            &format!("deployment pool failure candidate {index}"),
        );
        fragment.kind = FragmentKind::Fact;
        engine.observe(fragment).await.expect("写入候选事实");
    }
    let mut foreign = common::observation("deployment pool failure foreign scope");
    foreign.kind = FragmentKind::Fact;
    engine.observe(foreign).await.expect("写入其他 Scope 事实");

    let limits = RecallLimits {
        max_seeds: 3,
        max_neighbors: 2,
        max_hops: 1,
        max_nodes: 3,
        timeout_ms: 100,
    };
    let packet = engine
        .recall(RecallQuery {
            scope: scope.clone(),
            mode: RecallMode::Semantic,
            text: "deployment pool failure".to_owned(),
            entities: Vec::new(),
            at_time: None,
            limits,
        })
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
