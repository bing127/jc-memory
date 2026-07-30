//! Bounded recall pipeline behavior.

use chrono::{TimeZone as _, Utc};
use std::collections::BTreeMap;

use jc_memory_activation::{
    IndexedPoint, RecallCancellation, RecallEngine, RecallEngineError, RecallLimits, RecallQuery,
    edge_kinds_for,
};
use jc_memory_graph::{CognitiveEdge, CognitivePoint, EvidenceLink, PointKind};
use jc_memory_protocol::{
    EdgeKind, EventId, FragmentId, MemoryScope, NamespaceId, RecallMode, ValidityStatus,
};
use jc_memory_surface::{CognitiveSurface, SurfaceBoundary, SurfaceVersion};

const ALL_EDGE_KINDS: &[EdgeKind] = &[
    EdgeKind::TemporalBefore,
    EdgeKind::TemporalAfter,
    EdgeKind::Causes,
    EdgeKind::CausedBy,
    EdgeKind::Supports,
    EdgeKind::Contradicts,
    EdgeKind::Supersedes,
    EdgeKind::DependsOn,
    EdgeKind::PartOf,
    EdgeKind::SameEntity,
    EdgeKind::CoActivated,
    EdgeKind::PredictedBy,
    EdgeKind::ValidatedBy,
    EdgeKind::DisprovedBy,
    EdgeKind::RecalledBy,
    EdgeKind::UsedInDecision,
    EdgeKind::ExecutedAs,
    EdgeKind::ResultedIn,
];

#[tokio::test]
async fn recall_never_exceeds_seed_neighbor_hop_or_node_budgets() {
    let scope = MemoryScope::new(NamespaceId::new());
    let points: Vec<_> = (0..6)
        .map(|index| indexed_point(scope.clone(), &format!("deployment node {index}")))
        .collect();
    let edges = vec![
        edge(&points[0], &points[1], 0.9),
        edge(&points[0], &points[2], 0.8),
        edge(&points[1], &points[3], 0.9),
        edge(&points[2], &points[4], 0.9),
        edge(&points[3], &points[5], 0.9),
    ];
    let engine = RecallEngine::try_from_parts(points, edges, []).unwrap();
    let limits = RecallLimits {
        max_seeds: 1,
        max_neighbors: 1,
        max_hops: 1,
        max_nodes: 2,
        timeout_ms: 100,
    };

    let result = engine
        .recall(RecallQuery {
            scope,
            mode: RecallMode::Associative,
            text: "deployment".to_owned(),
            entities: Vec::new(),
            at_time: None,
            limits,
        })
        .await
        .unwrap();

    assert!(result.trace.seed_count <= limits.max_seeds);
    assert!(result.trace.visited_nodes <= limits.max_nodes);
    assert!(result.trace.max_depth <= limits.max_hops);
    assert!(
        result
            .trace
            .expanded_neighbors
            .values()
            .all(|count| *count <= limits.max_neighbors)
    );
    assert!(result.trace.truncated);
}

#[tokio::test]
async fn semantic_seed_sources_merge_without_duplicate_points() {
    let scope = MemoryScope::new(NamespaceId::new());
    let exact = indexed_point(scope.clone(), "postgres migration");
    let full_text = indexed_point(scope.clone(), "postgres database migration playbook");
    let sparse = indexed_point(scope.clone(), "postgresql migrations");
    let expected_ids = [exact.point.id, full_text.point.id, sparse.point.id];
    let engine = RecallEngine::try_from_parts([exact, full_text, sparse], [], []).unwrap();

    let result = engine
        .recall(RecallQuery {
            scope,
            mode: RecallMode::Semantic,
            text: "postgres migration".to_owned(),
            entities: Vec::new(),
            at_time: None,
            limits: limits(8, 0, 0, 8),
        })
        .await
        .unwrap();

    let actual_ids: Vec<_> = result.points.iter().map(|point| point.point_id).collect();
    assert_eq!(actual_ids.len(), 3);
    assert_eq!(
        actual_ids
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        3
    );
    assert!(
        expected_ids
            .iter()
            .all(|point_id| actual_ids.contains(point_id))
    );
}

#[tokio::test]
async fn entity_recall_can_seed_without_query_text() {
    let scope = MemoryScope::new(NamespaceId::new());
    let mut postgres = indexed_point(scope.clone(), "database configuration");
    postgres.entities = vec!["PostgreSQL".to_owned(), "production".to_owned()];
    let expected = postgres.point.id;
    let unrelated = indexed_point(scope.clone(), "deployment playbook");
    let engine = RecallEngine::try_from_parts([postgres, unrelated], [], []).unwrap();

    let result = engine
        .recall(RecallQuery {
            scope,
            mode: RecallMode::Entity,
            text: String::new(),
            entities: vec!["postgresql".to_owned()],
            at_time: None,
            limits: limits(4, 0, 0, 4),
        })
        .await
        .unwrap();

    assert_eq!(
        result
            .points
            .iter()
            .map(|point| point.point_id)
            .collect::<Vec<_>>(),
        vec![expected]
    );
}

#[tokio::test]
async fn equal_seed_scores_sort_by_recency_then_point_id() {
    let scope = MemoryScope::new(NamespaceId::new());
    let older = indexed_point_at(scope.clone(), "same text", 2025);
    let newer_a = indexed_point_at(scope.clone(), "same text", 2026);
    let newer_b = indexed_point_at(scope.clone(), "same text", 2026);
    let mut expected_newer = [newer_a.point.id, newer_b.point.id];
    expected_newer.sort();
    let engine = RecallEngine::try_from_parts([older.clone(), newer_b, newer_a], [], []).unwrap();

    let result = engine
        .recall(RecallQuery {
            scope,
            mode: RecallMode::Exact,
            text: "same text".to_owned(),
            entities: Vec::new(),
            at_time: None,
            limits: limits(3, 0, 0, 3),
        })
        .await
        .unwrap();

    assert_eq!(
        result
            .points
            .iter()
            .map(|point| point.point_id)
            .collect::<Vec<_>>(),
        vec![expected_newer[0], expected_newer[1], older.point.id]
    );
}

#[test]
fn every_recall_mode_has_an_explicit_edge_whitelist() {
    let cases: &[(RecallMode, &[EdgeKind])] = &[
        (RecallMode::Exact, &[]),
        (
            RecallMode::Semantic,
            &[
                EdgeKind::Supports,
                EdgeKind::Contradicts,
                EdgeKind::PartOf,
                EdgeKind::SameEntity,
                EdgeKind::CoActivated,
            ],
        ),
        (
            RecallMode::Temporal,
            &[
                EdgeKind::TemporalBefore,
                EdgeKind::TemporalAfter,
                EdgeKind::Supersedes,
            ],
        ),
        (
            RecallMode::Causal,
            &[
                EdgeKind::Causes,
                EdgeKind::CausedBy,
                EdgeKind::ValidatedBy,
                EdgeKind::DisprovedBy,
                EdgeKind::ResultedIn,
            ],
        ),
        (
            RecallMode::Entity,
            &[
                EdgeKind::PartOf,
                EdgeKind::SameEntity,
                EdgeKind::CoActivated,
            ],
        ),
        (
            RecallMode::Associative,
            &[
                EdgeKind::Supports,
                EdgeKind::PartOf,
                EdgeKind::SameEntity,
                EdgeKind::CoActivated,
            ],
        ),
        (
            RecallMode::Procedural,
            &[
                EdgeKind::DependsOn,
                EdgeKind::ValidatedBy,
                EdgeKind::DisprovedBy,
                EdgeKind::ExecutedAs,
                EdgeKind::ResultedIn,
            ],
        ),
        (
            RecallMode::CurrentState,
            &[
                EdgeKind::Supports,
                EdgeKind::Contradicts,
                EdgeKind::Supersedes,
            ],
        ),
        (
            RecallMode::HistoricalState,
            &[
                EdgeKind::TemporalBefore,
                EdgeKind::TemporalAfter,
                EdgeKind::Contradicts,
                EdgeKind::Supersedes,
            ],
        ),
        (RecallMode::DeepReconstruction, ALL_EDGE_KINDS),
    ];

    for (mode, expected) in cases {
        assert_eq!(edge_kinds_for(*mode), *expected, "{mode:?}");
    }
}

#[tokio::test]
async fn global_priority_keeps_the_strongest_frontier_within_node_budget() {
    let scope = MemoryScope::new(NamespaceId::new());
    let seed = indexed_point(scope.clone(), "unique root query");
    let high_branch = indexed_point(scope.clone(), "alpha branch");
    let medium_branch = indexed_point(scope.clone(), "beta branch");
    let weak_leaf = indexed_point(scope.clone(), "gamma leaf");
    let strong_leaf = indexed_point(scope.clone(), "delta leaf");
    let edges = [
        edge(&seed, &high_branch, 0.9),
        edge(&seed, &medium_branch, 0.8),
        edge(&high_branch, &weak_leaf, 0.1),
        edge(&medium_branch, &strong_leaf, 1.0),
    ];
    let engine = RecallEngine::try_from_parts(
        [
            seed.clone(),
            high_branch,
            medium_branch,
            weak_leaf.clone(),
            strong_leaf.clone(),
        ],
        edges,
        [],
    )
    .unwrap();

    let result = engine
        .recall(RecallQuery {
            scope,
            mode: RecallMode::Associative,
            text: seed.text,
            entities: Vec::new(),
            at_time: None,
            limits: limits(1, 2, 2, 4),
        })
        .await
        .unwrap();
    let point_ids: Vec<_> = result.points.iter().map(|point| point.point_id).collect();

    assert!(point_ids.contains(&strong_leaf.point.id));
    assert!(!point_ids.contains(&weak_leaf.point.id));
    assert!(result.trace.truncated);
}

#[tokio::test]
async fn recall_activates_only_live_surfaces_above_their_threshold() {
    let scope = MemoryScope::new(NamespaceId::new());
    let seed = indexed_point(scope.clone(), "surface seed");
    let companion = indexed_point(scope.clone(), "surface companion");
    let active = surface(
        scope.clone(),
        jc_memory_protocol::SurfaceStatus::Active,
        [(seed.point.id, 0.8), (companion.point.id, 0.6)],
        0.5,
    );
    let inactive = surface(
        scope.clone(),
        jc_memory_protocol::SurfaceStatus::Inactive,
        [(seed.point.id, 1.0)],
        0.1,
    );
    let engine =
        RecallEngine::try_from_parts([seed.clone(), companion], [], [active.clone(), inactive])
            .unwrap();

    let result = engine
        .recall(RecallQuery {
            scope,
            mode: RecallMode::Exact,
            text: seed.text,
            entities: Vec::new(),
            at_time: None,
            limits: limits(1, 0, 0, 4),
        })
        .await
        .unwrap();

    assert_eq!(result.activated_surfaces.len(), 1);
    assert_eq!(result.activated_surfaces[0].surface_id, active.id);
    assert_eq!(result.activated_surfaces[0].version, 1);
    assert!(result.activated_surfaces[0].activation >= 0.5);
}

#[tokio::test]
async fn evidence_gap_expands_locally_even_when_mode_has_no_regular_edges() {
    let scope = MemoryScope::new(NamespaceId::new());
    let mut unverified = indexed_point(scope.clone(), "unverified exact seed");
    unverified.point.validity = ValidityStatus::Unverified;
    let verifier = indexed_point(scope.clone(), "tool verification result");
    let validation = edge_with_kind(&unverified, &verifier, EdgeKind::ValidatedBy, 0.9);
    let engine = RecallEngine::try_from_parts(
        [unverified.clone(), verifier.clone()],
        [validation.clone()],
        [],
    )
    .unwrap();

    let result = engine
        .recall(RecallQuery {
            scope,
            mode: RecallMode::Exact,
            text: unverified.text,
            entities: Vec::new(),
            at_time: None,
            limits: limits(1, 2, 1, 2),
        })
        .await
        .unwrap();

    assert_eq!(result.points.len(), 2);
    assert_eq!(result.points[1].point_id, verifier.point.id);
    assert_eq!(result.points[1].via_edge_id, Some(validation.id));
    assert_eq!(result.trace.completion_nodes, 1);
    assert!(result.trace.visited_nodes <= 2);
    assert!(result.trace.max_depth <= 1);
}

#[tokio::test]
async fn cancelled_recall_stops_before_visiting_nodes() {
    let scope = MemoryScope::new(NamespaceId::new());
    let point = indexed_point(scope.clone(), "cancel me");
    let engine = RecallEngine::try_from_parts([point], [], []).unwrap();
    let cancellation = RecallCancellation::new();
    cancellation.cancel();

    let result = engine
        .recall_with_cancellation(
            RecallQuery {
                scope,
                mode: RecallMode::Exact,
                text: "cancel me".to_owned(),
                entities: Vec::new(),
                at_time: None,
                limits: limits(1, 0, 0, 1),
            },
            &cancellation,
        )
        .await;

    assert_eq!(result.unwrap_err(), RecallEngineError::Cancelled);
}

#[tokio::test]
async fn zero_timeout_fails_before_returning_partial_results() {
    let scope = MemoryScope::new(NamespaceId::new());
    let point = indexed_point(scope.clone(), "timeout query");
    let engine = RecallEngine::try_from_parts([point], [], []).unwrap();
    let mut bounded = limits(1, 0, 0, 1);
    bounded.timeout_ms = 0;

    let result = engine
        .recall(RecallQuery {
            scope,
            mode: RecallMode::Exact,
            text: "timeout query".to_owned(),
            entities: Vec::new(),
            at_time: None,
            limits: bounded,
        })
        .await;

    assert_eq!(result.unwrap_err(), RecallEngineError::TimedOut);
}

#[tokio::test]
async fn historical_recall_does_not_cross_future_points_or_edges() {
    let scope = MemoryScope::new(NamespaceId::new());
    let historical = indexed_point_at(scope.clone(), "historical seed", 2020);
    let future = indexed_point_at(scope.clone(), "future result", 2026);
    let mut future_edge = edge(&historical, &future, 1.0);
    future_edge.valid_from = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
    let engine =
        RecallEngine::try_from_parts([historical.clone(), future], [future_edge], []).unwrap();

    let result = engine
        .recall(RecallQuery {
            scope,
            mode: RecallMode::Associative,
            text: historical.text,
            entities: Vec::new(),
            at_time: Some(Utc.with_ymd_and_hms(2021, 1, 1, 0, 0, 0).unwrap()),
            limits: limits(1, 4, 2, 8),
        })
        .await
        .unwrap();

    assert_eq!(result.points.len(), 1);
    assert_eq!(result.points[0].point_id, historical.point.id);
    assert!(result.edges.is_empty());
}

#[tokio::test]
async fn seed_budget_reports_when_matching_candidates_are_omitted() {
    let scope = MemoryScope::new(NamespaceId::new());
    let points = [
        indexed_point(scope.clone(), "shared seed"),
        indexed_point(scope.clone(), "shared seed"),
        indexed_point(scope.clone(), "shared seed"),
    ];
    let engine = RecallEngine::try_from_parts(points, [], []).unwrap();

    let result = engine
        .recall(RecallQuery {
            scope,
            mode: RecallMode::Exact,
            text: "shared seed".to_owned(),
            entities: Vec::new(),
            at_time: None,
            limits: limits(1, 0, 0, 8),
        })
        .await
        .unwrap();

    assert_eq!(result.trace.seed_count, 1);
    assert_eq!(result.trace.visited_nodes, 1);
    assert!(result.trace.truncated);
}

#[tokio::test]
async fn current_recall_ignores_surface_versions_that_start_in_the_future() {
    let scope = MemoryScope::new(NamespaceId::new());
    let seed = indexed_point(scope.clone(), "future surface seed");
    let mut future_surface = surface(
        scope.clone(),
        jc_memory_protocol::SurfaceStatus::Active,
        [(seed.point.id, 1.0)],
        0.1,
    );
    future_surface.versions[0].valid_from = Utc.with_ymd_and_hms(2099, 1, 1, 0, 0, 0).unwrap();
    let engine = RecallEngine::try_from_parts([seed.clone()], [], [future_surface]).unwrap();

    let result = engine
        .recall(RecallQuery {
            scope,
            mode: RecallMode::Exact,
            text: seed.text,
            entities: Vec::new(),
            at_time: None,
            limits: limits(1, 0, 0, 2),
        })
        .await
        .unwrap();

    assert!(result.activated_surfaces.is_empty());
}

fn indexed_point(scope: MemoryScope, text: &str) -> IndexedPoint {
    indexed_point_at(scope, text, 2026)
}

fn indexed_point_at(scope: MemoryScope, text: &str, year: i32) -> IndexedPoint {
    IndexedPoint {
        point: CognitivePoint {
            id: jc_memory_protocol::PointId::new(),
            scope,
            kind: PointKind::Fact,
            source_fragments: vec![FragmentId::new()],
            occurred_at: Utc.with_ymd_and_hms(year, 7, 29, 12, 0, 0).unwrap(),
            confidence: 0.8,
            validity: ValidityStatus::Verified,
        },
        text: text.to_owned(),
        entities: Vec::new(),
    }
}

fn edge(source: &IndexedPoint, target: &IndexedPoint, weight: f32) -> CognitiveEdge {
    edge_with_kind(source, target, EdgeKind::CoActivated, weight)
}

fn edge_with_kind(
    source: &IndexedPoint,
    target: &IndexedPoint,
    kind: EdgeKind,
    weight: f32,
) -> CognitiveEdge {
    let mut edge = CognitiveEdge::new(
        &source.point,
        &target.point,
        kind,
        EvidenceLink {
            fragment_id: FragmentId::new(),
            event_id: Some(EventId::new()),
            evidence_ref_id: None,
        },
    )
    .unwrap();
    edge.weight = weight;
    edge
}

const fn limits(
    max_seeds: usize,
    max_neighbors: usize,
    max_hops: usize,
    max_nodes: usize,
) -> RecallLimits {
    RecallLimits {
        max_seeds,
        max_neighbors,
        max_hops,
        max_nodes,
        timeout_ms: 100,
    }
}

fn surface(
    scope: MemoryScope,
    status: jc_memory_protocol::SurfaceStatus,
    members: impl IntoIterator<Item = (jc_memory_protocol::PointId, f32)>,
    activation_threshold: f32,
) -> CognitiveSurface {
    CognitiveSurface {
        id: jc_memory_protocol::SurfaceId::new(),
        scope,
        kind: jc_memory_protocol::SurfaceKind::Dynamic,
        versions: vec![SurfaceVersion {
            version: 1,
            semantic_center: vec![0.5, 0.5],
            member_points: BTreeMap::from_iter(members),
            member_edges: Vec::new(),
            activation_threshold,
            valid_from: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
            valid_to: None,
            status,
            confidence: 0.8,
            boundary: SurfaceBoundary {
                min_member_weight: 0.1,
                max_members: 16,
            },
            evidence: vec![EvidenceLink {
                fragment_id: FragmentId::new(),
                event_id: Some(EventId::new()),
                evidence_ref_id: None,
            }],
            model_version_id: Some("fixture-surface-v1".to_owned()),
        }],
    }
}
