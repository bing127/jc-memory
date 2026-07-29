//! Evidence-linked directed cognitive edges and bounded online updates.

use chrono::{DateTime, Utc};
use jc_memory_protocol::{EdgeId, EdgeKind, EventId, EvidenceId, FragmentId, MemoryScope, PointId};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::CognitivePoint;

const CO_ACTIVATED_WEIGHT: f32 = 0.1;
const CO_ACTIVATED_CONFIDENCE: f32 = 0.25;
const DEFAULT_EDGE_WEIGHT: f32 = 0.5;

/// A traceable reference from a derived edge to authoritative evidence.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EvidenceLink {
    /// Fragment that supports the edge.
    pub fragment_id: FragmentId,
    /// Event envelope containing the fragment, when already persisted.
    pub event_id: Option<EventId>,
    /// Existing evidence-reference record, when the edge derives from one.
    pub evidence_ref_id: Option<EvidenceId>,
}

/// A directed, evidence-linked relationship between two cognitive points.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct CognitiveEdge {
    /// Edge identifier.
    pub id: EdgeId,
    /// Exact ownership scope shared by both endpoint points.
    pub scope: MemoryScope,
    /// Directed source point.
    pub source_point_id: PointId,
    /// Directed target point.
    pub target_point_id: PointId,
    /// Relationship semantics.
    pub kind: EdgeKind,
    /// Learned connection weight in `0.0..=1.0`.
    pub weight: f32,
    /// Confidence in the relationship in `0.0..=1.0`.
    pub confidence: f32,
    /// Authoritative evidence supporting creation and later updates.
    pub evidence: Vec<EvidenceLink>,
    /// Beginning of the relationship's validity interval.
    pub valid_from: DateTime<Utc>,
    /// End of the relationship's validity interval, when closed.
    pub valid_to: Option<DateTime<Utc>>,
    /// Version of the model that proposed the edge, when applicable.
    pub model_version_id: Option<String>,
    /// Whether an authorized human confirmed the relationship.
    pub human_confirmed: bool,
    /// Whether a tool result verified the relationship.
    pub tool_verified: bool,
}

/// A derived edge before a repository accepts it.
pub type EdgeCandidate = CognitiveEdge;

/// An error produced while constructing an edge.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum EdgeCreationError {
    /// Endpoint points do not share the exact same scope.
    #[error("edge endpoints must belong to the same scope")]
    ScopeMismatch,
    /// An edge cannot connect a point to itself.
    #[error("edge cannot link point {0} to itself")]
    SelfLink(PointId),
}

impl CognitiveEdge {
    /// Creates an unverified directed edge supported by one authoritative evidence link.
    ///
    /// # Errors
    ///
    /// Returns [`EdgeCreationError`] when endpoints differ in scope or identity.
    pub fn new(
        source: &CognitivePoint,
        target: &CognitivePoint,
        kind: EdgeKind,
        evidence: EvidenceLink,
    ) -> Result<Self, EdgeCreationError> {
        validate_endpoints(source, target)?;
        Ok(Self {
            id: EdgeId::new(),
            scope: source.scope.clone(),
            source_point_id: source.id,
            target_point_id: target.id,
            kind,
            weight: DEFAULT_EDGE_WEIGHT,
            confidence: source.confidence.min(target.confidence),
            evidence: vec![evidence],
            valid_from: source.occurred_at.max(target.occurred_at),
            valid_to: None,
            model_version_id: None,
            human_confirmed: false,
            tool_verified: false,
        })
    }

    /// Creates a low-confidence co-activation candidate without asserting causality.
    ///
    /// # Errors
    ///
    /// Returns [`EdgeCreationError`] when endpoints differ in scope or identity.
    pub fn co_activated(
        source: &CognitivePoint,
        target: &CognitivePoint,
        evidence: EvidenceLink,
    ) -> Result<Self, EdgeCreationError> {
        let mut edge = Self::new(source, target, EdgeKind::CoActivated, evidence)?;
        edge.weight = CO_ACTIVATED_WEIGHT;
        edge.confidence = edge.confidence.min(CO_ACTIVATED_CONFIDENCE);
        Ok(edge)
    }
}

fn validate_endpoints(
    source: &CognitivePoint,
    target: &CognitivePoint,
) -> Result<(), EdgeCreationError> {
    if source.id == target.id {
        return Err(EdgeCreationError::SelfLink(source.id));
    }
    if source.scope != target.scope {
        return Err(EdgeCreationError::ScopeMismatch);
    }
    Ok(())
}

/// Observed result used to strengthen or weaken an edge.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LearningOutcome {
    /// No success or failure signal; only co-activation contributes.
    Neutral,
    /// A successful result with its authoritative evidence.
    Success(EvidenceLink),
    /// A failed result with its authoritative evidence.
    Failure(EvidenceLink),
}

/// Auditable result of one bounded edge-weight update.
#[derive(Clone, Debug, PartialEq)]
pub struct EdgeUpdate {
    /// Weight before applying the update.
    pub previous_weight: f32,
    /// Weight after applying the formula and clipping.
    pub new_weight: f32,
    /// Contribution from joint activation.
    pub activation_delta: f32,
    /// Contribution from the success or failure outcome.
    pub outcome_delta: f32,
    /// Evidence appended for a success or failure update.
    pub evidence: Option<EvidenceLink>,
}

/// An invalid online edge-update parameter.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum EdgeUpdateError {
    /// A learning rate is negative or not finite.
    #[error("{field} must be finite and non-negative")]
    InvalidRate {
        /// Invalid parameter name.
        field: &'static str,
    },
    /// An activation is outside the finite unit interval.
    #[error("{field} must be finite and between 0.0 and 1.0")]
    InvalidActivation {
        /// Invalid parameter name.
        field: &'static str,
    },
    /// The edge's current weight is not finite.
    #[error("edge weight must be finite")]
    NonFiniteWeight,
}

/// Applies the bounded online connection update formula.
#[derive(Clone, Copy, Debug, Default)]
pub struct EdgeUpdater;

impl EdgeUpdater {
    /// Applies `clip(weight + eta*a_i*a_j + alpha*success - beta*failure)`.
    ///
    /// Success and failure evidence is appended to the edge so a derived
    /// weight never loses the authoritative record that caused its change.
    ///
    /// # Errors
    ///
    /// Returns [`EdgeUpdateError`] for invalid rates, activations, or a
    /// non-finite current weight. The edge remains unchanged on error.
    pub fn apply(
        edge: &mut CognitiveEdge,
        eta: f32,
        alpha: f32,
        beta: f32,
        activations: [f32; 2],
        outcome: LearningOutcome,
    ) -> Result<EdgeUpdate, EdgeUpdateError> {
        validate_rate("eta", eta)?;
        validate_rate("alpha", alpha)?;
        validate_rate("beta", beta)?;
        validate_activation("source_activation", activations[0])?;
        validate_activation("target_activation", activations[1])?;
        if !edge.weight.is_finite() {
            return Err(EdgeUpdateError::NonFiniteWeight);
        }

        let activation_delta = eta * activations[0] * activations[1];
        let (outcome_delta, evidence) = match outcome {
            LearningOutcome::Neutral => (0.0, None),
            LearningOutcome::Success(evidence) => (alpha, Some(evidence)),
            LearningOutcome::Failure(evidence) => (-beta, Some(evidence)),
        };
        let previous_weight = edge.weight;
        let new_weight = (previous_weight + activation_delta + outcome_delta).clamp(0.0, 1.0);
        if let Some(evidence) = &evidence {
            edge.evidence.push(evidence.clone());
        }
        edge.weight = new_weight;

        Ok(EdgeUpdate {
            previous_weight,
            new_weight,
            activation_delta,
            outcome_delta,
            evidence,
        })
    }
}

fn validate_rate(field: &'static str, value: f32) -> Result<(), EdgeUpdateError> {
    if value.is_finite() && value >= 0.0 {
        Ok(())
    } else {
        Err(EdgeUpdateError::InvalidRate { field })
    }
}

fn validate_activation(field: &'static str, value: f32) -> Result<(), EdgeUpdateError> {
    if value.is_finite() && (0.0..=1.0).contains(&value) {
        Ok(())
    } else {
        Err(EdgeUpdateError::InvalidActivation { field })
    }
}
