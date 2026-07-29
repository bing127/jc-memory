//! Atomic cognitive graph points.

use chrono::{DateTime, Utc};
use jc_memory_protocol::{
    CognitiveFragment, FragmentId, FragmentKind, MemoryScope, PointId, ValidityStatus,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// The atomic cognitive change represented by a point.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PointKind {
    /// A fact whose validity remains explicit.
    Fact,
    /// A decision made at runtime.
    Decision,
    /// A constraint on later decisions or actions.
    Constraint,
    /// An explicit transition between states.
    StateChange,
    /// An action performed by an actor.
    Action,
    /// An observed successful or neutral outcome.
    Result,
    /// An observed failed outcome.
    Failure,
    /// A stated or learned preference.
    Preference,
    /// A correction to earlier memory.
    Correction,
}

/// A graph point derived from one or more authoritative fragments.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct CognitivePoint {
    /// Point identifier.
    pub id: PointId,
    /// Ownership boundary inherited from the source fragment.
    pub scope: MemoryScope,
    /// Atomic change category.
    pub kind: PointKind,
    /// Authoritative fragments supporting this derived point.
    pub source_fragments: Vec<FragmentId>,
    /// When the represented change occurred.
    pub occurred_at: DateTime<Utc>,
    /// Confidence inherited from the source fragment.
    pub confidence: f32,
    /// Current validity inherited from the source fragment.
    pub validity: ValidityStatus,
}

/// An error produced when a fragment cannot form an atomic point.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum PointCreationError {
    /// The fragment kind is not itself an atomic cognitive change.
    #[error("fragment kind {0:?} cannot form a cognitive point")]
    UnsupportedFragmentKind(FragmentKind),
}

impl TryFrom<&CognitiveFragment> for CognitivePoint {
    type Error = PointCreationError;

    fn try_from(fragment: &CognitiveFragment) -> Result<Self, Self::Error> {
        let kind = PointKind::try_from(fragment.kind)?;
        Ok(Self {
            id: PointId::new(),
            scope: fragment.scope.clone(),
            kind,
            source_fragments: vec![fragment.id],
            occurred_at: fragment.occurred_at,
            confidence: fragment.confidence,
            validity: fragment.validity,
        })
    }
}

impl TryFrom<FragmentKind> for PointKind {
    type Error = PointCreationError;

    fn try_from(kind: FragmentKind) -> Result<Self, Self::Error> {
        match kind {
            FragmentKind::Fact => Ok(Self::Fact),
            FragmentKind::Decision => Ok(Self::Decision),
            FragmentKind::Constraint => Ok(Self::Constraint),
            FragmentKind::StateTransition => Ok(Self::StateChange),
            FragmentKind::Action => Ok(Self::Action),
            FragmentKind::Outcome | FragmentKind::Success => Ok(Self::Result),
            FragmentKind::Failure => Ok(Self::Failure),
            FragmentKind::Preference => Ok(Self::Preference),
            FragmentKind::Correction => Ok(Self::Correction),
            FragmentKind::Observation
            | FragmentKind::Claim
            | FragmentKind::Inference
            | FragmentKind::Prediction
            | FragmentKind::ToolCall
            | FragmentKind::ToolResult
            | FragmentKind::Plan
            | FragmentKind::Approval
            | FragmentKind::Rejection => Err(PointCreationError::UnsupportedFragmentKind(kind)),
        }
    }
}
