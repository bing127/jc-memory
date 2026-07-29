//! Evidence-backed semantic and procedural memory for Jiangcheng Memory.

/// Evidence-backed semantic facts, preferences, and constraints.
pub mod knowledge;
/// Evidence-backed procedure outcome statistics.
pub mod procedure;

pub use knowledge::{SemanticEntry, SemanticEvidence, SemanticKind, SemanticPromotionError};
pub use procedure::{ProcedureEvidence, ProcedureExperience, ProcedureExperienceError};
