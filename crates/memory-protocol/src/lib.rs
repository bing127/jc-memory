//! Stable domain and wire types for Jiangcheng Memory.

/// Immutable event protocol types.
pub mod event;
/// Cognitive fragment domain types.
pub mod fragment;
/// Cognitive graph protocol types.
pub mod graph;
/// Strongly typed identifiers used across protocol boundaries.
pub mod ids;
/// Model-independent Memory Packet wire types.
pub mod packet;
/// Recall request protocol types.
pub mod recall;
/// Hierarchical memory ownership scope.
pub mod scope;
/// Cognitive surface protocol types.
pub mod surface;

pub use event::{CognitiveStage, EventDraft, MemoryEvent};
pub use fragment::{
    CognitiveFragment, EvidenceRef, FragmentContent, FragmentKind, FragmentSource, Metadata,
    Perspective, SecurityLabel, SourceType, ValidityStatus, VerificationMethod,
};
pub use graph::{EdgeKind, PointCreationError, PointKind};
pub use ids::{
    AgentId, BlobId, EdgeId, EventId, EvidenceId, FragmentId, IdParseError, NamespaceId, PointId,
    ProjectId, RecallId, SessionId, StateId, StateVersionId, SurfaceId, TenantId, TraceId, UserId,
};
pub use packet::{
    ConflictView, Coverage, EpisodeView, EvidenceView, KnowledgeKind, KnowledgeView, MemoryPacket,
    MissingEvidence, PointView, ProcedureView, RecallPath, StateView, SurfaceView,
};
pub use recall::RecallMode;
pub use scope::MemoryScope;
pub use surface::{SurfaceKind, SurfaceStatus};
