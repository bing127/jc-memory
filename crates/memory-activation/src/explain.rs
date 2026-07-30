//! Deterministic evidence provenance for bounded Recall results.

use std::collections::{BTreeMap, BTreeSet};

use jc_memory_protocol::{EdgeId, EdgeKind, FragmentId, PointId, SurfaceId, ValidityStatus};
use serde::{Deserialize, Serialize};

use crate::{IndexedPoint, RecallLimits, RecallResult};

/// One initial Point selected before graph propagation.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct SeedTrace {
    /// Selected seed Point.
    pub point_id: PointId,
    /// Initial activation score.
    pub score: f32,
}

/// One directed Edge traversed by bounded activation.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct TraversalTrace {
    /// Traversed Edge identity.
    pub edge_id: EdgeId,
    /// Directed source Point.
    pub source_point_id: PointId,
    /// Directed target Point.
    pub target_point_id: PointId,
    /// Relationship semantics.
    pub kind: EdgeKind,
    /// Target activation after this traversal.
    pub score: f32,
    /// Target depth from its selected seed.
    pub depth: usize,
}

/// One Surface version activated by recalled member Points.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct SurfaceActivationTrace {
    /// Stable Surface identity.
    pub surface_id: SurfaceId,
    /// Effective Surface version.
    pub version: u64,
    /// Weighted activation score.
    pub activation: f32,
}

/// Complete bounded provenance for one Recall result.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ExplainTrace {
    /// Initial Point activations.
    pub seeds: Vec<SeedTrace>,
    /// Directed traversals in result order.
    pub traversals: Vec<TraversalTrace>,
    /// Activated Surface versions.
    pub surfaces: Vec<SurfaceActivationTrace>,
    /// Traversed relationships without human or tool verification.
    pub inferred_edges: Vec<EdgeId>,
    /// Authoritative Fragments behind verified recalled Points.
    pub verified_fragments: Vec<FragmentId>,
    /// Hard limits applied to the Recall.
    pub limits: RecallLimits,
    /// Whether a hard limit omitted any candidate.
    pub truncated: bool,
}

pub(crate) fn build(
    indexed_points: &BTreeMap<PointId, IndexedPoint>,
    result: &RecallResult,
    limits: RecallLimits,
) -> ExplainTrace {
    let activations: BTreeMap<_, _> = result
        .points
        .iter()
        .map(|point| (point.point_id, (point.activation, point.depth)))
        .collect();
    let seeds = result
        .points
        .iter()
        .filter(|point| point.depth == 0)
        .map(|point| SeedTrace {
            point_id: point.point_id,
            score: point.activation,
        })
        .collect();
    let traversals = result
        .edges
        .iter()
        .map(|edge| {
            let (score, depth) = activations
                .get(&edge.target_point_id)
                .copied()
                .unwrap_or((0.0, 0));
            TraversalTrace {
                edge_id: edge.id,
                source_point_id: edge.source_point_id,
                target_point_id: edge.target_point_id,
                kind: edge.kind,
                score,
                depth,
            }
        })
        .collect();
    let surfaces = result
        .activated_surfaces
        .iter()
        .map(|surface| SurfaceActivationTrace {
            surface_id: surface.surface_id,
            version: surface.version,
            activation: surface.activation,
        })
        .collect();
    let inferred_edges = result
        .edges
        .iter()
        .filter(|edge| !edge.human_confirmed && !edge.tool_verified)
        .map(|edge| edge.id)
        .collect();
    let verified_fragments = result
        .points
        .iter()
        .filter_map(|point| indexed_points.get(&point.point_id))
        .filter(|indexed| indexed.point.validity == ValidityStatus::Verified)
        .flat_map(|indexed| indexed.point.source_fragments.iter().copied())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();

    ExplainTrace {
        seeds,
        traversals,
        surfaces,
        inferred_edges,
        verified_fragments,
        limits,
        truncated: result.trace.truncated,
    }
}
