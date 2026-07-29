//! Immutable event protocol types.

use crate::{BlobId, CognitiveFragment, EventId, FragmentId, MemoryScope};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// The temporal-cognitive stage represented by an event.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CognitiveStage {
    /// Verified evidence from the past.
    PastEvidence,
    /// An observation captured in the present transition.
    CurrentObservation,
    /// An unverified hypothesis about a future result.
    FutureHypothesis,
    /// A selection made by the runtime or an authorized principal.
    RuntimeDecision,
    /// An action requested from or executed by a tool.
    ToolAction,
    /// A result actually returned by the environment.
    ActualResult,
    /// Evidence that validates or disproves an earlier result or prediction.
    ResultVerification,
}

/// An immutable event before its per-scope sequence is assigned transactionally.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct EventDraft {
    /// Event identifier.
    pub id: EventId,
    /// Source fragment identifier.
    pub fragment_id: FragmentId,
    /// Ownership scope copied from the fragment.
    pub scope: MemoryScope,
    /// Cognitive stage.
    pub stage: CognitiveStage,
    /// Time the event draft was recorded.
    pub recorded_at: DateTime<Utc>,
    /// Canonical SHA-256 of the complete fragment payload.
    pub content_hash: BlobId,
    /// Original validated fragment.
    pub payload: CognitiveFragment,
}

/// A persisted event with its transactionally assigned per-scope sequence.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct MemoryEvent {
    /// Immutable event draft.
    pub draft: EventDraft,
    /// Monotonic sequence within the exact memory scope.
    pub sequence: u64,
}
