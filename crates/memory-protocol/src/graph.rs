//! Cognitive graph wire types.

use serde::{Deserialize, Serialize};

/// Directed relationship between two cognitive points.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    /// The source occurred before the target.
    TemporalBefore,
    /// The source occurred after the target.
    TemporalAfter,
    /// The source is evidence that it caused the target.
    Causes,
    /// The source was caused by the target.
    CausedBy,
    /// The source supports the target.
    Supports,
    /// The source contradicts the target.
    Contradicts,
    /// The source supersedes the target.
    Supersedes,
    /// The source depends on the target.
    DependsOn,
    /// The source is part of the target.
    PartOf,
    /// Both points describe the same entity.
    SameEntity,
    /// Both points were activated together without asserting causality.
    CoActivated,
    /// The source was predicted by the target.
    PredictedBy,
    /// The source was validated by the target.
    ValidatedBy,
    /// The source was disproved by the target.
    DisprovedBy,
    /// The source was recalled by the target recall record.
    RecalledBy,
    /// The source was used in the target decision.
    UsedInDecision,
    /// The source decision was executed as the target action.
    ExecutedAs,
    /// The source action resulted in the target outcome.
    ResultedIn,
}
