//! Cognitive fragment domain types.

use crate::{BlobId, EventId, EvidenceId, FragmentId, MemoryScope, SessionId, TraceId};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Arbitrary structured metadata attached to a fragment.
pub type Metadata = serde_json::Map<String, serde_json::Value>;

/// 调用方显式声明的结构化状态。
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct StateDeclaration {
    /// 在精确 Scope 内稳定的状态键。
    pub key: String,
    /// 不经自然语言推断的状态值。
    pub value: serde_json::Value,
    /// 解释本次状态声明原因的可审计文本。
    pub reason: String,
}

/// The semantic role of a cognitive fragment.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FragmentKind {
    /// A direct observation of the current environment.
    Observation,
    /// A statement made by a source without implicit verification.
    Claim,
    /// A derived interpretation that is not a verified fact.
    Inference,
    /// A statement about a possible future result.
    Prediction,
    /// A runtime-selected decision.
    Decision,
    /// An action taken by an actor.
    Action,
    /// A request sent to a tool.
    ToolCall,
    /// Data returned by a tool.
    ToolResult,
    /// The observed outcome of an action.
    Outcome,
    /// A correction to earlier memory.
    Correction,
    /// A constraint that applies to decisions or actions.
    Constraint,
    /// A stated or learned preference.
    Preference,
    /// A fact whose validity is represented separately.
    Fact,
    /// A failed result.
    Failure,
    /// A successful result.
    Success,
    /// A proposed sequence of actions.
    Plan,
    /// An approval granted by an authorized principal.
    Approval,
    /// A rejection by an authorized principal.
    Rejection,
    /// A transition between explicit states.
    StateTransition,
}

/// Content carried by a cognitive fragment.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum FragmentContent {
    /// UTF-8 text stored inline.
    Text {
        /// Original text without generated replacement.
        text: String,
    },
    /// Structured JSON stored inline.
    Json {
        /// Original JSON value.
        value: serde_json::Value,
    },
    /// Content stored in the content-addressed `BlobStore`.
    Blob {
        /// Plaintext SHA-256 blob identifier.
        blob_id: BlobId,
        /// IANA media type.
        media_type: String,
        /// Original plaintext byte length.
        byte_len: u64,
    },
}

/// The source category that produced a fragment.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceType {
    /// A human user.
    User,
    /// An agent runtime.
    Agent,
    /// An external or local language model.
    Model,
    /// A tool invocation.
    Tool,
    /// Imported external data.
    Import,
    /// An internal system process.
    System,
}

/// Provenance describing who produced and observed a fragment.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FragmentSource {
    /// Source category.
    pub source_type: SourceType,
    /// Stable identifier within the source system.
    pub source_id: String,
    /// Actor that caused the event, when known.
    pub actor_id: Option<String>,
    /// Observer that captured the event, when known.
    pub observer_id: Option<String>,
    /// Tool identifier, when the source is a tool.
    pub tool_id: Option<String>,
    /// Original source URI, when one exists.
    pub uri: Option<String>,
}

/// Perspective from which content was captured.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Perspective {
    /// First-person statement.
    FirstPerson,
    /// Second-person statement.
    SecondPerson,
    /// Third-person statement.
    ThirdPerson,
    /// System-level observation.
    System,
    /// Tool-level observation.
    Tool,
}

/// Current validity assigned to a fragment.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidityStatus {
    /// Not yet verified.
    Unverified,
    /// Verified by explicit evidence.
    Verified,
    /// Disputed by other evidence.
    Disputed,
    /// Replaced by a newer version.
    Superseded,
    /// Explicitly invalidated.
    Invalidated,
    /// Isolated from normal recall pending review.
    Quarantined,
}

/// Coarse confidentiality classification.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SecurityLabel {
    /// Publicly shareable data.
    Public,
    /// Internal data.
    Internal,
    /// Confidential data.
    Confidential,
    /// Highly restricted data.
    Restricted,
}

/// Method used to verify supporting evidence.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationMethod {
    /// Verified by an authorized human.
    Manual,
    /// Verified from an actual tool result.
    Tool,
    /// Verified against an external authoritative source.
    ExternalSource,
    /// Verified by multiple independent evidence items.
    CrossEvidence,
}

/// A typed reference to one supporting fragment or event.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EvidenceRef {
    /// Identifier of this evidence reference.
    pub id: EvidenceId,
    /// Referenced fragment, mutually exclusive with `event_id`.
    pub fragment_id: Option<FragmentId>,
    /// Referenced event, mutually exclusive with `fragment_id`.
    pub event_id: Option<EventId>,
    /// Verification method.
    pub method: VerificationMethod,
    /// Optional human-readable verification note.
    pub note: Option<String>,
}

/// The immutable input unit accepted by Jiangcheng Memory.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct CognitiveFragment {
    /// Fragment identifier.
    pub id: FragmentId,
    /// Hierarchical ownership scope.
    pub scope: MemoryScope,
    /// Optional session identifier.
    pub session_id: Option<SessionId>,
    /// Optional distributed trace identifier.
    pub trace_id: Option<TraceId>,
    /// Semantic role.
    pub kind: FragmentKind,
    /// Original content or Blob reference.
    pub content: FragmentContent,
    /// Source provenance.
    pub source: FragmentSource,
    /// Perspective of the source.
    pub perspective: Perspective,
    /// When the event occurred in the source system.
    pub occurred_at: DateTime<Utc>,
    /// When Jiangcheng Memory observed the event.
    pub observed_at: DateTime<Utc>,
    /// Source confidence in the unit interval.
    pub confidence: f32,
    /// Importance in the unit interval.
    pub importance: f32,
    /// Current validity.
    pub validity: ValidityStatus,
    /// Confidentiality label.
    pub security_label: SecurityLabel,
    /// Evidence supporting validity.
    pub evidence: Vec<EvidenceRef>,
    /// Structured metadata that does not replace original content.
    pub metadata: Metadata,
}
