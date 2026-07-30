//! Cognitive points, edges, and bounded local graphs for Jiangcheng Memory.

/// Evidence-linked directed cognitive edges.
pub mod edge;
/// Bounded local graph result types and edge filters.
pub mod local_graph;
/// Atomic cognitive graph points derived from authoritative fragments.
pub mod point;
/// Deterministic local graph storage and loading.
pub mod repository;

pub use edge::{
    CognitiveEdge, EdgeCandidate, EdgeCreationError, EdgeUpdate, EdgeUpdateError, EdgeUpdater,
    EvidenceLink, LearningOutcome,
};
pub use jc_memory_protocol::{PointCreationError, PointKind};
pub use local_graph::{EdgeFilter, LocalGraph};
pub use point::CognitivePoint;
pub use repository::{GraphRepositoryError, LocalGraphRepository};
