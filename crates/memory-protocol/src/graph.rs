//! Cognitive graph wire types.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::fragment::FragmentKind;

/// Atomic cognitive change represented by a graph point.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PointKind {
    /// Evidence-backed fact.
    Fact,
    /// Runtime or user decision.
    Decision,
    /// Constraint on later behavior.
    Constraint,
    /// Explicit state transition.
    StateChange,
    /// Action performed by an actor.
    Action,
    /// Observed successful or neutral result.
    Result,
    /// Observed failed result.
    Failure,
    /// Stated or learned preference.
    Preference,
    /// Correction to earlier memory.
    Correction,
}

/// An error produced when a Fragment cannot form an atomic Point kind.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum PointCreationError {
    /// The Fragment kind is not itself an atomic cognitive change.
    #[error("fragment kind {0:?} cannot form a cognitive point")]
    UnsupportedFragmentKind(FragmentKind),
}

impl TryFrom<FragmentKind> for PointKind {
    type Error = PointCreationError;

    fn try_from(kind: FragmentKind) -> Result<Self, Self::Error> {
        match kind {
            FragmentKind::Fact => Ok(Self::Fact),
            FragmentKind::Decision => Ok(Self::Decision),
            FragmentKind::Constraint => Ok(Self::Constraint),
            FragmentKind::StateTransition => Ok(Self::StateChange),
            FragmentKind::Action => Ok(Self::Action),
            FragmentKind::Outcome | FragmentKind::Success => Ok(Self::Result),
            FragmentKind::Failure => Ok(Self::Failure),
            FragmentKind::Preference => Ok(Self::Preference),
            FragmentKind::Correction => Ok(Self::Correction),
            FragmentKind::Observation
            | FragmentKind::Claim
            | FragmentKind::Inference
            | FragmentKind::Prediction
            | FragmentKind::ToolCall
            | FragmentKind::ToolResult
            | FragmentKind::Plan
            | FragmentKind::Approval
            | FragmentKind::Rejection => Err(PointCreationError::UnsupportedFragmentKind(kind)),
        }
    }
}

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
