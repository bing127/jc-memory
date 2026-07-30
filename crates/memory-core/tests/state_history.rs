//! 状态历史与共同出现关系的 M2 验收测试。

use chrono::{TimeZone as _, Utc};
use jc_memory_episodic::{StateEvidence, StateRepository, StateResolution};
use jc_memory_graph::{CognitivePoint, EdgeCandidate, EvidenceLink, PointKind};
use jc_memory_protocol::{
    EdgeKind, EventId, FragmentId, MemoryScope, NamespaceId, PointId, ValidityStatus,
};
use serde_json::json;

#[tokio::test]
async fn state_replacement_keeps_current_and_historical_values() {
    let scope = MemoryScope::new(NamespaceId::new());
    let repository = StateRepository::new(scope.clone());
    let initial_at = at(2024, 1, 1);
    let replacement_at = at(2025, 6, 1);

    repository
        .record(
            "database",
            json!("PostgreSQL"),
            initial_at,
            state_evidence(&scope, initial_at),
        )
        .await
        .expect("写入初始状态");
    repository
        .supersede(
            "database",
            json!("MySQL"),
            replacement_at,
            state_evidence(&scope, replacement_at),
        )
        .await
        .expect("替换当前状态");

    assert_eq!(
        resolved(repository.current("database").await).value,
        json!("MySQL")
    );
    assert_eq!(
        resolved(repository.at("database", at(2024, 6, 1)).await).value,
        json!("PostgreSQL")
    );
    assert_eq!(repository.history("database").await.len(), 2);
}

#[test]
fn co_occurrence_never_claims_verified_causality() {
    let scope = MemoryScope::new(NamespaceId::new());
    let source = point(scope.clone());
    let target = point(scope);
    let edge = EdgeCandidate::co_activated(&source, &target, evidence()).expect("建立共同出现关系");

    assert_eq!(edge.kind, EdgeKind::CoActivated);
    assert_ne!(edge.kind, EdgeKind::Causes);
    assert!(!edge.human_confirmed);
    assert!(!edge.tool_verified);
}

fn resolved(resolution: StateResolution) -> jc_memory_episodic::StateVersion {
    match resolution {
        StateResolution::Resolved(version) => version,
        StateResolution::Conflicted(_) => panic!("预期状态只有一个有效版本"),
        StateResolution::Missing => panic!("预期状态存在"),
    }
}

fn state_evidence(scope: &MemoryScope, occurred_at: chrono::DateTime<Utc>) -> StateEvidence {
    let point = CognitivePoint {
        id: PointId::new(),
        scope: scope.clone(),
        kind: PointKind::StateChange,
        source_fragments: vec![FragmentId::new()],
        occurred_at,
        confidence: 1.0,
        validity: ValidityStatus::Verified,
    };
    StateEvidence {
        link: EvidenceLink {
            fragment_id: point.source_fragments[0],
            event_id: Some(EventId::new()),
            evidence_ref_id: None,
        },
        point,
    }
}

fn point(scope: MemoryScope) -> CognitivePoint {
    CognitivePoint {
        id: PointId::new(),
        scope,
        kind: PointKind::Failure,
        source_fragments: vec![FragmentId::new()],
        occurred_at: at(2026, 7, 30),
        confidence: 0.8,
        validity: ValidityStatus::Unverified,
    }
}

fn evidence() -> EvidenceLink {
    EvidenceLink {
        fragment_id: FragmentId::new(),
        event_id: Some(EventId::new()),
        evidence_ref_id: None,
    }
}

fn at(year: i32, month: u32, day: u32) -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(year, month, day, 0, 0, 0).unwrap()
}
