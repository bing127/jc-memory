//! Cognitive point construction behavior.

use chrono::{TimeZone as _, Utc};
use jc_memory_graph::{CognitivePoint, PointCreationError, PointKind};
use jc_memory_protocol::{
    CognitiveFragment, FragmentContent, FragmentId, FragmentKind, FragmentSource, MemoryScope,
    NamespaceId, Perspective, SecurityLabel, SourceType, ValidityStatus,
};

#[test]
fn atomic_fragment_kinds_map_to_points() {
    let cases = [
        (FragmentKind::Fact, PointKind::Fact),
        (FragmentKind::Decision, PointKind::Decision),
        (FragmentKind::Constraint, PointKind::Constraint),
        (FragmentKind::StateTransition, PointKind::StateChange),
        (FragmentKind::Action, PointKind::Action),
        (FragmentKind::Outcome, PointKind::Result),
        (FragmentKind::Success, PointKind::Result),
        (FragmentKind::Failure, PointKind::Failure),
        (FragmentKind::Preference, PointKind::Preference),
        (FragmentKind::Correction, PointKind::Correction),
    ];

    for (fragment_kind, expected_point_kind) in cases {
        let point = CognitivePoint::try_from(&fixture(fragment_kind)).unwrap();
        assert_eq!(point.kind, expected_point_kind);
    }
}

#[test]
fn point_preserves_authoritative_fragment_fields() {
    let mut fragment = fixture(FragmentKind::Failure);
    fragment.confidence = 0.73;
    fragment.validity = ValidityStatus::Disputed;

    let point = CognitivePoint::try_from(&fragment).unwrap();

    assert_eq!(point.scope, fragment.scope);
    assert_eq!(point.kind, PointKind::Failure);
    assert_eq!(point.source_fragments, vec![fragment.id]);
    assert_eq!(point.occurred_at, fragment.occurred_at);
    assert_eq!(point.confidence.to_bits(), fragment.confidence.to_bits());
    assert_eq!(point.validity, fragment.validity);
}

#[test]
fn non_atomic_fragments_do_not_become_points() {
    for kind in [FragmentKind::Observation, FragmentKind::Prediction] {
        assert_eq!(
            CognitivePoint::try_from(&fixture(kind)).unwrap_err(),
            PointCreationError::UnsupportedFragmentKind(kind)
        );
    }
}

fn fixture(kind: FragmentKind) -> CognitiveFragment {
    let occurred_at = Utc.with_ymd_and_hms(2026, 7, 29, 12, 0, 0).unwrap();
    CognitiveFragment {
        id: FragmentId::new(),
        scope: MemoryScope::new(NamespaceId::new()),
        session_id: None,
        trace_id: None,
        kind,
        content: FragmentContent::Text {
            text: "deployment failed".to_owned(),
        },
        source: FragmentSource {
            source_type: SourceType::Tool,
            source_id: "deployment-runner".to_owned(),
            actor_id: None,
            observer_id: Some("test-observer".to_owned()),
            tool_id: Some("deployment-runner".to_owned()),
            uri: None,
        },
        perspective: Perspective::Tool,
        occurred_at,
        observed_at: occurred_at,
        confidence: 0.8,
        importance: 0.6,
        validity: ValidityStatus::Unverified,
        security_label: SecurityLabel::Internal,
        evidence: Vec::new(),
        metadata: serde_json::Map::new(),
    }
}
