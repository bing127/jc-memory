//! Versioned state history and conflict behavior.

use chrono::{TimeZone as _, Utc};
use jc_memory_episodic::{StateEvidence, StateRepository, StateResolution};
use jc_memory_graph::{CognitivePoint, EvidenceLink, PointKind};
use jc_memory_protocol::{
    EdgeKind, EventId, FragmentId, MemoryScope, NamespaceId, PointId, ValidityStatus,
};
use serde_json::json;

#[tokio::test]
async fn replacement_preserves_current_history_and_supersedes_edge() {
    let scope = MemoryScope::new(NamespaceId::new());
    let repository = StateRepository::new(scope.clone());
    let initial_time = at(2024, 1, 1);
    let replacement_time = at(2025, 6, 1);
    let initial_evidence = evidence(&scope, initial_time);
    let replacement_evidence = evidence(&scope, replacement_time);

    let initial = repository
        .record(
            "database",
            json!("PostgreSQL"),
            initial_time,
            initial_evidence.clone(),
        )
        .await
        .unwrap();
    let replacement = repository
        .supersede(
            "database",
            json!("MySQL"),
            replacement_time,
            replacement_evidence.clone(),
        )
        .await
        .unwrap();

    let current = resolved(repository.current("database").await);
    assert_eq!(current.value, json!("MySQL"));
    assert_eq!(current.previous_version_id, Some(initial.version.id));
    let historical = resolved(repository.at("database", at(2024, 6, 1)).await);
    assert_eq!(historical.value, json!("PostgreSQL"));

    let history = repository.history("database").await;
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].valid_to, Some(replacement_time));
    assert_eq!(history[0].validity, ValidityStatus::Superseded);

    let edge = replacement.supersedes_edge.unwrap();
    assert_eq!(edge.kind, EdgeKind::Supersedes);
    assert_eq!(edge.source_point_id, replacement_evidence.point.id);
    assert_eq!(edge.target_point_id, initial_evidence.point.id);
    assert_eq!(edge.evidence, vec![replacement_evidence.link]);
}

#[tokio::test]
async fn different_values_at_the_same_time_remain_conflicted() {
    let scope = MemoryScope::new(NamespaceId::new());
    let repository = StateRepository::new(scope.clone());
    let effective_at = at(2025, 6, 1);

    repository
        .record(
            "database",
            json!("PostgreSQL"),
            effective_at,
            evidence(&scope, effective_at),
        )
        .await
        .unwrap();
    let conflict = repository
        .supersede(
            "database",
            json!("MySQL"),
            effective_at,
            evidence(&scope, effective_at),
        )
        .await
        .unwrap();

    assert!(conflict.supersedes_edge.is_none());
    let StateResolution::Conflicted(versions) = repository.current("database").await else {
        panic!("same-time competing values must remain conflicted");
    };
    assert_eq!(versions.len(), 2);
    assert!(
        versions
            .iter()
            .any(|version| version.value == json!("PostgreSQL"))
    );
    assert!(
        versions
            .iter()
            .any(|version| version.value == json!("MySQL"))
    );
}

fn resolved(resolution: StateResolution) -> jc_memory_episodic::StateVersion {
    match resolution {
        StateResolution::Resolved(version) => version,
        StateResolution::Conflicted(_) => panic!("expected one resolved state"),
        StateResolution::Missing => panic!("expected state to exist"),
    }
}

fn evidence(scope: &MemoryScope, occurred_at: chrono::DateTime<Utc>) -> StateEvidence {
    let fragment_id = FragmentId::new();
    StateEvidence {
        point: CognitivePoint {
            id: PointId::new(),
            scope: scope.clone(),
            kind: PointKind::StateChange,
            source_fragments: vec![fragment_id],
            occurred_at,
            confidence: 0.9,
            validity: ValidityStatus::Verified,
        },
        link: EvidenceLink {
            fragment_id,
            event_id: Some(EventId::new()),
            evidence_ref_id: None,
        },
    }
}

fn at(year: i32, month: u32, day: u32) -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(year, month, day, 0, 0, 0).unwrap()
}
