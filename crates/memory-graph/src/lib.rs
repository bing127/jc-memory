//! Cognitive points, edges, and bounded local graphs for Jiangcheng Memory.

/// Atomic cognitive graph points derived from authoritative fragments.
pub mod point;

pub use point::{CognitivePoint, PointCreationError, PointKind};
