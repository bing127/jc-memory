//! Scope-bound storage and mutation of reconstructable cognitive surfaces.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use jc_memory_graph::{CognitiveEdge, GraphRepositoryError, LocalGraphRepository};
use jc_memory_protocol::{EdgeKind, MemoryScope, PointId, SurfaceId, SurfaceStatus};
use thiserror::Error;
use tokio::sync::Mutex;

use crate::{
    AddMember, CognitiveSurface, CreateSurface, DeactivateSurface, FreezeSurface, MergeSurfaces,
    RemoveMember, SplitSurface, SurfaceConflict, SurfaceMembership, SurfacePoint, SurfaceVersion,
};

/// A rejected surface repository operation.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum SurfaceRepositoryError {
    /// A point or evidence record belongs to another exact scope.
    #[error("surface data must belong to the repository scope")]
    ScopeMismatch,
    /// A point has no authoritative source Fragment.
    #[error("point {0} must reference at least one authoritative fragment")]
    MissingPointEvidence(PointId),
    /// A point vector is empty or contains a non-finite component.
    #[error("point {0} must have a non-empty finite semantic vector")]
    InvalidSemanticVector(PointId),
    /// A member vector has a different dimension from the Surface center.
    #[error("point {0} semantic vector dimension does not match the surface")]
    SemanticDimensionMismatch(PointId),
    /// A surface does not exist.
    #[error("surface {0} does not exist")]
    MissingSurface(SurfaceId),
    /// A point does not exist.
    #[error("point {0} does not exist")]
    MissingPoint(PointId),
    /// A unit-interval field is invalid.
    #[error("{field} must be finite and between 0.0 and 1.0")]
    InvalidUnitInterval {
        /// Invalid field name.
        field: &'static str,
    },
    /// The configured surface member capacity is zero.
    #[error("surface max_members must be greater than zero")]
    EmptyBoundary,
    /// Adding a new point would exceed the surface boundary.
    #[error("surface member capacity has been reached")]
    MemberCapacityReached,
    /// The member weight is below the surface boundary.
    #[error("member weight is below the surface boundary")]
    MemberBelowBoundary,
    /// The requested member is not in the current Surface version.
    #[error("point {point_id} is not a member of surface {surface_id}")]
    MissingMember {
        /// Surface that was queried.
        surface_id: SurfaceId,
        /// Point that was queried.
        point_id: PointId,
    },
    /// A mutation predates the current Surface version.
    #[error("surface mutation cannot predate the current version")]
    OutOfOrder,
    /// Frozen Surfaces reject automatically proposed membership changes.
    #[error("frozen surface rejects automatic membership changes")]
    SurfaceFrozen,
    /// Inactive or terminal Surfaces reject membership changes.
    #[error("inactive surface rejects membership changes")]
    SurfaceInactive,
    /// The requested lifecycle transition is not valid from the current status.
    #[error("cannot change surface status from {from:?} to {to:?}")]
    InvalidLifecycleTransition {
        /// Current status.
        from: SurfaceStatus,
        /// Requested status.
        to: SurfaceStatus,
    },
    /// Merge requires at least two unique source Surfaces.
    #[error("surface merge requires at least two unique sources")]
    InvalidMergeSources,
    /// Split requires at least two non-empty successor partitions.
    #[error("surface split requires at least two non-empty partitions")]
    InvalidSplitPartitions,
    /// Split partitions must assign every current member exactly once.
    #[error("surface split partitions must contain every current member exactly once")]
    InvalidSplitMembership,
    /// The supplied graph parts violate graph ownership or evidence invariants.
    #[error(transparent)]
    Graph(#[from] GraphRepositoryError),
}

#[derive(Debug, Default)]
struct SurfaceStore {
    surfaces: BTreeMap<SurfaceId, CognitiveSurface>,
}

/// In-process, scope-bound repository for reconstructable Surface state.
#[derive(Clone, Debug)]
pub struct SurfaceRepository {
    scope: MemoryScope,
    points: BTreeMap<PointId, SurfacePoint>,
    edges: BTreeMap<jc_memory_protocol::EdgeId, CognitiveEdge>,
    store: Arc<Mutex<SurfaceStore>>,
}

impl SurfaceRepository {
    /// Builds a repository after validating Point ownership and semantic vectors.
    ///
    /// # Errors
    ///
    /// Returns [`SurfaceRepositoryError`] when a Point is outside the repository
    /// scope, lacks authoritative evidence, or has an invalid vector.
    pub fn try_from_parts(
        scope: MemoryScope,
        points: impl IntoIterator<Item = SurfacePoint>,
        edges: impl IntoIterator<Item = CognitiveEdge>,
    ) -> Result<Self, SurfaceRepositoryError> {
        let points: Vec<_> = points.into_iter().collect();
        let edges: Vec<_> = edges.into_iter().collect();
        let mut point_map = BTreeMap::new();
        for point in &points {
            if point.point.scope != scope {
                return Err(SurfaceRepositoryError::ScopeMismatch);
            }
            if point.point.source_fragments.is_empty() {
                return Err(SurfaceRepositoryError::MissingPointEvidence(point.point.id));
            }
            if point.semantic_vector.is_empty()
                || point.semantic_vector.iter().any(|value| !value.is_finite())
            {
                return Err(SurfaceRepositoryError::InvalidSemanticVector(
                    point.point.id,
                ));
            }
            point_map.insert(point.point.id, point.clone());
        }
        LocalGraphRepository::try_from_parts(
            points.iter().map(|point| point.point.clone()),
            edges.iter().cloned(),
        )?;
        let edge_map = edges.into_iter().map(|edge| (edge.id, edge)).collect();
        Ok(Self {
            scope,
            points: point_map,
            edges: edge_map,
            store: Arc::new(Mutex::new(SurfaceStore::default())),
        })
    }

    /// Creates a Surface with an empty initial membership.
    ///
    /// # Errors
    ///
    /// Returns [`SurfaceRepositoryError`] for cross-scope evidence or invalid limits.
    pub async fn create_surface(
        &self,
        command: CreateSurface,
    ) -> Result<CognitiveSurface, SurfaceRepositoryError> {
        self.validate_evidence_scope(&command.evidence.scope)?;
        validate_unit_interval("activation_threshold", command.activation_threshold)?;
        validate_unit_interval("confidence", command.confidence)?;
        validate_unit_interval(
            "boundary.min_member_weight",
            command.boundary.min_member_weight,
        )?;
        if command.boundary.max_members == 0 {
            return Err(SurfaceRepositoryError::EmptyBoundary);
        }

        let surface = CognitiveSurface {
            id: SurfaceId::new(),
            scope: self.scope.clone(),
            kind: command.kind,
            versions: vec![SurfaceVersion {
                version: 1,
                semantic_center: Vec::new(),
                member_points: BTreeMap::new(),
                member_edges: Vec::new(),
                activation_threshold: command.activation_threshold,
                valid_from: command.created_at,
                valid_to: None,
                status: SurfaceStatus::Active,
                confidence: command.confidence,
                boundary: command.boundary,
                evidence: vec![command.evidence.link],
                model_version_id: command.model_version_id,
            }],
        };
        self.store
            .lock()
            .await
            .surfaces
            .insert(surface.id, surface.clone());
        Ok(surface)
    }

    /// Adds or updates one point membership in a Surface.
    ///
    /// # Errors
    ///
    /// Returns [`SurfaceRepositoryError`] when the Surface or Point is missing,
    /// evidence crosses scope, or the membership violates a boundary.
    pub async fn add_member(&self, command: AddMember) -> Result<(), SurfaceRepositoryError> {
        self.validate_evidence_scope(&command.evidence.scope)?;
        validate_unit_interval("member_weight", command.member_weight)?;
        let point = self
            .points
            .get(&command.point_id)
            .ok_or(SurfaceRepositoryError::MissingPoint(command.point_id))?;

        let mut store = self.store.lock().await;
        let surface = store
            .surfaces
            .get_mut(&command.surface_id)
            .ok_or(SurfaceRepositoryError::MissingSurface(command.surface_id))?;
        let current = surface.current();
        validate_membership_status(current.status, command.automatic)?;
        validate_center_dimension(current, point)?;
        if command.member_weight < current.boundary.min_member_weight {
            return Err(SurfaceRepositoryError::MemberBelowBoundary);
        }
        if !current.member_points.contains_key(&command.point_id)
            && current.member_points.len() == current.boundary.max_members
        {
            return Err(SurfaceRepositoryError::MemberCapacityReached);
        }
        let version = begin_next_version(surface, command.changed_at, command.evidence.link)?;
        add_to_center(version, point, command.member_weight)?;
        version.member_edges = self.member_edges_for(&version.member_points, version.valid_from);
        Ok(())
    }

    /// Removes one Point from a Surface while preserving the previous snapshot.
    ///
    /// # Errors
    ///
    /// Returns [`SurfaceRepositoryError`] when ownership is invalid, the
    /// Surface is missing, the Point is not a member, or time moves backwards.
    pub async fn remove_member(&self, command: RemoveMember) -> Result<(), SurfaceRepositoryError> {
        self.validate_evidence_scope(&command.evidence.scope)?;
        let point = self
            .points
            .get(&command.point_id)
            .ok_or(SurfaceRepositoryError::MissingPoint(command.point_id))?;
        let mut store = self.store.lock().await;
        let surface = store
            .surfaces
            .get_mut(&command.surface_id)
            .ok_or(SurfaceRepositoryError::MissingSurface(command.surface_id))?;
        validate_membership_status(surface.current().status, command.automatic)?;
        validate_center_dimension(surface.current(), point)?;
        if !surface
            .current()
            .member_points
            .contains_key(&command.point_id)
        {
            return Err(SurfaceRepositoryError::MissingMember {
                surface_id: command.surface_id,
                point_id: command.point_id,
            });
        }
        let version = begin_next_version(surface, command.changed_at, command.evidence.link)?;
        remove_from_center(version, point)?;
        version.member_edges = self.member_edges_for(&version.member_points, version.valid_from);
        Ok(())
    }

    /// Freezes an active Surface while preserving its active version.
    ///
    /// # Errors
    ///
    /// Returns [`SurfaceRepositoryError`] for cross-scope evidence, a missing
    /// Surface, invalid time, or a non-active current status.
    pub async fn freeze(&self, command: FreezeSurface) -> Result<(), SurfaceRepositoryError> {
        self.change_status(
            command.surface_id,
            SurfaceStatus::Frozen,
            command.changed_at,
            command.evidence,
            &[SurfaceStatus::Active],
        )
        .await
    }

    /// Deactivates an active or frozen Surface while preserving prior versions.
    ///
    /// # Errors
    ///
    /// Returns [`SurfaceRepositoryError`] for cross-scope evidence, a missing
    /// Surface, invalid time, or a terminal current status.
    pub async fn deactivate(
        &self,
        command: DeactivateSurface,
    ) -> Result<(), SurfaceRepositoryError> {
        self.change_status(
            command.surface_id,
            SurfaceStatus::Inactive,
            command.changed_at,
            command.evidence,
            &[SurfaceStatus::Active, SurfaceStatus::Frozen],
        )
        .await
    }

    /// Merges source Surfaces into a new successor without rewriting old versions.
    ///
    /// Duplicate member points retain their strongest current membership weight.
    ///
    /// # Errors
    ///
    /// Returns [`SurfaceRepositoryError`] for invalid ownership, limits, source
    /// count, source status, or effective time.
    pub async fn merge(
        &self,
        command: MergeSurfaces,
    ) -> Result<CognitiveSurface, SurfaceRepositoryError> {
        self.validate_evidence_scope(&command.evidence.scope)?;
        validate_unit_interval("activation_threshold", command.activation_threshold)?;
        validate_unit_interval("confidence", command.confidence)?;
        validate_unit_interval(
            "boundary.min_member_weight",
            command.boundary.min_member_weight,
        )?;
        if command.boundary.max_members == 0 {
            return Err(SurfaceRepositoryError::EmptyBoundary);
        }
        let unique_sources: BTreeSet<_> = command.source_surface_ids.iter().copied().collect();
        if unique_sources.len() < 2 || unique_sources.len() != command.source_surface_ids.len() {
            return Err(SurfaceRepositoryError::InvalidMergeSources);
        }

        let mut store = self.store.lock().await;
        let sources: Vec<_> = command
            .source_surface_ids
            .iter()
            .map(|surface_id| {
                store
                    .surfaces
                    .get(surface_id)
                    .cloned()
                    .ok_or(SurfaceRepositoryError::MissingSurface(*surface_id))
            })
            .collect::<Result<_, _>>()?;
        for source in &sources {
            validate_terminal_transition(source.current().status, SurfaceStatus::Merged)?;
            if command.changed_at < source.current().valid_from {
                return Err(SurfaceRepositoryError::OutOfOrder);
            }
        }

        let mut member_points = BTreeMap::new();
        let mut source_evidence = Vec::new();
        for source in &sources {
            for (point_id, weight) in &source.current().member_points {
                let existing = member_points.entry(*point_id).or_insert(*weight);
                *existing = f32::max(*existing, *weight);
            }
            source_evidence.extend(source.current().evidence.iter().cloned());
        }
        if member_points.len() > command.boundary.max_members {
            return Err(SurfaceRepositoryError::MemberCapacityReached);
        }
        if member_points
            .values()
            .any(|weight| *weight < command.boundary.min_member_weight)
        {
            return Err(SurfaceRepositoryError::MemberBelowBoundary);
        }
        let semantic_center = self.center_for_members(&member_points)?;
        let member_edges = self.member_edges_for(&member_points, command.changed_at);
        source_evidence.push(command.evidence.link.clone());

        for source_id in &command.source_surface_ids {
            let source = store
                .surfaces
                .get_mut(source_id)
                .ok_or(SurfaceRepositoryError::MissingSurface(*source_id))?;
            begin_next_version(source, command.changed_at, command.evidence.link.clone())?.status =
                SurfaceStatus::Merged;
        }

        let successor = CognitiveSurface {
            id: SurfaceId::new(),
            scope: self.scope.clone(),
            kind: command.kind,
            versions: vec![SurfaceVersion {
                version: 1,
                semantic_center,
                member_points,
                member_edges,
                activation_threshold: command.activation_threshold,
                valid_from: command.changed_at,
                valid_to: None,
                status: SurfaceStatus::Active,
                confidence: command.confidence,
                boundary: command.boundary,
                evidence: source_evidence,
                model_version_id: command.model_version_id,
            }],
        };
        store.surfaces.insert(successor.id, successor.clone());
        Ok(successor)
    }

    /// Splits a Surface into disjoint successors without rewriting source history.
    ///
    /// # Errors
    ///
    /// Returns [`SurfaceRepositoryError`] for invalid ownership, source status,
    /// effective time, or incomplete/overlapping partitions.
    pub async fn split(
        &self,
        command: SplitSurface,
    ) -> Result<Vec<CognitiveSurface>, SurfaceRepositoryError> {
        self.validate_evidence_scope(&command.evidence.scope)?;
        if command.partitions.len() < 2
            || command
                .partitions
                .iter()
                .any(|partition| partition.member_point_ids.is_empty())
        {
            return Err(SurfaceRepositoryError::InvalidSplitPartitions);
        }

        let mut store = self.store.lock().await;
        let source = store
            .surfaces
            .get(&command.source_surface_id)
            .cloned()
            .ok_or(SurfaceRepositoryError::MissingSurface(
                command.source_surface_id,
            ))?;
        validate_terminal_transition(source.current().status, SurfaceStatus::Split)?;
        if command.changed_at < source.current().valid_from {
            return Err(SurfaceRepositoryError::OutOfOrder);
        }
        let mut partition_members = BTreeSet::new();
        for point_id in command
            .partitions
            .iter()
            .flat_map(|partition| &partition.member_point_ids)
        {
            if !partition_members.insert(*point_id) {
                return Err(SurfaceRepositoryError::InvalidSplitMembership);
            }
        }
        let source_members: BTreeSet<_> = source.current().member_points.keys().copied().collect();
        if partition_members != source_members {
            return Err(SurfaceRepositoryError::InvalidSplitMembership);
        }

        let mut successors = Vec::with_capacity(command.partitions.len());
        for partition in &command.partitions {
            let member_points: BTreeMap<_, _> = partition
                .member_point_ids
                .iter()
                .map(|point_id| (*point_id, source.current().member_points[point_id]))
                .collect();
            let semantic_center = self.center_for_members(&member_points)?;
            let member_edges = self.member_edges_for(&member_points, command.changed_at);
            let mut evidence = source.current().evidence.clone();
            evidence.push(command.evidence.link.clone());
            successors.push(CognitiveSurface {
                id: SurfaceId::new(),
                scope: self.scope.clone(),
                kind: source.kind,
                versions: vec![SurfaceVersion {
                    version: 1,
                    semantic_center,
                    member_points,
                    member_edges,
                    activation_threshold: source.current().activation_threshold,
                    valid_from: command.changed_at,
                    valid_to: None,
                    status: SurfaceStatus::Active,
                    confidence: source.current().confidence,
                    boundary: source.current().boundary.clone(),
                    evidence,
                    model_version_id: source.current().model_version_id.clone(),
                }],
            });
        }

        begin_next_version(
            store.surfaces.get_mut(&command.source_surface_id).ok_or(
                SurfaceRepositoryError::MissingSurface(command.source_surface_id),
            )?,
            command.changed_at,
            command.evidence.link,
        )?
        .status = SurfaceStatus::Split;
        for successor in &successors {
            store.surfaces.insert(successor.id, successor.clone());
        }
        Ok(successors)
    }

    /// Returns a Surface with its full ordered version history.
    pub async fn get(&self, surface_id: SurfaceId) -> Option<CognitiveSurface> {
        self.store.lock().await.surfaces.get(&surface_id).cloned()
    }

    /// Returns contradiction edges whose endpoints are members of the current version.
    ///
    /// # Errors
    ///
    /// Returns [`SurfaceRepositoryError::MissingSurface`] when the Surface is unknown.
    pub async fn conflicts(
        &self,
        surface_id: SurfaceId,
    ) -> Result<Vec<SurfaceConflict>, SurfaceRepositoryError> {
        let store = self.store.lock().await;
        let surface = store
            .surfaces
            .get(&surface_id)
            .ok_or(SurfaceRepositoryError::MissingSurface(surface_id))?;
        Ok(surface
            .current()
            .member_edges
            .iter()
            .filter_map(|edge_id| self.edges.get(edge_id))
            .filter(|edge| edge.kind == EdgeKind::Contradicts)
            .map(|edge| SurfaceConflict {
                edge_id: edge.id,
                source_point_id: edge.source_point_id,
                target_point_id: edge.target_point_id,
            })
            .collect())
    }

    /// Returns every current Surface membership for a Point.
    pub async fn memberships(&self, point_id: PointId) -> Vec<SurfaceMembership> {
        let store = self.store.lock().await;
        store
            .surfaces
            .values()
            .filter_map(|surface| {
                surface
                    .current()
                    .member_points
                    .get(&point_id)
                    .map(|weight| SurfaceMembership {
                        surface_id: surface.id,
                        point_id,
                        member_weight: *weight,
                    })
            })
            .collect()
    }

    fn validate_evidence_scope(
        &self,
        evidence_scope: &MemoryScope,
    ) -> Result<(), SurfaceRepositoryError> {
        if evidence_scope == &self.scope {
            Ok(())
        } else {
            Err(SurfaceRepositoryError::ScopeMismatch)
        }
    }

    fn center_for_members(
        &self,
        members: &BTreeMap<PointId, f32>,
    ) -> Result<Vec<f32>, SurfaceRepositoryError> {
        let mut center = Vec::new();
        let mut total_weight = 0.0_f32;
        for (point_id, weight) in members {
            let point = self
                .points
                .get(point_id)
                .ok_or(SurfaceRepositoryError::MissingPoint(*point_id))?;
            if center.is_empty() {
                center.resize(point.semantic_vector.len(), 0.0);
            } else if center.len() != point.semantic_vector.len() {
                return Err(SurfaceRepositoryError::SemanticDimensionMismatch(*point_id));
            }
            for (sum, component) in center.iter_mut().zip(&point.semantic_vector) {
                *sum += component * *weight;
            }
            total_weight += *weight;
        }
        if total_weight > 0.0 {
            for component in &mut center {
                *component /= total_weight;
            }
        }
        Ok(center)
    }

    fn member_edges_for(
        &self,
        members: &BTreeMap<PointId, f32>,
        effective_at: chrono::DateTime<chrono::Utc>,
    ) -> Vec<jc_memory_protocol::EdgeId> {
        self.edges
            .values()
            .filter(|edge| {
                members.contains_key(&edge.source_point_id)
                    && members.contains_key(&edge.target_point_id)
                    && edge.valid_from <= effective_at
                    && edge.valid_to.is_none_or(|valid_to| effective_at < valid_to)
            })
            .map(|edge| edge.id)
            .collect()
    }

    async fn change_status(
        &self,
        surface_id: SurfaceId,
        target: SurfaceStatus,
        changed_at: chrono::DateTime<chrono::Utc>,
        evidence: crate::SurfaceEvidence,
        allowed: &[SurfaceStatus],
    ) -> Result<(), SurfaceRepositoryError> {
        self.validate_evidence_scope(&evidence.scope)?;
        let mut store = self.store.lock().await;
        let surface = store
            .surfaces
            .get_mut(&surface_id)
            .ok_or(SurfaceRepositoryError::MissingSurface(surface_id))?;
        let current = surface.current().status;
        if !allowed.contains(&current) {
            return Err(SurfaceRepositoryError::InvalidLifecycleTransition {
                from: current,
                to: target,
            });
        }
        begin_next_version(surface, changed_at, evidence.link)?.status = target;
        Ok(())
    }
}

fn begin_next_version(
    surface: &mut CognitiveSurface,
    changed_at: chrono::DateTime<chrono::Utc>,
    evidence: jc_memory_graph::EvidenceLink,
) -> Result<&mut SurfaceVersion, SurfaceRepositoryError> {
    let current = surface.current();
    if changed_at < current.valid_from {
        return Err(SurfaceRepositoryError::OutOfOrder);
    }
    let mut next = current.clone();
    next.version += 1;
    next.valid_from = changed_at;
    next.valid_to = None;
    next.evidence.push(evidence);
    surface.current_mut().valid_to = Some(changed_at);
    surface.versions.push(next);
    Ok(surface.current_mut())
}

fn validate_unit_interval(field: &'static str, value: f32) -> Result<(), SurfaceRepositoryError> {
    if value.is_finite() && (0.0..=1.0).contains(&value) {
        Ok(())
    } else {
        Err(SurfaceRepositoryError::InvalidUnitInterval { field })
    }
}

fn validate_membership_status(
    status: SurfaceStatus,
    automatic: bool,
) -> Result<(), SurfaceRepositoryError> {
    match status {
        SurfaceStatus::Active => Ok(()),
        SurfaceStatus::Frozen if !automatic => Ok(()),
        SurfaceStatus::Frozen => Err(SurfaceRepositoryError::SurfaceFrozen),
        SurfaceStatus::Inactive | SurfaceStatus::Split | SurfaceStatus::Merged => {
            Err(SurfaceRepositoryError::SurfaceInactive)
        }
    }
}

fn validate_terminal_transition(
    current: SurfaceStatus,
    target: SurfaceStatus,
) -> Result<(), SurfaceRepositoryError> {
    if matches!(current, SurfaceStatus::Active | SurfaceStatus::Frozen) {
        Ok(())
    } else {
        Err(SurfaceRepositoryError::InvalidLifecycleTransition {
            from: current,
            to: target,
        })
    }
}

fn validate_center_dimension(
    version: &SurfaceVersion,
    point: &SurfacePoint,
) -> Result<(), SurfaceRepositoryError> {
    if version.semantic_center.is_empty()
        || version.semantic_center.len() == point.semantic_vector.len()
    {
        Ok(())
    } else {
        Err(SurfaceRepositoryError::SemanticDimensionMismatch(
            point.point.id,
        ))
    }
}

fn add_to_center(
    version: &mut SurfaceVersion,
    point: &SurfacePoint,
    member_weight: f32,
) -> Result<(), SurfaceRepositoryError> {
    let old_weight = version
        .member_points
        .get(&point.point.id)
        .copied()
        .unwrap_or(0.0);
    let old_total: f32 = version.member_points.values().sum();
    if !version.semantic_center.is_empty()
        && version.semantic_center.len() != point.semantic_vector.len()
    {
        return Err(SurfaceRepositoryError::SemanticDimensionMismatch(
            point.point.id,
        ));
    }
    if version.semantic_center.is_empty() {
        version
            .semantic_center
            .resize(point.semantic_vector.len(), 0.0);
    }
    let new_total = old_total - old_weight + member_weight;
    for (center, component) in version
        .semantic_center
        .iter_mut()
        .zip(&point.semantic_vector)
    {
        let weighted_sum = *center * old_total - component * old_weight + component * member_weight;
        *center = if new_total > 0.0 {
            weighted_sum / new_total
        } else {
            0.0
        };
    }
    version.member_points.insert(point.point.id, member_weight);
    Ok(())
}

fn remove_from_center(
    version: &mut SurfaceVersion,
    point: &SurfacePoint,
) -> Result<(), SurfaceRepositoryError> {
    let old_weight = version.member_points[&point.point.id];
    let old_total: f32 = version.member_points.values().sum();
    if version.semantic_center.len() != point.semantic_vector.len() {
        return Err(SurfaceRepositoryError::SemanticDimensionMismatch(
            point.point.id,
        ));
    }
    let new_total = old_total - old_weight;
    for (center, component) in version
        .semantic_center
        .iter_mut()
        .zip(&point.semantic_vector)
    {
        let weighted_sum = *center * old_total - component * old_weight;
        *center = if new_total > 0.0 {
            weighted_sum / new_total
        } else {
            0.0
        };
    }
    version.member_points.remove(&point.point.id);
    if version.member_points.is_empty() {
        version.semantic_center.clear();
    }
    Ok(())
}
