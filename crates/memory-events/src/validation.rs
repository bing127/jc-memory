use jc_memory_protocol::{CognitiveFragment, FragmentContent, FragmentKind, ValidityStatus};
use thiserror::Error;

const MAX_CLOCK_SKEW: chrono::Duration = chrono::Duration::minutes(5);

/// A violated cognitive fragment invariant.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum FragmentValidationError {
    /// A unit-interval field is non-finite or outside `0.0..=1.0`.
    #[error("{field} must be finite and between 0.0 and 1.0")]
    UnitInterval {
        /// Name of the invalid field.
        field: &'static str,
    },
    /// Observation time is too far before occurrence time.
    #[error("observed_at is earlier than occurred_at beyond the allowed clock skew")]
    ObservedBeforeOccurred,
    /// A verified fragment has no explicit evidence.
    #[error("a verified fragment requires evidence")]
    VerifiedWithoutEvidence,
    /// A model prediction cannot enter memory as a verified fact.
    #[error("a prediction cannot be marked verified")]
    PredictionCannotBeVerified,
    /// Text content is empty or whitespace-only.
    #[error("text content cannot be empty")]
    EmptyText,
    /// Blob content has an empty media type.
    #[error("blob media_type cannot be empty")]
    EmptyMediaType,
    /// Blob content declares zero bytes.
    #[error("blob byte_len must be greater than zero")]
    EmptyBlob,
    /// Source identifier is empty.
    #[error("source_id cannot be empty")]
    EmptySourceId,
    /// An evidence reference does not have exactly one target.
    #[error("evidence must reference exactly one fragment or event")]
    EvidenceTargetCount,
}

/// Validates all invariants required before a fragment can become an event.
///
/// # Errors
///
/// Returns [`FragmentValidationError`] when content, provenance, time, evidence,
/// confidence, importance, or prediction validity violates an invariant.
pub fn validate_fragment(fragment: &CognitiveFragment) -> Result<(), FragmentValidationError> {
    validate_unit_interval("confidence", fragment.confidence)?;
    validate_unit_interval("importance", fragment.importance)?;
    validate_content(&fragment.content)?;

    if fragment.source.source_id.trim().is_empty() {
        return Err(FragmentValidationError::EmptySourceId);
    }
    if fragment.observed_at + MAX_CLOCK_SKEW < fragment.occurred_at {
        return Err(FragmentValidationError::ObservedBeforeOccurred);
    }
    if fragment.kind == FragmentKind::Prediction && fragment.validity == ValidityStatus::Verified {
        return Err(FragmentValidationError::PredictionCannotBeVerified);
    }
    if fragment.validity == ValidityStatus::Verified && fragment.evidence.is_empty() {
        return Err(FragmentValidationError::VerifiedWithoutEvidence);
    }
    if fragment
        .evidence
        .iter()
        .any(|evidence| evidence.fragment_id.is_some() == evidence.event_id.is_some())
    {
        return Err(FragmentValidationError::EvidenceTargetCount);
    }
    Ok(())
}

fn validate_unit_interval(field: &'static str, value: f32) -> Result<(), FragmentValidationError> {
    if value.is_finite() && (0.0..=1.0).contains(&value) {
        Ok(())
    } else {
        Err(FragmentValidationError::UnitInterval { field })
    }
}

fn validate_content(content: &FragmentContent) -> Result<(), FragmentValidationError> {
    match content {
        FragmentContent::Text { text } if text.trim().is_empty() => {
            Err(FragmentValidationError::EmptyText)
        }
        FragmentContent::Blob { media_type, .. } if media_type.trim().is_empty() => {
            Err(FragmentValidationError::EmptyMediaType)
        }
        FragmentContent::Blob { byte_len: 0, .. } => Err(FragmentValidationError::EmptyBlob),
        FragmentContent::Text { .. }
        | FragmentContent::Json { .. }
        | FragmentContent::Blob { .. } => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Utc};
    use jc_memory_protocol::fragment::{
        CognitiveFragment, EvidenceRef, FragmentContent, FragmentKind, FragmentSource, Perspective,
        SecurityLabel, SourceType, ValidityStatus, VerificationMethod,
    };
    use jc_memory_protocol::{EvidenceId, FragmentId, MemoryScope, NamespaceId};
    use serde_json::Map;

    fn evidence() -> EvidenceRef {
        EvidenceRef {
            id: EvidenceId::new(),
            fragment_id: Some(FragmentId::new()),
            event_id: None,
            method: VerificationMethod::Tool,
            note: Some("verified by deterministic fixture".to_owned()),
        }
    }

    fn fixture(kind: FragmentKind) -> CognitiveFragment {
        let now = Utc::now();
        CognitiveFragment {
            id: FragmentId::new(),
            scope: MemoryScope::new(NamespaceId::new()),
            session_id: None,
            trace_id: None,
            kind,
            content: FragmentContent::Text {
                text: "observed evidence".to_owned(),
            },
            source: FragmentSource {
                source_type: SourceType::Tool,
                source_id: "fixture-tool".to_owned(),
                actor_id: None,
                observer_id: Some("test-observer".to_owned()),
                tool_id: Some("fixture-tool".to_owned()),
                uri: None,
            },
            perspective: Perspective::Tool,
            occurred_at: now,
            observed_at: now,
            confidence: 0.8,
            importance: 0.5,
            validity: ValidityStatus::Unverified,
            security_label: SecurityLabel::Internal,
            evidence: Vec::new(),
            metadata: Map::new(),
        }
    }

    #[test]
    fn prediction_cannot_enter_as_verified_fact() {
        let mut fragment = fixture(FragmentKind::Prediction);
        fragment.validity = ValidityStatus::Verified;
        fragment.evidence.push(evidence());
        assert_eq!(
            validate_fragment(&fragment).unwrap_err(),
            FragmentValidationError::PredictionCannotBeVerified
        );
    }

    #[test]
    fn verified_fragment_requires_evidence() {
        let mut fragment = fixture(FragmentKind::Fact);
        fragment.validity = ValidityStatus::Verified;
        assert_eq!(
            validate_fragment(&fragment).unwrap_err(),
            FragmentValidationError::VerifiedWithoutEvidence
        );
    }

    #[test]
    fn confidence_and_importance_are_finite_unit_intervals() {
        let mut fragment = fixture(FragmentKind::Observation);
        for invalid in [-0.1, 1.1, f32::NAN, f32::INFINITY] {
            fragment.confidence = invalid;
            assert!(matches!(
                validate_fragment(&fragment),
                Err(FragmentValidationError::UnitInterval {
                    field: "confidence"
                })
            ));
        }
        fragment.confidence = 0.5;
        fragment.importance = f32::NEG_INFINITY;
        assert!(matches!(
            validate_fragment(&fragment),
            Err(FragmentValidationError::UnitInterval {
                field: "importance"
            })
        ));
    }

    #[test]
    fn observation_time_allows_only_small_clock_skew() {
        let mut fragment = fixture(FragmentKind::Observation);
        fragment.occurred_at = fragment.observed_at + Duration::minutes(6);
        assert_eq!(
            validate_fragment(&fragment).unwrap_err(),
            FragmentValidationError::ObservedBeforeOccurred
        );
    }

    #[test]
    fn content_and_source_must_be_meaningful() {
        let mut fragment = fixture(FragmentKind::Observation);
        fragment.content = FragmentContent::Text {
            text: "   ".to_owned(),
        };
        assert_eq!(
            validate_fragment(&fragment).unwrap_err(),
            FragmentValidationError::EmptyText
        );

        fragment.content = FragmentContent::Text {
            text: "valid".to_owned(),
        };
        fragment.source.source_id.clear();
        assert_eq!(
            validate_fragment(&fragment).unwrap_err(),
            FragmentValidationError::EmptySourceId
        );
    }

    #[test]
    fn evidence_references_exactly_one_target() {
        let mut fragment = fixture(FragmentKind::Fact);
        let mut invalid = evidence();
        invalid.event_id = Some(jc_memory_protocol::EventId::new());
        fragment.evidence.push(invalid);
        assert_eq!(
            validate_fragment(&fragment).unwrap_err(),
            FragmentValidationError::EvidenceTargetCount
        );
    }
}
