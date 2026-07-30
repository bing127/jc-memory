//! Bounded recall activation and explanation for Jiangcheng Memory.

/// Surface activation and evidence completion.
pub mod completion;
/// Evidence provenance and traversal explanations.
pub mod explain;
/// Recall query and hard budget types.
pub mod query;
/// Deterministic searchable Point index records.
pub mod seeds;
/// Scope-bound bounded graph activation.
pub mod spread;

pub use completion::ActivatedSurface;
pub use explain::{ExplainTrace, SeedTrace, SurfaceActivationTrace, TraversalTrace};
pub use query::{RecallCancellation, RecallLimits, RecallQuery};
pub use seeds::IndexedPoint;
pub use spread::{
    ActivatedPoint, RecallEngine, RecallEngineError, RecallResult, RecallTrace, edge_kinds_for,
};
