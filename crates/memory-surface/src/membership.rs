//! Surface membership and authoritative evidence links.

use jc_memory_graph::{CognitivePoint, EvidenceLink};
use jc_memory_protocol::{EdgeId, MemoryScope, PointId, SurfaceId};
use serde::{Deserialize, Serialize};

/// A graph point paired with its deterministic semantic vector.
#[derive(Clone, Debug, PartialEq)]
pub struct SurfacePoint {
    /// Evidence-backed graph point.
    pub point: CognitivePoint,
    /// Model-independent vector used for center maintenance.
    pub semantic_vector: Vec<f32>,
}

/// A scope-qualified evidence link supporting a surface mutation.
#[derive(Clone, Debug, PartialEq)]
pub struct SurfaceEvidence {
    /// Exact ownership scope of the referenced authoritative record.
    pub scope: MemoryScope,
    /// Fragment, Event, or `EvidenceRef` link.
    pub link: EvidenceLink,
}

/// Current membership of one point in one surface.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct SurfaceMembership {
    /// Surface containing the point.
    pub surface_id: SurfaceId,
    /// Member point.
    pub point_id: PointId,
    /// Membership strength in the unit interval.
    pub member_weight: f32,
}

/// A contradictory directed edge whose endpoints share one Surface.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SurfaceConflict {
    /// Evidence-linked contradiction edge.
    pub edge_id: EdgeId,
    /// Directed source member point.
    pub source_point_id: PointId,
    /// Directed target member point.
    pub target_point_id: PointId,
}
