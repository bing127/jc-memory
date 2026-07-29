//! Fast episodic memory, pattern separation, and versioned state for Jiangcheng Memory.

/// Deterministic write-time pattern separation.
pub mod separation;
/// Versioned, evidence-linked state chains.
pub mod state;

pub use separation::{
    PatternSeparator, SeparationCandidate, SeparationDecision, SeparationInput, SeparationScore,
};
pub use state::{
    StateEvidence, StateMutation, StateRepository, StateRepositoryError, StateResolution,
    StateVersion,
};
