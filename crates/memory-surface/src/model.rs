//! Versioned cognitive surface domain types.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use jc_memory_graph::EvidenceLink;
use jc_memory_protocol::{EdgeId, MemoryScope, PointId, SurfaceId, SurfaceKind, SurfaceStatus};
use serde::{Deserialize, Serialize};

/// Semantic and capacity limits applied to surface membership.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct SurfaceBoundary {
    /// Lowest accepted membership weight.
    pub min_member_weight: f32,
    /// Maximum number of member points in one surface version.
    pub max_members: usize,
}

/// One immutable snapshot in a cognitive surface's history.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct SurfaceVersion {
    /// Monotonically increasing version number, starting at one.
    pub version: u64,
    /// Incrementally maintained semantic center.
    pub semantic_center: Vec<f32>,
    /// Point membership weights for this version.
    pub member_points: BTreeMap<PointId, f32>,
    /// Edges whose endpoints are both members in this version.
    pub member_edges: Vec<EdgeId>,
    /// Minimum activation required to activate the surface.
    pub activation_threshold: f32,
    /// Inclusive beginning of this version's effective interval.
    pub valid_from: DateTime<Utc>,
    /// Exclusive end of this version's effective interval.
    pub valid_to: Option<DateTime<Utc>>,
    /// Lifecycle state of this version.
    pub status: SurfaceStatus,
    /// Confidence in this derived surface.
    pub confidence: f32,
    /// Membership boundary applied to this version.
    pub boundary: SurfaceBoundary,
    /// Authoritative links supporting this derived version.
    pub evidence: Vec<EvidenceLink>,
    /// Model version that proposed the surface, when applicable.
    pub model_version_id: Option<String>,
}

/// A cognitive surface and all of its reconstructable versions.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct CognitiveSurface {
    /// Stable identifier shared by every version.
    pub id: SurfaceId,
    /// Exact ownership scope of the surface and every member.
    pub scope: MemoryScope,
    /// Origin and intended lifetime of the surface.
    pub kind: SurfaceKind,
    /// Ordered version history, including the current version last.
    pub versions: Vec<SurfaceVersion>,
}

impl CognitiveSurface {
    /// Returns the current version.
    ///
    /// # Panics
    ///
    /// Panics only if a `CognitiveSurface` was deserialized or constructed with
    /// an empty version history. Repository-created Surfaces always start at version one.
    #[must_use]
    pub fn current(&self) -> &SurfaceVersion {
        self.versions
            .last()
            .expect("a cognitive surface always has an initial version")
    }

    pub(crate) fn current_mut(&mut self) -> &mut SurfaceVersion {
        self.versions
            .last_mut()
            .expect("a cognitive surface always has an initial version")
    }
}
