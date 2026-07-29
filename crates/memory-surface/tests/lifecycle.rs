//! Cognitive Surface lifecycle behavior.

use chrono::{TimeZone as _, Utc};
use jc_memory_graph::{
    CognitiveEdge, CognitivePoint, EvidenceLink, GraphRepositoryError, PointKind,
};
use jc_memory_protocol::{
    EdgeKind, EventId, FragmentId, MemoryScope, NamespaceId, SurfaceKind, ValidityStatus,
};
use jc_memory_surface::{
    AddMember, CreateSurface, DeactivateSurface, FreezeSurface, MergeSurfaces, RemoveMember,
    SplitSurface, SurfaceBoundary, SurfaceEvidence, SurfacePartition, SurfacePoint,
    SurfaceRepository, SurfaceRepositoryError,
};

#[tokio::test]
async fn point_can_join_multiple_surfaces() {
    let scope = MemoryScope::new(NamespaceId::new());
    let point = point(scope.clone(), vec![1.0, 0.0]);
    let repository = SurfaceRepository::try_from_parts(scope.clone(), [point.clone()], []).unwrap();
    let explicit = repository
        .create_surface(create_surface(&scope, SurfaceKind::Explicit))
        .await
        .unwrap();
    let temporary = repository
        .create_surface(create_surface(&scope, SurfaceKind::TemporaryTask))
        .await
        .unwrap();

    repository
        .add_member(AddMember {
            surface_id: explicit.id,
            point_id: point.point.id,
            member_weight: 0.8,
            automatic: false,
            changed_at: at(13),
            evidence: evidence(point.point.scope.clone()),
        })
        .await
        .unwrap();
    repository
        .add_member(AddMember {
            surface_id: temporary.id,
            point_id: point.point.id,
            member_weight: 0.6,
            automatic: true,
            changed_at: at(14),
            evidence: evidence(point.point.scope.clone()),
        })
        .await
        .unwrap();

    let memberships = repository.memberships(point.point.id).await;
    assert_eq!(memberships.len(), 2);
    assert!(memberships.iter().any(|membership| {
        membership.surface_id == explicit.id
            && membership.member_weight.to_bits() == 0.8_f32.to_bits()
    }));
    assert!(memberships.iter().any(|membership| {
        membership.surface_id == temporary.id
            && membership.member_weight.to_bits() == 0.6_f32.to_bits()
    }));
}

#[tokio::test]
async fn add_and_remove_create_immutable_member_versions() {
    let scope = MemoryScope::new(NamespaceId::new());
    let point = point(scope.clone(), vec![0.2, 0.8]);
    let repository = SurfaceRepository::try_from_parts(scope.clone(), [point.clone()], []).unwrap();
    let surface = repository
        .create_surface(create_surface(&scope, SurfaceKind::Explicit))
        .await
        .unwrap();

    repository
        .add_member(AddMember {
            surface_id: surface.id,
            point_id: point.point.id,
            member_weight: 0.75,
            automatic: false,
            changed_at: at(13),
            evidence: evidence(scope.clone()),
        })
        .await
        .unwrap();
    repository
        .remove_member(RemoveMember {
            surface_id: surface.id,
            point_id: point.point.id,
            automatic: false,
            changed_at: at(14),
            evidence: evidence(scope),
        })
        .await
        .unwrap();

    let surface = repository.get(surface.id).await.unwrap();
    assert_eq!(surface.versions.len(), 3);
    assert_eq!(surface.versions[0].version, 1);
    assert!(surface.versions[0].member_points.is_empty());
    assert_eq!(surface.versions[0].valid_to, Some(at(13)));
    assert_eq!(surface.versions[1].version, 2);
    assert_eq!(
        surface.versions[1]
            .member_points
            .get(&point.point.id)
            .map(|weight| weight.to_bits()),
        Some(0.75_f32.to_bits())
    );
    assert_eq!(surface.versions[1].valid_to, Some(at(14)));
    assert_eq!(surface.versions[2].version, 3);
    assert!(surface.versions[2].member_points.is_empty());
    assert_eq!(surface.versions[2].valid_to, None);
}

#[tokio::test]
async fn frozen_surface_rejects_automatic_but_accepts_explicit_membership() {
    let scope = MemoryScope::new(NamespaceId::new());
    let first = point(scope.clone(), vec![1.0, 0.0]);
    let second = point(scope.clone(), vec![0.0, 1.0]);
    let repository =
        SurfaceRepository::try_from_parts(scope.clone(), [first.clone(), second.clone()], [])
            .unwrap();
    let surface = repository
        .create_surface(create_surface(&scope, SurfaceKind::Dynamic))
        .await
        .unwrap();
    repository
        .freeze(FreezeSurface {
            surface_id: surface.id,
            changed_at: at(13),
            evidence: evidence(scope.clone()),
        })
        .await
        .unwrap();

    let automatic = repository
        .add_member(AddMember {
            surface_id: surface.id,
            point_id: first.point.id,
            member_weight: 0.8,
            automatic: true,
            changed_at: at(14),
            evidence: evidence(scope.clone()),
        })
        .await;
    assert_eq!(
        automatic.unwrap_err(),
        SurfaceRepositoryError::SurfaceFrozen
    );

    repository
        .add_member(AddMember {
            surface_id: surface.id,
            point_id: second.point.id,
            member_weight: 0.7,
            automatic: false,
            changed_at: at(14),
            evidence: evidence(scope),
        })
        .await
        .unwrap();
    let surface = repository.get(surface.id).await.unwrap();
    assert_eq!(surface.versions.len(), 3);
    assert_eq!(
        surface.current().status,
        jc_memory_protocol::SurfaceStatus::Frozen
    );
    assert!(
        surface
            .current()
            .member_points
            .contains_key(&second.point.id)
    );
}

#[tokio::test]
async fn inactive_surface_rejects_all_membership_changes() {
    let scope = MemoryScope::new(NamespaceId::new());
    let point = point(scope.clone(), vec![1.0, 0.0]);
    let repository = SurfaceRepository::try_from_parts(scope.clone(), [point.clone()], []).unwrap();
    let surface = repository
        .create_surface(create_surface(&scope, SurfaceKind::Explicit))
        .await
        .unwrap();
    repository
        .deactivate(DeactivateSurface {
            surface_id: surface.id,
            changed_at: at(13),
            evidence: evidence(scope.clone()),
        })
        .await
        .unwrap();

    let result = repository
        .add_member(AddMember {
            surface_id: surface.id,
            point_id: point.point.id,
            member_weight: 0.8,
            automatic: false,
            changed_at: at(14),
            evidence: evidence(scope),
        })
        .await;
    assert_eq!(result.unwrap_err(), SurfaceRepositoryError::SurfaceInactive);
    let surface = repository.get(surface.id).await.unwrap();
    assert_eq!(surface.versions.len(), 2);
    assert_eq!(
        surface.current().status,
        jc_memory_protocol::SurfaceStatus::Inactive
    );
}

#[tokio::test]
async fn merge_creates_a_successor_without_rewriting_source_history() {
    let scope = MemoryScope::new(NamespaceId::new());
    let first = point(scope.clone(), vec![1.0, 0.0]);
    let second = point(scope.clone(), vec![0.0, 1.0]);
    let repository =
        SurfaceRepository::try_from_parts(scope.clone(), [first.clone(), second.clone()], [])
            .unwrap();
    let left = repository
        .create_surface(create_surface(&scope, SurfaceKind::Explicit))
        .await
        .unwrap();
    let right = repository
        .create_surface(create_surface(&scope, SurfaceKind::Dynamic))
        .await
        .unwrap();
    add(&repository, &scope, left.id, first.point.id, 0.8, 13).await;
    add(&repository, &scope, right.id, second.point.id, 0.6, 13).await;
    let left_before = repository.get(left.id).await.unwrap();
    let right_before = repository.get(right.id).await.unwrap();

    let merged = repository
        .merge(MergeSurfaces {
            source_surface_ids: vec![left.id, right.id],
            kind: SurfaceKind::Dynamic,
            activation_threshold: 0.55,
            confidence: 0.75,
            boundary: SurfaceBoundary {
                min_member_weight: 0.1,
                max_members: 16,
            },
            changed_at: at(14),
            evidence: evidence(scope.clone()),
            model_version_id: Some("fixture-model-v2".to_owned()),
        })
        .await
        .unwrap();

    let left_after = repository.get(left.id).await.unwrap();
    let right_after = repository.get(right.id).await.unwrap();
    let mut expected_left_history = left_before.versions;
    expected_left_history.last_mut().unwrap().valid_to = Some(at(14));
    let mut expected_right_history = right_before.versions;
    expected_right_history.last_mut().unwrap().valid_to = Some(at(14));
    assert_eq!(left_after.versions[..2], expected_left_history);
    assert_eq!(right_after.versions[..2], expected_right_history);
    assert_eq!(
        left_after.current().status,
        jc_memory_protocol::SurfaceStatus::Merged
    );
    assert_eq!(
        right_after.current().status,
        jc_memory_protocol::SurfaceStatus::Merged
    );
    assert_ne!(merged.id, left.id);
    assert_ne!(merged.id, right.id);
    assert_eq!(merged.current().version, 1);
    assert_eq!(merged.current().member_points.len(), 2);
    assert!(merged.current().member_points.contains_key(&first.point.id));
    assert!(
        merged
            .current()
            .member_points
            .contains_key(&second.point.id)
    );
}

#[tokio::test]
async fn split_creates_disjoint_successors_without_rewriting_source_history() {
    let scope = MemoryScope::new(NamespaceId::new());
    let first = point(scope.clone(), vec![1.0, 0.0]);
    let second = point(scope.clone(), vec![0.0, 1.0]);
    let repository =
        SurfaceRepository::try_from_parts(scope.clone(), [first.clone(), second.clone()], [])
            .unwrap();
    let source = repository
        .create_surface(create_surface(&scope, SurfaceKind::Dynamic))
        .await
        .unwrap();
    add(&repository, &scope, source.id, first.point.id, 0.8, 13).await;
    add(&repository, &scope, source.id, second.point.id, 0.6, 14).await;
    let source_before = repository.get(source.id).await.unwrap();

    let successors = repository
        .split(SplitSurface {
            source_surface_id: source.id,
            partitions: vec![
                SurfacePartition {
                    member_point_ids: vec![first.point.id],
                },
                SurfacePartition {
                    member_point_ids: vec![second.point.id],
                },
            ],
            changed_at: at(15),
            evidence: evidence(scope),
        })
        .await
        .unwrap();

    let source_after = repository.get(source.id).await.unwrap();
    let mut expected_source_history = source_before.versions;
    expected_source_history.last_mut().unwrap().valid_to = Some(at(15));
    assert_eq!(source_after.versions[..3], expected_source_history);
    assert_eq!(
        source_after.current().status,
        jc_memory_protocol::SurfaceStatus::Split
    );
    assert_eq!(successors.len(), 2);
    assert_ne!(successors[0].id, successors[1].id);
    assert_eq!(
        successors[0]
            .current()
            .member_points
            .keys()
            .copied()
            .collect::<Vec<_>>(),
        vec![first.point.id]
    );
    assert_eq!(
        successors[1]
            .current()
            .member_points
            .keys()
            .copied()
            .collect::<Vec<_>>(),
        vec![second.point.id]
    );
}

#[tokio::test]
async fn semantic_center_tracks_incremental_member_weights() {
    let scope = MemoryScope::new(NamespaceId::new());
    let first = point(scope.clone(), vec![1.0, 0.0]);
    let second = point(scope.clone(), vec![0.0, 1.0]);
    let repository =
        SurfaceRepository::try_from_parts(scope.clone(), [first.clone(), second.clone()], [])
            .unwrap();
    let surface = repository
        .create_surface(create_surface(&scope, SurfaceKind::Dynamic))
        .await
        .unwrap();

    add(&repository, &scope, surface.id, first.point.id, 0.75, 13).await;
    add(&repository, &scope, surface.id, second.point.id, 0.25, 14).await;
    let after_add = repository.get(surface.id).await.unwrap();
    assert_vector_close(&after_add.current().semantic_center, &[0.75, 0.25]);

    repository
        .remove_member(RemoveMember {
            surface_id: surface.id,
            point_id: first.point.id,
            automatic: false,
            changed_at: at(15),
            evidence: evidence(scope),
        })
        .await
        .unwrap();
    let after_remove = repository.get(surface.id).await.unwrap();
    assert_vector_close(&after_remove.current().semantic_center, &[0.0, 1.0]);
    assert_vector_close(&after_remove.versions[2].semantic_center, &[0.75, 0.25]);
}

#[tokio::test]
async fn contradicting_member_edge_is_reported_as_a_surface_conflict() {
    let scope = MemoryScope::new(NamespaceId::new());
    let first = point(scope.clone(), vec![1.0, 0.0]);
    let second = point(scope.clone(), vec![0.0, 1.0]);
    let contradiction = CognitiveEdge::new(
        &first.point,
        &second.point,
        EdgeKind::Contradicts,
        evidence(scope.clone()).link,
    )
    .unwrap();
    let repository = SurfaceRepository::try_from_parts(
        scope.clone(),
        [first.clone(), second.clone()],
        [contradiction.clone()],
    )
    .unwrap();
    let surface = repository
        .create_surface(create_surface(&scope, SurfaceKind::Explicit))
        .await
        .unwrap();
    add(&repository, &scope, surface.id, first.point.id, 0.8, 13).await;
    add(&repository, &scope, surface.id, second.point.id, 0.7, 14).await;

    let current = repository.get(surface.id).await.unwrap();
    assert_eq!(current.current().member_edges, vec![contradiction.id]);
    let conflicts = repository.conflicts(surface.id).await.unwrap();
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].edge_id, contradiction.id);
    assert_eq!(conflicts[0].source_point_id, first.point.id);
    assert_eq!(conflicts[0].target_point_id, second.point.id);
}

#[tokio::test]
async fn rejected_vector_dimension_does_not_append_a_partial_version() {
    let scope = MemoryScope::new(NamespaceId::new());
    let first = point(scope.clone(), vec![1.0, 0.0]);
    let incompatible = point(scope.clone(), vec![0.2, 0.3, 0.5]);
    let repository =
        SurfaceRepository::try_from_parts(scope.clone(), [first.clone(), incompatible.clone()], [])
            .unwrap();
    let surface = repository
        .create_surface(create_surface(&scope, SurfaceKind::Dynamic))
        .await
        .unwrap();
    add(&repository, &scope, surface.id, first.point.id, 0.8, 13).await;

    let result = repository
        .add_member(AddMember {
            surface_id: surface.id,
            point_id: incompatible.point.id,
            member_weight: 0.7,
            automatic: false,
            changed_at: at(14),
            evidence: evidence(scope),
        })
        .await;

    assert_eq!(
        result.unwrap_err(),
        SurfaceRepositoryError::SemanticDimensionMismatch(incompatible.point.id)
    );
    let unchanged = repository.get(surface.id).await.unwrap();
    assert_eq!(unchanged.versions.len(), 2);
    assert_eq!(unchanged.current().valid_to, None);
    assert_eq!(
        unchanged
            .current()
            .member_points
            .keys()
            .copied()
            .collect::<Vec<_>>(),
        vec![first.point.id]
    );
}

#[tokio::test]
async fn conflict_detection_ignores_edges_outside_the_surface_version_time() {
    let scope = MemoryScope::new(NamespaceId::new());
    let first = point(scope.clone(), vec![1.0, 0.0]);
    let second = point(scope.clone(), vec![0.0, 1.0]);
    let mut future_contradiction = CognitiveEdge::new(
        &first.point,
        &second.point,
        EdgeKind::Contradicts,
        evidence(scope.clone()).link,
    )
    .unwrap();
    future_contradiction.valid_from = at(20);
    let repository = SurfaceRepository::try_from_parts(
        scope.clone(),
        [first.clone(), second.clone()],
        [future_contradiction],
    )
    .unwrap();
    let surface = repository
        .create_surface(create_surface(&scope, SurfaceKind::Explicit))
        .await
        .unwrap();
    add(&repository, &scope, surface.id, first.point.id, 0.8, 13).await;
    add(&repository, &scope, surface.id, second.point.id, 0.7, 14).await;

    let current = repository.get(surface.id).await.unwrap();
    assert!(current.current().member_edges.is_empty());
    assert!(repository.conflicts(surface.id).await.unwrap().is_empty());
}

#[tokio::test]
async fn repository_rejects_cross_scope_points_edges_and_surface_evidence() {
    let scope = MemoryScope::new(NamespaceId::new());
    let foreign_scope = MemoryScope::new(NamespaceId::new());
    let foreign_point = point(foreign_scope, vec![1.0, 0.0]);
    assert_eq!(
        SurfaceRepository::try_from_parts(scope.clone(), [foreign_point], []).unwrap_err(),
        SurfaceRepositoryError::ScopeMismatch
    );

    let first = point(scope.clone(), vec![1.0, 0.0]);
    let second = point(scope.clone(), vec![0.0, 1.0]);
    let mut foreign_edge = CognitiveEdge::new(
        &first.point,
        &second.point,
        EdgeKind::Supports,
        evidence(scope.clone()).link,
    )
    .unwrap();
    foreign_edge.scope = MemoryScope::new(NamespaceId::new());
    assert_eq!(
        SurfaceRepository::try_from_parts(
            scope.clone(),
            [first.clone(), second.clone()],
            [foreign_edge]
        )
        .unwrap_err(),
        SurfaceRepositoryError::Graph(GraphRepositoryError::ScopeMismatch)
    );

    let repository = SurfaceRepository::try_from_parts(scope.clone(), [first, second], []).unwrap();
    let mut command = create_surface(&scope, SurfaceKind::Explicit);
    command.evidence = evidence(MemoryScope::new(NamespaceId::new()));
    assert_eq!(
        repository.create_surface(command).await.unwrap_err(),
        SurfaceRepositoryError::ScopeMismatch
    );
}

#[tokio::test]
async fn rejected_merge_boundary_leaves_all_sources_unchanged() {
    let scope = MemoryScope::new(NamespaceId::new());
    let first = point(scope.clone(), vec![1.0, 0.0]);
    let second = point(scope.clone(), vec![0.0, 1.0]);
    let repository =
        SurfaceRepository::try_from_parts(scope.clone(), [first.clone(), second.clone()], [])
            .unwrap();
    let left = repository
        .create_surface(create_surface(&scope, SurfaceKind::Dynamic))
        .await
        .unwrap();
    let right = repository
        .create_surface(create_surface(&scope, SurfaceKind::Dynamic))
        .await
        .unwrap();
    add(&repository, &scope, left.id, first.point.id, 0.2, 13).await;
    add(&repository, &scope, right.id, second.point.id, 0.3, 13).await;
    let left_before = repository.get(left.id).await.unwrap();
    let right_before = repository.get(right.id).await.unwrap();

    let result = repository
        .merge(MergeSurfaces {
            source_surface_ids: vec![left.id, right.id],
            kind: SurfaceKind::Dynamic,
            activation_threshold: 0.5,
            confidence: 0.8,
            boundary: SurfaceBoundary {
                min_member_weight: 0.5,
                max_members: 16,
            },
            changed_at: at(14),
            evidence: evidence(scope),
            model_version_id: None,
        })
        .await;

    assert_eq!(
        result.unwrap_err(),
        SurfaceRepositoryError::MemberBelowBoundary
    );
    assert_eq!(repository.get(left.id).await.unwrap(), left_before);
    assert_eq!(repository.get(right.id).await.unwrap(), right_before);
}

fn create_surface(scope: &MemoryScope, kind: SurfaceKind) -> CreateSurface {
    CreateSurface {
        kind,
        activation_threshold: 0.5,
        confidence: 0.8,
        boundary: SurfaceBoundary {
            min_member_weight: 0.1,
            max_members: 16,
        },
        evidence: evidence(scope.clone()),
        model_version_id: Some("fixture-model-v1".to_owned()),
        created_at: at(12),
    }
}

fn point(scope: MemoryScope, semantic_vector: Vec<f32>) -> SurfacePoint {
    SurfacePoint {
        point: CognitivePoint {
            id: jc_memory_protocol::PointId::new(),
            scope,
            kind: PointKind::Fact,
            source_fragments: vec![FragmentId::new()],
            occurred_at: at(10),
            confidence: 0.8,
            validity: ValidityStatus::Verified,
        },
        semantic_vector,
    }
}

fn evidence(scope: MemoryScope) -> SurfaceEvidence {
    SurfaceEvidence {
        scope,
        link: EvidenceLink {
            fragment_id: FragmentId::new(),
            event_id: Some(EventId::new()),
            evidence_ref_id: None,
        },
    }
}

async fn add(
    repository: &SurfaceRepository,
    scope: &MemoryScope,
    surface_id: jc_memory_protocol::SurfaceId,
    point_id: jc_memory_protocol::PointId,
    member_weight: f32,
    hour: u32,
) {
    repository
        .add_member(AddMember {
            surface_id,
            point_id,
            member_weight,
            automatic: false,
            changed_at: at(hour),
            evidence: evidence(scope.clone()),
        })
        .await
        .unwrap();
}

fn at(hour: u32) -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 7, 29, hour, 0, 0).unwrap()
}

fn assert_vector_close(actual: &[f32], expected: &[f32]) {
    assert_eq!(actual.len(), expected.len());
    for (actual, expected) in actual.iter().zip(expected) {
        assert!((actual - expected).abs() < 1.0e-6, "{actual} != {expected}");
    }
}
