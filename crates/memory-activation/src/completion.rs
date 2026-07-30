//! Surface activation and bounded evidence completion.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use jc_memory_protocol::{
    EdgeKind, MemoryScope, PointId, SurfaceId, SurfaceStatus, ValidityStatus,
};
use jc_memory_surface::{CognitiveSurface, SurfaceVersion};

use crate::ActivatedPoint;

const EVIDENCE_EDGES: &[EdgeKind] = &[
    EdgeKind::Supports,
    EdgeKind::Contradicts,
    EdgeKind::ValidatedBy,
    EdgeKind::DisprovedBy,
];

pub(crate) const fn has_evidence_gap(validity: ValidityStatus) -> bool {
    matches!(
        validity,
        ValidityStatus::Unverified | ValidityStatus::Disputed
    )
}

pub(crate) fn is_evidence_edge(kind: EdgeKind) -> bool {
    EVIDENCE_EDGES.contains(&kind)
}

/// A live Surface activated by visited member Points.
#[derive(Clone, Debug, PartialEq)]
pub struct ActivatedSurface {
    /// Stable Surface identifier.
    pub surface_id: SurfaceId,
    /// Historical or current Surface version used for activation.
    pub version: u64,
    /// Weighted mean activation across all Surface members.
    pub activation: f32,
}

pub(crate) fn activate_surfaces(
    surfaces: &[CognitiveSurface],
    scope: &MemoryScope,
    effective_at: DateTime<Utc>,
    points: &[ActivatedPoint],
) -> Vec<ActivatedSurface> {
    let activations: BTreeMap<PointId, f32> = points
        .iter()
        .map(|point| (point.point_id, point.activation))
        .collect();
    let mut activated = Vec::new();
    for surface in surfaces.iter().filter(|surface| &surface.scope == scope) {
        let Some(version) = version_at(surface, effective_at) else {
            continue;
        };
        if !matches!(
            version.status,
            SurfaceStatus::Active | SurfaceStatus::Frozen
        ) {
            continue;
        }
        let total_weight: f32 = version.member_points.values().sum();
        if total_weight <= 0.0 {
            continue;
        }
        let weighted_activation: f32 = version
            .member_points
            .iter()
            .filter_map(|(point_id, weight)| {
                activations
                    .get(point_id)
                    .map(|activation| activation * weight)
            })
            .sum();
        let activation = weighted_activation / total_weight;
        if activation >= version.activation_threshold {
            activated.push(ActivatedSurface {
                surface_id: surface.id,
                version: version.version,
                activation,
            });
        }
    }
    activated.sort_by(|left, right| {
        right
            .activation
            .total_cmp(&left.activation)
            .then_with(|| left.surface_id.cmp(&right.surface_id))
    });
    activated
}

fn version_at(surface: &CognitiveSurface, effective_at: DateTime<Utc>) -> Option<&SurfaceVersion> {
    surface.versions.iter().rev().find(|version| {
        version.valid_from <= effective_at
            && version
                .valid_to
                .is_none_or(|valid_to| effective_at < valid_to)
    })
}
