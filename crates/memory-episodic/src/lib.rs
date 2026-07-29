//! Fast episodic memory, pattern separation, and versioned state for Jiangcheng Memory.

/// Deterministic write-time pattern separation.
pub mod separation;

pub use separation::{
    PatternSeparator, SeparationCandidate, SeparationDecision, SeparationInput, SeparationScore,
};
