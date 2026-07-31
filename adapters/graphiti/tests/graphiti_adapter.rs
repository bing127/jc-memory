//! Graphiti temporal edge、inferred edge 和来源版本契约测试。

use chrono::{TimeZone as _, Utc};
use jc_memory_adapter_graphiti::{GraphitiAdapter, GraphitiConfig, GraphitiEdge, GraphitiNode};
use jc_memory_protocol::{
    CognitiveFragment, FragmentContent, FragmentId, FragmentKind, FragmentSource, MemoryScope,
    NamespaceId, Perspective, PointId, PointKind, SecurityLabel, SourceType, ValidityStatus,
};

fn config() -> GraphitiConfig {
    GraphitiConfig::from_yaml(include_str!("fixtures/graphiti-adapter.yaml"))
        .expect("解析 Graphiti 配置")
}

fn scope() -> MemoryScope {
    MemoryScope::new(NamespaceId::new())
}

fn fragment_id() -> FragmentId {
    FragmentId::new()
}

fn node(external_id: &str, point_id: PointId, occurred_at_day: u32) -> GraphitiNode {
    GraphitiNode {
        external_id: external_id.to_owned(),
        entity_type: "Person".to_owned(),
        point_id,
        fragment_id: fragment_id(),
        scope: scope(),
        kind: PointKind::Fact,
        occurred_at: Utc
            .with_ymd_and_hms(2025, 1, occurred_at_day, 1, 2, 3)
            .unwrap(),
        confidence: 0.6,
        validity: ValidityStatus::Unverified,
    }
}

#[test]
fn node_mapping_preserves_entity_point_and_source_provenance() {
    let adapter = GraphitiAdapter::new(config());
    let point_id = PointId::new();
    let node = node("node-alice", point_id, 2);
    let mapped = adapter.map_node(node.clone()).expect("映射 Graphiti Node");

    assert_eq!(mapped.point.id, point_id);
    assert_eq!(mapped.point.source_fragments, vec![node.fragment_id]);
    assert_eq!(mapped.entity.external_id, "node-alice");
    assert_eq!(mapped.entity.entity_type, "Person");
    assert_eq!(mapped.provenance.source_version, "graphiti/v1");
    assert_eq!(mapped.provenance.external_id, "node-alice");
}

#[test]
fn edge_mapping_preserves_temporal_interval_and_keeps_inferred_edge_unverified() {
    let adapter = GraphitiAdapter::new(config());
    let source = node("node-alice", PointId::new(), 2);
    let mut target = node("node-project", PointId::new(), 3);
    target.scope = source.scope.clone();
    let source_point = adapter.map_node(source.clone()).expect("映射源 Node").point;
    let target_point = adapter
        .map_node(target.clone())
        .expect("映射目标 Node")
        .point;
    let valid_at = Utc.with_ymd_and_hms(2025, 2, 1, 1, 2, 3).unwrap();
    let invalid_at = Utc.with_ymd_and_hms(2025, 3, 1, 1, 2, 3).unwrap();
    let candidate = adapter
        .map_edge(
            GraphitiEdge {
                external_id: "edge-1".to_owned(),
                source_node_id: "node-alice".to_owned(),
                target_node_id: "node-project".to_owned(),
                kind: jc_memory_protocol::EdgeKind::Causes,
                valid_at,
                invalid_at: Some(invalid_at),
                confidence: 0.4,
                evidence_fragment_id: source.fragment_id,
                inferred: true,
                model_version_id: Some("graphiti-model-7".to_owned()),
            },
            &source_point,
            &target_point,
        )
        .expect("映射 Graphiti Edge");

    assert_eq!(candidate.edge.valid_from, valid_at);
    assert_eq!(candidate.edge.valid_to, Some(invalid_at));
    assert!(!candidate.edge.human_confirmed);
    assert!(!candidate.edge.tool_verified);
    assert_eq!(
        candidate.edge.model_version_id.as_deref(),
        Some("graphiti-model-7")
    );
    assert_eq!(candidate.provenance.external_id, "edge-1");
    assert!(candidate.provenance.inferred);
}

#[test]
fn fixture_declares_graphiti_contract_without_live_service_claim() {
    let config = config();
    assert_eq!(config.source_version, "graphiti/v1");
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/graphiti-v1.json"))
            .expect("解析 Graphiti Fixture");
    assert_eq!(fixture["verification"], "fixture_only");
}

#[test]
fn helper_fragment_fixture_keeps_graphiti_source_shape_explicit() {
    let fragment = CognitiveFragment {
        id: fragment_id(),
        scope: scope(),
        session_id: None,
        trace_id: None,
        kind: FragmentKind::Fact,
        content: FragmentContent::Text {
            text: "Graphiti evidence".to_owned(),
        },
        source: FragmentSource {
            source_type: SourceType::Import,
            source_id: "graphiti/v1/node/node-alice".to_owned(),
            actor_id: None,
            observer_id: Some("graphiti-fixture".to_owned()),
            tool_id: None,
            uri: None,
        },
        perspective: Perspective::System,
        occurred_at: Utc.with_ymd_and_hms(2025, 1, 2, 1, 2, 3).unwrap(),
        observed_at: Utc.with_ymd_and_hms(2025, 1, 2, 1, 2, 3).unwrap(),
        confidence: 0.0,
        importance: 0.5,
        validity: ValidityStatus::Unverified,
        security_label: SecurityLabel::Internal,
        evidence: Vec::new(),
        metadata: serde_json::Map::new(),
    };
    assert!(fragment.source.source_id.starts_with("graphiti/v1/"));
}
