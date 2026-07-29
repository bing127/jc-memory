//! Recall query modes shared by embedded and remote boundaries.

use serde::{Deserialize, Serialize};

/// Strategy used to select seeds and traverse bounded memory context.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecallMode {
    /// Finds exact identifiers or literal content matches.
    Exact,
    /// Finds semantically similar memory.
    Semantic,
    /// Traverses temporal relationships.
    Temporal,
    /// Traverses evidence-backed causal relationships.
    Causal,
    /// Finds memory associated with identified entities.
    Entity,
    /// Expands through bounded associative relationships.
    Associative,
    /// Finds relevant procedure experience.
    Procedural,
    /// Resolves the current effective state.
    CurrentState,
    /// Resolves state at a requested historical time.
    HistoricalState,
    /// Reconstructs a deeper evidence subgraph within explicit limits.
    DeepReconstruction,
}
