//! Validation and immutable event construction for Jiangcheng Memory.

/// Immutable event factory.
pub mod factory;
/// Cognitive fragment validation.
pub mod validation;

pub use validation::{FragmentValidationError, validate_fragment, validate_state_metadata};
