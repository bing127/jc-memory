//! Evidence-linked edge updates and bounded local-graph loading.

use chrono::{TimeZone as _, Utc};
use jc_memory_graph::{
    CognitiveEdge, CognitivePoint, EdgeCandidate, EdgeCreationError, EdgeFilter, EdgeUpdater,
    EvidenceLink, GraphRepositoryError, LearningOutcome, LocalGraphRepository, PointKind,
};
use jc_memory_protocol::{
    EdgeKind, EventId, FragmentId, MemoryScope, NamespaceId, PointId, ProjectId, ValidityStatus,
};

#[test]
fn co_occurrence_is_directional_but_not_verified_causality() {
    let scope = scope_with_project(ProjectId::new());
    let source = point(scope.clone());
    let target = point(scope);
    let evidence = evidence();

    let edge = EdgeCandidate::co_activated(&source, &target, evidence.clone()).unwrap();

    assert_eq!(edge.source_point_id, source.id);
    assert_eq!(edge.target_point_id, target.id);
    assert_eq!(edge.kind, EdgeKind::CoActivated);
    assert_ne!(edge.kind, EdgeKind::Causes);
    assert!(!edge.human_confirmed);
    assert!(!edge.tool_verified);
    assert_eq!(edge.evidence, vec![evidence]);
}

#[test]
fn co_occurrence_confidence_never_exceeds_its_source_points() {
    let scope = scope_with_project(ProjectId::new());
    let mut source = point(scope.clone());
    source.confidence = 0.1;
    let target = point(scope);

    let edge = EdgeCandidate::co_activated(&source, &target, evidence()).unwrap();

    assert_eq!(edge.confidence.to_bits(), source.confidence.to_bits());
}

#[test]
fn edge_creation_rejects_cross_scope_and_self_links() {
    let source = point(scope_with_project(ProjectId::new()));
    let other_scope = point(scope_with_project(ProjectId::new()));
    assert_eq!(
        EdgeCandidate::co_activated(&source, &other_scope, evidence()).unwrap_err(),
        EdgeCreationError::ScopeMismatch
    );
    assert_eq!(
        EdgeCandidate::co_activated(&source, &source, evidence()).unwrap_err(),
        EdgeCreationError::SelfLink(source.id)
    );
}

#[test]
fn repository_rejects_a_point_without_authoritative_fragments() {
    let mut unsupported = point(scope_with_project(ProjectId::new()));
    unsupported.source_fragments.clear();

    assert_eq!(
        LocalGraphRepository::try_from_parts([unsupported.clone()], []).unwrap_err(),
        GraphRepositoryError::MissingPointEvidence(unsupported.id)
    );
}

#[test]
fn online_updates_clip_weight_and_retain_outcome_evidence() {
    let scope = scope_with_project(ProjectId::new());
    let source = point(scope.clone());
    let target = point(scope);
    let mut edge = EdgeCandidate::co_activated(&source, &target, evidence()).unwrap();
    edge.weight = 0.9;

    let success_evidence = evidence();
    let strengthened = EdgeUpdater::apply(
        &mut edge,
        0.4,
        0.5,
        0.1,
        [1.0, 1.0],
        LearningOutcome::Success(success_evidence.clone()),
    )
    .unwrap();
    assert_eq!(strengthened.previous_weight.to_bits(), 0.9_f32.to_bits());
    assert_eq!(strengthened.new_weight.to_bits(), 1.0_f32.to_bits());
    assert_eq!(edge.weight.to_bits(), 1.0_f32.to_bits());
    assert_eq!(edge.evidence.last(), Some(&success_evidence));

    let failure_evidence = evidence();
    let weakened = EdgeUpdater::apply(
        &mut edge,
        0.0,
        0.0,
        2.0,
        [0.0, 0.0],
        LearningOutcome::Failure(failure_evidence.clone()),
    )
    .unwrap();
    assert_eq!(weakened.previous_weight.to_bits(), 1.0_f32.to_bits());
    assert_eq!(weakened.new_weight.to_bits(), 0.0_f32.to_bits());
    assert_eq!(edge.weight.to_bits(), 0.0_f32.to_bits());
    assert_eq!(edge.evidence.last(), Some(&failure_evidence));
}

#[test]
fn local_graph_enforces_scope_filter_neighbor_and_node_budgets() {
    let scope = scope_with_project(ProjectId::new());
    let foreign_scope = scope_with_project(ProjectId::new());
    let seed = point(scope.clone());
    let mut neighbors = [
        point(scope.clone()),
        point(scope.clone()),
        point(scope.clone()),
    ];
    neighbors.sort_by_key(|point| point.id);
    let causal_target = point(scope.clone());
    let foreign = point(foreign_scope.clone());

    let mut edges: Vec<CognitiveEdge> = neighbors
        .iter()
        .enumerate()
        .map(|(index, target)| {
            let mut edge = EdgeCandidate::co_activated(&seed, target, evidence()).unwrap();
            edge.weight = [0.9, 0.8, 0.7][index];
            edge
        })
        .collect();
    edges.push(CognitiveEdge::new(&seed, &causal_target, EdgeKind::Causes, evidence()).unwrap());

    let repository = LocalGraphRepository::try_from_parts(
        std::iter::once(seed.clone())
            .chain(neighbors.iter().cloned())
            .chain([causal_target, foreign.clone()]),
        edges,
    )
    .unwrap();
    let filter = EdgeFilter::only([EdgeKind::CoActivated]);

    let neighbor_limited = repository.load(&scope, &[seed.id, foreign.id], &filter, 2, 10);
    assert_eq!(neighbor_limited.points.len(), 3);
    assert_eq!(neighbor_limited.edges.len(), 2);
    assert!(
        neighbor_limited
            .points
            .iter()
            .all(|point| point.scope == scope)
    );
    assert!(
        neighbor_limited
            .edges
            .iter()
            .all(|edge| edge.kind == EdgeKind::CoActivated)
    );
    assert!(neighbor_limited.truncated);

    let node_limited = repository.load(&scope, &[seed.id], &filter, 10, 2);
    assert_eq!(node_limited.points.len(), 2);
    assert_eq!(node_limited.edges.len(), 1);
    assert!(node_limited.truncated);
}

fn evidence() -> EvidenceLink {
    EvidenceLink {
        fragment_id: FragmentId::new(),
        event_id: Some(EventId::new()),
        evidence_ref_id: None,
    }
}

fn point(scope: MemoryScope) -> CognitivePoint {
    CognitivePoint {
        id: PointId::new(),
        scope,
        kind: PointKind::Failure,
        source_fragments: vec![FragmentId::new()],
        occurred_at: Utc.with_ymd_and_hms(2026, 7, 29, 12, 0, 0).unwrap(),
        confidence: 0.8,
        validity: ValidityStatus::Unverified,
    }
}

fn scope_with_project(project_id: ProjectId) -> MemoryScope {
    let mut scope = MemoryScope::new(NamespaceId::new());
    scope.project_id = Some(project_id);
    scope
}
