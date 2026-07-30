//! Prediction、Inference 和 Plan 隔离验收测试。

mod common;

use jc_memory_core::{MemoryConfig, MemoryEngine, RecallLimits, RecallQuery};
use jc_memory_protocol::{
    EventId, FragmentId, FragmentKind, MemoryScope, NamespaceId, RecallMode, ValidityStatus,
};
use jc_memory_semantic::{SemanticEntry, SemanticEvidence, SemanticKind, SemanticPromotionError};
use serde_json::json;

#[test]
fn predictions_cannot_be_promoted_to_verified_fact() {
    let scope = MemoryScope::new(NamespaceId::new());
    let evidence = [FragmentKind::Prediction, FragmentKind::Inference]
        .into_iter()
        .map(|kind| SemanticEvidence {
            scope: scope.clone(),
            event_id: EventId::new(),
            fragment_id: FragmentId::new(),
            kind,
            validity: ValidityStatus::Verified,
            confidence: 1.0,
        })
        .collect();

    assert!(matches!(
        SemanticEntry::promote(scope, SemanticKind::Fact, json!("未来会成功"), evidence),
        Err(SemanticPromotionError::HypothesisCannotBecomeFact { .. })
    ));
}

#[tokio::test]
async fn prediction_cannot_be_verified_or_returned_as_fact_point() {
    let data_dir = tempfile::tempdir().expect("创建引擎数据目录");
    let engine = MemoryEngine::open(MemoryConfig::local(data_dir.path()))
        .await
        .expect("打开记忆引擎");
    let mut prediction = common::observation("下次部署将成功");
    prediction.kind = FragmentKind::Prediction;
    prediction.validity = ValidityStatus::Verified;
    let scope = prediction.scope.clone();
    let fragment_id = prediction.id;
    let context = common::access_context(&scope);
    assert!(engine.observe(&context, prediction.clone()).await.is_err());
    prediction.validity = ValidityStatus::Unverified;
    engine
        .observe(&context, prediction)
        .await
        .expect("写入未验证预测");

    let packet = engine
        .recall(
            &context,
            RecallQuery {
                scope: scope.clone(),
                mode: RecallMode::Exact,
                text: "下次部署将成功".to_owned(),
                entities: Vec::new(),
                at_time: None,
                limits: limits(4),
            },
        )
        .await
        .expect("召回预测记录");

    assert!(packet.points.is_empty());
    assert!(packet.evidence.is_empty());
    assert_eq!(
        engine
            .fragment(&context, fragment_id)
            .await
            .expect("读取权威预测")
            .kind,
        FragmentKind::Prediction
    );
    assert_eq!(
        engine
            .stats(&context, &scope)
            .await
            .expect("读取权威统计")
            .fragment_count,
        1
    );
}

fn limits(max_nodes: usize) -> RecallLimits {
    RecallLimits {
        max_seeds: max_nodes,
        max_neighbors: max_nodes,
        max_hops: 2,
        max_nodes,
        timeout_ms: 100,
    }
}
