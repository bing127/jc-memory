//! Stable domain and wire types for Jiangcheng Memory.

/// Strongly typed identifiers used across protocol boundaries.
pub mod ids;
/// Hierarchical memory ownership scope.
pub mod scope;

pub use ids::{
    AgentId, BlobId, EventId, EvidenceId, FragmentId, IdParseError, NamespaceId, ProjectId,
    SessionId, TenantId, TraceId, UserId,
};
pub use scope::MemoryScope;
