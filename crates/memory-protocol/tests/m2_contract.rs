//! Stable M2 graph and recall wire-contract tests.

use std::str::FromStr as _;

use jc_memory_protocol::{
    EdgeId, EdgeKind, PointId, RecallId, RecallMode, StateId, StateVersionId, SurfaceId,
    SurfaceKind, SurfaceStatus,
};

#[test]
fn recall_and_edge_names_are_stable() {
    let recall_modes = [
        (RecallMode::Exact, "exact"),
        (RecallMode::Semantic, "semantic"),
        (RecallMode::Temporal, "temporal"),
        (RecallMode::Causal, "causal"),
        (RecallMode::Entity, "entity"),
        (RecallMode::Associative, "associative"),
        (RecallMode::Procedural, "procedural"),
        (RecallMode::CurrentState, "current_state"),
        (RecallMode::HistoricalState, "historical_state"),
        (RecallMode::DeepReconstruction, "deep_reconstruction"),
    ];
    for (mode, expected) in recall_modes {
        assert_eq!(
            serde_json::to_string(&mode).unwrap(),
            format!("\"{expected}\"")
        );
        assert_eq!(
            serde_json::from_str::<RecallMode>(&format!("\"{expected}\"")).unwrap(),
            mode
        );
    }

    let edge_kinds = [
        (EdgeKind::TemporalBefore, "temporal_before"),
        (EdgeKind::TemporalAfter, "temporal_after"),
        (EdgeKind::Causes, "causes"),
        (EdgeKind::CausedBy, "caused_by"),
        (EdgeKind::Supports, "supports"),
        (EdgeKind::Contradicts, "contradicts"),
        (EdgeKind::Supersedes, "supersedes"),
        (EdgeKind::DependsOn, "depends_on"),
        (EdgeKind::PartOf, "part_of"),
        (EdgeKind::SameEntity, "same_entity"),
        (EdgeKind::CoActivated, "co_activated"),
        (EdgeKind::PredictedBy, "predicted_by"),
        (EdgeKind::ValidatedBy, "validated_by"),
        (EdgeKind::DisprovedBy, "disproved_by"),
        (EdgeKind::RecalledBy, "recalled_by"),
        (EdgeKind::UsedInDecision, "used_in_decision"),
        (EdgeKind::ExecutedAs, "executed_as"),
        (EdgeKind::ResultedIn, "resulted_in"),
    ];
    for (kind, expected) in edge_kinds {
        assert_eq!(
            serde_json::to_string(&kind).unwrap(),
            format!("\"{expected}\"")
        );
        assert_eq!(
            serde_json::from_str::<EdgeKind>(&format!("\"{expected}\"")).unwrap(),
            kind
        );
    }
}

#[test]
fn surface_names_are_stable() {
    let kinds = [
        (SurfaceKind::Explicit, "explicit"),
        (SurfaceKind::Dynamic, "dynamic"),
        (SurfaceKind::TemporaryTask, "temporary_task"),
    ];
    for (kind, expected) in kinds {
        assert_eq!(
            serde_json::to_string(&kind).unwrap(),
            format!("\"{expected}\"")
        );
        assert_eq!(
            serde_json::from_str::<SurfaceKind>(&format!("\"{expected}\"")).unwrap(),
            kind
        );
    }

    let statuses = [
        (SurfaceStatus::Active, "active"),
        (SurfaceStatus::Frozen, "frozen"),
        (SurfaceStatus::Inactive, "inactive"),
        (SurfaceStatus::Split, "split"),
        (SurfaceStatus::Merged, "merged"),
    ];
    for (status, expected) in statuses {
        assert_eq!(
            serde_json::to_string(&status).unwrap(),
            format!("\"{expected}\"")
        );
        assert_eq!(
            serde_json::from_str::<SurfaceStatus>(&format!("\"{expected}\"")).unwrap(),
            status
        );
    }
}

#[test]
fn m2_ids_are_uuid_v7_and_round_trip() {
    macro_rules! assert_id_contract {
        ($type:ty) => {{
            let id = <$type>::new();
            assert_eq!(id.as_uuid().get_version_num(), 7);
            let json = serde_json::to_string(&id).unwrap();
            assert_eq!(serde_json::from_str::<$type>(&json).unwrap(), id);
            assert_eq!(<$type>::from_str(&id.to_string()).unwrap(), id);
        }};
    }

    assert_id_contract!(PointId);
    assert_id_contract!(EdgeId);
    assert_id_contract!(SurfaceId);
    assert_id_contract!(StateId);
    assert_id_contract!(StateVersionId);
    assert_id_contract!(RecallId);
}
