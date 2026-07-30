//! Model-independent Memory Packet and Explain contracts.

use std::collections::BTreeSet;

use chrono::{TimeZone as _, Utc};
use jc_memory_activation::{
    ActivatedPoint, ActivatedSurface, IndexedPoint, RecallEngine, RecallLimits, RecallResult,
    RecallTrace,
};
use jc_memory_graph::{CognitiveEdge, CognitivePoint, EvidenceLink};
use jc_memory_protocol::{
    EdgeKind, EventId, FragmentId, MemoryPacket, MemoryScope, NamespaceId, PointId, PointKind,
    PointView, RecallId, RecallMode, SurfaceId, ValidityStatus,
};

#[test]
fn memory_packet_has_exactly_the_stable_top_level_fields() {
    let packet = MemoryPacket::empty(RecallId::new(), RecallMode::Causal);

    let value = serde_json::to_value(packet).unwrap();
    let actual: BTreeSet<_> = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    let expected = BTreeSet::from([
        "query_id",
        "intent",
        "current_states",
        "historical_states",
        "activated_surfaces",
        "episodes",
        "points",
        "paths",
        "procedures",
        "constraints",
        "preferences",
        "past_failures",
        "evidence",
        "conflicts",
        "missing_evidence",
        "coverage",
        "confidence",
        "recommended_context",
    ]);

    assert_eq!(actual, expected);
}

#[test]
fn recommended_context_is_a_deterministic_model_independent_rendering() {
    let mut packet = MemoryPacket::empty(RecallId::new(), RecallMode::Semantic);
    let point_id = PointId::new();
    packet.points.push(PointView {
        point_id,
        kind: PointKind::Fact,
        confidence: 0.8,
        validity: ValidityStatus::Verified,
        source_fragments: vec![FragmentId::new()],
    });

    packet.refresh_recommended_context().unwrap();
    let first = packet.recommended_context.clone();
    packet.refresh_recommended_context().unwrap();

    assert_eq!(packet.recommended_context, first);
    let rendered: serde_json::Value = serde_json::from_str(&first).unwrap();
    assert_eq!(rendered["points"][0]["point_id"], point_id.to_string());
    assert!(rendered.get("recommended_context").is_none());
    assert!(!first.contains("system_prompt"));
    assert!(!first.contains("<|assistant|>"));
}

#[test]
fn explain_trace_retains_seeds_paths_surfaces_and_evidence_provenance() {
    let scope = MemoryScope::new(NamespaceId::new());
    let verified_fragment = FragmentId::new();
    let seed = indexed_point(
        scope.clone(),
        "database migration",
        ValidityStatus::Verified,
        verified_fragment,
    );
    let target = indexed_point(
        scope,
        "migration result",
        ValidityStatus::Unverified,
        FragmentId::new(),
    );
    let edge = CognitiveEdge::new(
        &seed.point,
        &target.point,
        EdgeKind::Supports,
        EvidenceLink {
            fragment_id: verified_fragment,
            event_id: Some(EventId::new()),
            evidence_ref_id: None,
        },
    )
    .unwrap();
    let engine =
        RecallEngine::try_from_parts([seed.clone(), target.clone()], [edge.clone()], []).unwrap();
    let limits = RecallLimits {
        max_seeds: 1,
        max_neighbors: 2,
        max_hops: 1,
        max_nodes: 2,
        timeout_ms: 100,
    };
    let surface_id = SurfaceId::new();
    let result = RecallResult {
        points: vec![
            ActivatedPoint {
                point_id: seed.point.id,
                activation: 1.0,
                depth: 0,
                via_edge_id: None,
            },
            ActivatedPoint {
                point_id: target.point.id,
                activation: 0.4,
                depth: 1,
                via_edge_id: Some(edge.id),
            },
        ],
        edges: vec![edge.clone()],
        activated_surfaces: vec![ActivatedSurface {
            surface_id,
            version: 3,
            activation: 0.7,
        }],
        trace: RecallTrace {
            seed_count: 1,
            visited_nodes: 2,
            max_depth: 1,
            truncated: true,
            ..RecallTrace::default()
        },
    };

    let explain = engine.explain(&result, limits);

    assert_eq!(explain.seeds[0].point_id, seed.point.id);
    assert_eq!(explain.traversals[0].edge_id, edge.id);
    assert_eq!(explain.traversals[0].source_point_id, seed.point.id);
    assert_eq!(explain.traversals[0].target_point_id, target.point.id);
    assert_eq!(explain.surfaces[0].surface_id, surface_id);
    assert_eq!(explain.inferred_edges, vec![edge.id]);
    assert_eq!(explain.verified_fragments, vec![verified_fragment]);
    assert_eq!(explain.limits, limits);
    assert!(explain.truncated);
    serde_json::to_value(explain).unwrap();
}

fn indexed_point(
    scope: MemoryScope,
    text: &str,
    validity: ValidityStatus,
    fragment_id: FragmentId,
) -> IndexedPoint {
    IndexedPoint {
        point: CognitivePoint {
            id: PointId::new(),
            scope,
            kind: PointKind::Fact,
            source_fragments: vec![fragment_id],
            occurred_at: Utc.with_ymd_and_hms(2026, 7, 30, 12, 0, 0).unwrap(),
            confidence: 0.8,
            validity,
        },
        text: text.to_owned(),
        entities: Vec::new(),
    }
}
