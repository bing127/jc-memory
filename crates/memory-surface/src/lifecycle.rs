//! Commands that create and mutate cognitive surfaces.

use chrono::{DateTime, Utc};
use jc_memory_protocol::{PointId, SurfaceId, SurfaceKind};

use crate::{SurfaceBoundary, SurfaceEvidence};

/// Creates the initial version of a cognitive surface.
#[derive(Clone, Debug, PartialEq)]
pub struct CreateSurface {
    /// Surface origin and intended lifetime.
    pub kind: SurfaceKind,
    /// Minimum activation required during recall.
    pub activation_threshold: f32,
    /// Confidence in the initial derived surface.
    pub confidence: f32,
    /// Initial semantic and capacity boundary.
    pub boundary: SurfaceBoundary,
    /// Authoritative evidence supporting creation.
    pub evidence: SurfaceEvidence,
    /// Model version that proposed the surface, when applicable.
    pub model_version_id: Option<String>,
    /// Effective time of the initial version.
    pub created_at: DateTime<Utc>,
}

/// Adds or updates a point membership.
#[derive(Clone, Debug, PartialEq)]
pub struct AddMember {
    /// Surface to mutate.
    pub surface_id: SurfaceId,
    /// Point to add.
    pub point_id: PointId,
    /// Membership strength in the unit interval.
    pub member_weight: f32,
    /// Whether the mutation was proposed automatically.
    pub automatic: bool,
    /// Effective time of the mutation.
    pub changed_at: DateTime<Utc>,
    /// Authoritative evidence supporting the mutation.
    pub evidence: SurfaceEvidence,
}

/// Removes a point membership from a Surface.
#[derive(Clone, Debug, PartialEq)]
pub struct RemoveMember {
    /// Surface to mutate.
    pub surface_id: SurfaceId,
    /// Point to remove.
    pub point_id: PointId,
    /// Whether the mutation was proposed automatically.
    pub automatic: bool,
    /// Effective time of the mutation.
    pub changed_at: DateTime<Utc>,
    /// Authoritative evidence supporting the mutation.
    pub evidence: SurfaceEvidence,
}

/// Freezes a Surface against automatic membership changes.
#[derive(Clone, Debug, PartialEq)]
pub struct FreezeSurface {
    /// Surface to freeze.
    pub surface_id: SurfaceId,
    /// Effective time of the status change.
    pub changed_at: DateTime<Utc>,
    /// Authoritative evidence supporting the status change.
    pub evidence: SurfaceEvidence,
}

/// Deactivates a Surface and excludes it from later membership changes.
#[derive(Clone, Debug, PartialEq)]
pub struct DeactivateSurface {
    /// Surface to deactivate.
    pub surface_id: SurfaceId,
    /// Effective time of the status change.
    pub changed_at: DateTime<Utc>,
    /// Authoritative evidence supporting the status change.
    pub evidence: SurfaceEvidence,
}

/// Merges two or more active or frozen Surfaces into one new Surface.
#[derive(Clone, Debug, PartialEq)]
pub struct MergeSurfaces {
    /// Source Surfaces that will receive terminal `merged` versions.
    pub source_surface_ids: Vec<SurfaceId>,
    /// Kind assigned to the successor Surface.
    pub kind: SurfaceKind,
    /// Activation threshold assigned to the successor.
    pub activation_threshold: f32,
    /// Confidence assigned to the successor.
    pub confidence: f32,
    /// Boundary assigned to the successor.
    pub boundary: SurfaceBoundary,
    /// Effective time of the merge and successor creation.
    pub changed_at: DateTime<Utc>,
    /// Authoritative evidence supporting the merge.
    pub evidence: SurfaceEvidence,
    /// Model version that proposed the merge, when applicable.
    pub model_version_id: Option<String>,
}

/// One disjoint member partition used to split a Surface.
#[derive(Clone, Debug, PartialEq)]
pub struct SurfacePartition {
    /// Member points assigned to this successor.
    pub member_point_ids: Vec<PointId>,
}

/// Splits one active or frozen Surface into two or more successors.
#[derive(Clone, Debug, PartialEq)]
pub struct SplitSurface {
    /// Source Surface that will receive a terminal `split` version.
    pub source_surface_id: SurfaceId,
    /// Complete, disjoint partition of the source's current member points.
    pub partitions: Vec<SurfacePartition>,
    /// Effective time of the split and successor creation.
    pub changed_at: DateTime<Utc>,
    /// Authoritative evidence supporting the split.
    pub evidence: SurfaceEvidence,
}
