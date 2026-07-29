//! Stable domain and wire types for Jiangcheng Memory.

/// Immutable event protocol types.
pub mod event;
/// Cognitive fragment domain types.
pub mod fragment;
/// Strongly typed identifiers used across protocol boundaries.
pub mod ids;
/// Hierarchical memory ownership scope.
pub mod scope;

pub use event::{CognitiveStage, EventDraft, MemoryEvent};
pub use fragment::{
    CognitiveFragment, EvidenceRef, FragmentContent, FragmentKind, FragmentSource, Metadata,
    Perspective, SecurityLabel, SourceType, ValidityStatus, VerificationMethod,
};
pub use ids::{
    AgentId, BlobId, EventId, EvidenceId, FragmentId, IdParseError, NamespaceId, ProjectId,
    SessionId, TenantId, TraceId, UserId,
};
pub use scope::MemoryScope;
