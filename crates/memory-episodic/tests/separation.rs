//! Deterministic pattern-separation behavior.

use std::str::FromStr as _;

use chrono::{Duration, TimeZone as _, Utc};
use jc_memory_episodic::{
    PatternSeparator, SeparationCandidate, SeparationDecision, SeparationInput,
};
use jc_memory_graph::CognitivePoint;
use jc_memory_protocol::{
    CognitiveFragment, FragmentContent, FragmentId, FragmentKind, FragmentSource, MemoryScope,
    NamespaceId, Perspective, PointId, ProjectId, SecurityLabel, SourceType, ValidityStatus,
};

#[test]
fn repeated_write_is_an_exact_duplicate() {
    let fragment = fixture("deployment failed", scope_with_project(ProjectId::new()));
    let existing = candidate(fragment.clone(), &["service:api"], Some("failed"));
    let mut repeated = fragment;
    repeated.id = FragmentId::new();
    let input = input(repeated, &["service:api"], Some("failed"));

    assert_eq!(
        PatternSeparator::default().decide(&input, std::slice::from_ref(&existing)),
        SeparationDecision::ExactDuplicate(existing.point.id)
    );
}

#[test]
fn same_text_in_different_projects_stays_distinct() {
    let alpha = scope_with_project(ProjectId::new());
    let mut beta = alpha.clone();
    beta.project_id = Some(ProjectId::new());
    let input = input(
        fixture("deployment failed", alpha),
        &["service:api"],
        Some("failed"),
    );
    let existing = candidate(
        fixture("deployment failed", beta),
        &["service:api"],
        Some("failed"),
    );

    assert_eq!(
        PatternSeparator::default().decide(&input, &[existing]),
        SeparationDecision::CreateDistinct
    );
}

#[test]
fn same_name_with_different_entity_identity_only_links_as_similar() {
    let fragment = fixture(
        "Atlas deployment failed",
        scope_with_project(ProjectId::new()),
    );
    let existing = candidate(fragment.clone(), &["service:atlas-primary"], Some("failed"));
    let input = input(fragment, &["service:atlas-canary"], Some("failed"));

    assert_eq!(
        PatternSeparator::default().decide(&input, std::slice::from_ref(&existing)),
        SeparationDecision::LinkSimilar(existing.point.id)
    );
}

#[test]
fn a_different_time_bucket_is_not_an_exact_duplicate() {
    let fragment = fixture("deployment failed", scope_with_project(ProjectId::new()));
    let mut later = fragment.clone();
    later.id = FragmentId::new();
    later.occurred_at += Duration::minutes(6);
    later.observed_at = later.occurred_at;
    let existing = candidate(fragment, &["service:api"], Some("failed"));

    assert_eq!(
        PatternSeparator::default().decide(
            &input(later, &["service:api"], Some("failed")),
            std::slice::from_ref(&existing)
        ),
        SeparationDecision::LinkSimilar(existing.point.id)
    );
}

#[test]
fn different_source_or_state_is_not_an_exact_duplicate() {
    let fragment = fixture("deployment failed", scope_with_project(ProjectId::new()));
    let source_candidate = candidate(fragment.clone(), &["service:api"], Some("failed"));
    let state_candidate = source_candidate.clone();

    let mut other_source = fragment.clone();
    other_source.id = FragmentId::new();
    other_source.source.source_id = "manual-report".to_owned();
    assert_eq!(
        PatternSeparator::default().decide(
            &input(other_source, &["service:api"], Some("failed")),
            std::slice::from_ref(&source_candidate)
        ),
        SeparationDecision::LinkSimilar(source_candidate.point.id)
    );

    assert_eq!(
        PatternSeparator::default().decide(
            &input(fragment, &["service:api"], Some("recovering")),
            std::slice::from_ref(&state_candidate)
        ),
        SeparationDecision::LinkSimilar(state_candidate.point.id)
    );
}

#[test]
fn nearby_content_links_but_unrelated_content_stays_distinct() {
    let scope = scope_with_project(ProjectId::new());
    let existing = candidate(
        fixture("production deployment failed", scope.clone()),
        &["service:api"],
        Some("failed"),
    );

    let nearby = input(
        fixture("deployment failed in production", scope.clone()),
        &["service:api"],
        Some("failed"),
    );
    assert_eq!(
        PatternSeparator::default().decide(&nearby, std::slice::from_ref(&existing)),
        SeparationDecision::LinkSimilar(existing.point.id)
    );

    let unrelated = input(
        fixture("customer approved the invoice", scope),
        &["service:api"],
        Some("failed"),
    );
    assert_eq!(
        PatternSeparator::default().decide(&unrelated, &[existing]),
        SeparationDecision::CreateDistinct
    );
}

#[test]
fn score_exposes_all_six_dimensions_and_ties_are_stable() {
    let fragment = fixture("deployment failed", scope_with_project(ProjectId::new()));
    let input = input(fragment.clone(), &["service:api"], Some("failed"));
    let mut higher_id = candidate(fragment.clone(), &["service:api"], Some("recovering"));
    higher_id.point.id = PointId::from_str("ffffffff-ffff-7fff-bfff-ffffffffffff").unwrap();
    let mut lower_id = candidate(fragment, &["service:api"], Some("recovering"));
    lower_id.point.id = PointId::from_str("00000000-0000-7000-8000-000000000001").unwrap();

    let score = PatternSeparator::default().score(&input, &lower_id);
    assert_eq!(score.content.to_bits(), 1.0_f32.to_bits());
    assert_eq!(score.entity.to_bits(), 1.0_f32.to_bits());
    assert_eq!(score.temporal.to_bits(), 1.0_f32.to_bits());
    assert_eq!(score.project.to_bits(), 1.0_f32.to_bits());
    assert_eq!(score.source.to_bits(), 1.0_f32.to_bits());
    assert_eq!(score.state.to_bits(), 0.0_f32.to_bits());
    assert_eq!(
        PatternSeparator::default().decide(&input, &[higher_id, lower_id.clone()]),
        SeparationDecision::LinkSimilar(lower_id.point.id)
    );
}

fn input(
    fragment: CognitiveFragment,
    entity_keys: &[&str],
    state_signature: Option<&str>,
) -> SeparationInput {
    SeparationInput {
        fragment,
        entity_keys: entity_keys
            .iter()
            .map(|value| (*value).to_owned())
            .collect(),
        state_signature: state_signature.map(str::to_owned),
    }
}

fn candidate(
    fragment: CognitiveFragment,
    entity_keys: &[&str],
    state_signature: Option<&str>,
) -> SeparationCandidate {
    let point = CognitivePoint::try_from(&fragment).unwrap();
    SeparationCandidate {
        point,
        input: input(fragment, entity_keys, state_signature),
    }
}

fn scope_with_project(project_id: ProjectId) -> MemoryScope {
    let mut scope = MemoryScope::new(NamespaceId::new());
    scope.project_id = Some(project_id);
    scope
}

fn fixture(text: &str, scope: MemoryScope) -> CognitiveFragment {
    let occurred_at = Utc.with_ymd_and_hms(2026, 7, 29, 12, 1, 0).unwrap();
    CognitiveFragment {
        id: FragmentId::new(),
        scope,
        session_id: None,
        trace_id: None,
        kind: FragmentKind::Failure,
        content: FragmentContent::Text {
            text: text.to_owned(),
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
