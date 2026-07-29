//! Evidence requirements for semantic and procedural memory.

use jc_memory_protocol::{
    EventId, FragmentId, FragmentKind, MemoryScope, NamespaceId, ValidityStatus,
};
use jc_memory_semantic::{
    ProcedureEvidence, ProcedureExperience, ProcedureExperienceError, SemanticEntry,
    SemanticEvidence, SemanticKind, SemanticPromotionError,
};
use serde_json::json;

#[test]
fn stable_semantic_promotion_requires_two_independent_events() {
    let scope = MemoryScope::new(NamespaceId::new());
    let first = semantic_evidence(&scope, FragmentKind::Fact);
    assert_eq!(
        SemanticEntry::promote(
            scope.clone(),
            SemanticKind::Fact,
            json!({"database": "PostgreSQL"}),
            vec![first.clone()],
        )
        .unwrap_err(),
        SemanticPromotionError::InsufficientIndependentEvents { found: 1 }
    );

    let second = semantic_evidence(&scope, FragmentKind::Fact);
    let entry = SemanticEntry::promote(
        scope,
        SemanticKind::Fact,
        json!({"database": "PostgreSQL"}),
        vec![first.clone(), second.clone()],
    )
    .unwrap();

    assert!(entry.stable);
    assert_eq!(entry.evidence, vec![first, second]);
    assert_eq!(entry.validity, ValidityStatus::Verified);
}

#[test]
fn predictions_cannot_be_promoted_to_semantic_facts() {
    let scope = MemoryScope::new(NamespaceId::new());
    let prediction = semantic_evidence(&scope, FragmentKind::Prediction);
    assert_eq!(
        SemanticEntry::promote(
            scope.clone(),
            SemanticKind::Fact,
            json!("deployment will succeed"),
            vec![
                prediction.clone(),
                semantic_evidence(&scope, FragmentKind::Fact),
            ],
        )
        .unwrap_err(),
        SemanticPromotionError::HypothesisCannotBecomeFact {
            fragment_id: prediction.fragment_id,
            kind: FragmentKind::Prediction,
        }
    );
}

#[test]
fn semantic_promotion_rejects_cross_scope_evidence() {
    let scope = MemoryScope::new(NamespaceId::new());
    let foreign = semantic_evidence(&MemoryScope::new(NamespaceId::new()), FragmentKind::Fact);
    assert_eq!(
        SemanticEntry::promote(
            scope.clone(),
            SemanticKind::Fact,
            json!("scoped fact"),
            vec![
                semantic_evidence(&scope, FragmentKind::Fact),
                foreign.clone()
            ],
        )
        .unwrap_err(),
        SemanticPromotionError::EvidenceScopeMismatch(foreign.event_id)
    );
}

#[test]
fn procedure_success_rate_keeps_sample_count_and_evidence_ids() {
    let scope = MemoryScope::new(NamespaceId::new());
    let first = procedure_evidence(&scope, true);
    let second = procedure_evidence(&scope, false);
    let third = procedure_evidence(&scope, true);
    let mut experience =
        ProcedureExperience::new(scope.clone(), "deploy.api", first.clone()).unwrap();
    experience.record(second.clone()).unwrap();
    experience.record(third.clone()).unwrap();

    assert_eq!(experience.sample_count, 3);
    assert_eq!(experience.success_count, 2);
    assert_eq!(experience.failure_count, 1);
    assert!((experience.success_rate - (2.0 / 3.0)).abs() < f64::EPSILON);
    assert_eq!(experience.evidence, vec![first.clone(), second, third]);

    let before = experience.clone();
    assert_eq!(
        experience.record(first.clone()).unwrap_err(),
        ProcedureExperienceError::DuplicateEvent(first.event_id)
    );
    assert_eq!(experience, before);

    let foreign = procedure_evidence(&MemoryScope::new(NamespaceId::new()), true);
    assert_eq!(
        experience.record(foreign.clone()).unwrap_err(),
        ProcedureExperienceError::EvidenceScopeMismatch(foreign.event_id)
    );
    assert_eq!(experience, before);
}

fn semantic_evidence(scope: &MemoryScope, kind: FragmentKind) -> SemanticEvidence {
    SemanticEvidence {
        scope: scope.clone(),
        event_id: EventId::new(),
        fragment_id: FragmentId::new(),
        kind,
        validity: ValidityStatus::Verified,
        confidence: 0.9,
    }
}

fn procedure_evidence(scope: &MemoryScope, succeeded: bool) -> ProcedureEvidence {
    ProcedureEvidence {
        scope: scope.clone(),
        event_id: EventId::new(),
        fragment_id: FragmentId::new(),
        succeeded,
    }
}
