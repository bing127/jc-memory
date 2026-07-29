use crate::validation::{FragmentValidationError, validate_fragment};
use jc_memory_protocol::{
    BlobId, CognitiveFragment, CognitiveStage, EventDraft, EventId, FragmentKind, ValidityStatus,
};
use serde_json::Value;
use sha2::{Digest as _, Sha256};
use std::str::FromStr as _;
use thiserror::Error;

/// Failure while constructing an immutable event draft.
#[derive(Debug, Error)]
pub enum EventFactoryError {
    /// Fragment validation failed.
    #[error(transparent)]
    Validation(#[from] FragmentValidationError),
    /// Canonical serialization failed.
    #[error("failed to serialize canonical fragment: {0}")]
    Serialization(#[from] serde_json::Error),
    /// The computed digest could not be represented as a `BlobId`.
    #[error("computed an invalid content digest: {0}")]
    Digest(#[from] jc_memory_protocol::IdParseError),
}

/// Constructs validated immutable event drafts.
#[derive(Clone, Copy, Debug, Default)]
pub struct EventFactory;

impl EventFactory {
    /// Creates an event factory.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Validates a fragment and constructs its immutable draft.
    ///
    /// # Errors
    /// Returns an error when validation, serialization, or digest conversion fails.
    pub fn from_fragment(
        &self,
        fragment: CognitiveFragment,
    ) -> Result<EventDraft, EventFactoryError> {
        validate_fragment(&fragment)?;
        let canonical = canonicalize(serde_json::to_value(&fragment)?);
        let digest = hex_digest(&serde_json::to_vec(&canonical)?)?;
        Ok(EventDraft {
            id: EventId::new(),
            fragment_id: fragment.id,
            scope: fragment.scope.clone(),
            stage: stage_for(&fragment),
            recorded_at: chrono::Utc::now(),
            content_hash: digest,
            payload: fragment,
        })
    }
}

fn hex_digest(bytes: &[u8]) -> Result<BlobId, jc_memory_protocol::IdParseError> {
    BlobId::from_str(&format!("{:x}", Sha256::digest(bytes)))
}

fn canonicalize(value: Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.into_iter().map(canonicalize).collect()),
        Value::Object(values) => {
            let mut entries: Vec<_> = values.into_iter().collect();
            entries.sort_by(|left, right| left.0.cmp(&right.0));
            Value::Object(
                entries
                    .into_iter()
                    .map(|(key, value)| (key, canonicalize(value)))
                    .collect(),
            )
        }
        scalar => scalar,
    }
}

fn stage_for(fragment: &CognitiveFragment) -> CognitiveStage {
    match fragment.kind {
        FragmentKind::Inference | FragmentKind::Prediction | FragmentKind::Plan => {
            CognitiveStage::FutureHypothesis
        }
        FragmentKind::Decision | FragmentKind::Approval | FragmentKind::Rejection => {
            CognitiveStage::RuntimeDecision
        }
        FragmentKind::Action | FragmentKind::ToolCall => CognitiveStage::ToolAction,
        FragmentKind::ToolResult
        | FragmentKind::Outcome
        | FragmentKind::Failure
        | FragmentKind::Success => CognitiveStage::ActualResult,
        FragmentKind::StateTransition => CognitiveStage::ResultVerification,
        FragmentKind::Fact
        | FragmentKind::Correction
        | FragmentKind::Constraint
        | FragmentKind::Preference
            if fragment.validity == ValidityStatus::Verified =>
        {
            CognitiveStage::PastEvidence
        }
        FragmentKind::Observation
        | FragmentKind::Claim
        | FragmentKind::Fact
        | FragmentKind::Correction
        | FragmentKind::Constraint
        | FragmentKind::Preference => CognitiveStage::CurrentObservation,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use jc_memory_protocol::{
        CognitiveFragment, FragmentContent, FragmentId, FragmentKind, FragmentSource, MemoryScope,
        NamespaceId, Perspective, SecurityLabel, SourceType, ValidityStatus,
    };

    fn fixture(kind: FragmentKind) -> CognitiveFragment {
        let now = Utc::now();
        CognitiveFragment {
            id: FragmentId::new(),
            scope: MemoryScope::new(NamespaceId::new()),
            session_id: None,
            trace_id: None,
            kind,
            content: FragmentContent::Text {
                text: "evidence".into(),
            },
            source: FragmentSource {
                source_type: SourceType::Tool,
                source_id: "fixture".into(),
                actor_id: None,
                observer_id: None,
                tool_id: Some("fixture".into()),
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
            metadata: serde_json::Map::new(),
        }
    }

    #[test]
    fn same_fragment_produces_same_canonical_content_hash() {
        let fragment = fixture(FragmentKind::Observation);
        let first = EventFactory::new().from_fragment(fragment.clone()).unwrap();
        let second = EventFactory::new().from_fragment(fragment).unwrap();
        assert_eq!(first.content_hash, second.content_hash);
        assert_ne!(first.id, second.id);
    }

    #[test]
    fn prediction_keeps_future_hypothesis_stage() {
        let event = EventFactory::new()
            .from_fragment(fixture(FragmentKind::Prediction))
            .unwrap();
        assert_eq!(
            event.stage,
            jc_memory_protocol::CognitiveStage::FutureHypothesis
        );
    }
}
