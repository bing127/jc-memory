//! 跨年份、跨措辞召回验收测试。

mod common;

use chrono::{TimeZone as _, Utc};
use jc_memory_core::{MemoryConfig, MemoryEngine, RecallLimits, RecallQuery};
use jc_memory_protocol::{FragmentKind, RecallMode};

#[tokio::test]
async fn semantic_recall_finds_the_2018_authoritative_fragment_in_2026() {
    let data_dir = tempfile::tempdir().expect("创建引擎数据目录");
    let engine = MemoryEngine::open(MemoryConfig::local(data_dir.path()))
        .await
        .expect("打开记忆引擎");
    let mut historical =
        common::observation("PostgreSQL connection pool exhausted during blue deployment");
    historical.kind = FragmentKind::Fact;
    historical.occurred_at = Utc.with_ymd_and_hms(2018, 3, 4, 10, 0, 0).unwrap();
    historical.observed_at = historical.occurred_at;
    let fragment_id = historical.id;
    let scope = historical.scope.clone();
    engine.observe(historical).await.expect("写入 2018 年事实");

    let packet = engine
        .recall(RecallQuery {
            scope,
            mode: RecallMode::Semantic,
            text: "database deploy pool failure".to_owned(),
            entities: Vec::new(),
            at_time: Some(Utc.with_ymd_and_hms(2026, 7, 30, 12, 0, 0).unwrap()),
            limits: RecallLimits {
                max_seeds: 4,
                max_neighbors: 4,
                max_hops: 2,
                max_nodes: 8,
                timeout_ms: 100,
            },
        })
        .await
        .expect("执行跨年份召回");

    assert!(
        packet
            .points
            .iter()
            .any(|point| point.source_fragments == vec![fragment_id])
    );
    assert!(packet.coverage.visited_nodes <= 8);
}
