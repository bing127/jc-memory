//! Evidence-backed semantic facts, preferences, and constraints.

use std::collections::BTreeSet;

use jc_memory_protocol::{EventId, FragmentId, FragmentKind, MemoryScope, PointId, ValidityStatus};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

/// Stable semantic memory category.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticKind {
    /// Evidence-backed fact.
    Fact,
    /// Evidence-backed preference.
    Preference,
    /// Evidence-backed constraint.
    Constraint,
}

/// One independent Event and Fragment supporting semantic promotion.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct SemanticEvidence {
    /// Exact ownership scope of the supporting Event and Fragment.
    pub scope: MemoryScope,
    /// Independent authoritative Event identifier.
    pub event_id: EventId,
    /// Authoritative Fragment identifier carried by the Event.
    pub fragment_id: FragmentId,
    /// Original Fragment kind, retained to prevent hypothesis promotion.
    pub kind: FragmentKind,
    /// Validity of the supporting Fragment.
    pub validity: ValidityStatus,
    /// Confidence of the supporting Fragment.
    pub confidence: f32,
}

/// Stable derived semantic memory with explicit authoritative support.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct SemanticEntry {
    /// Derived semantic point identifier.
    pub id: PointId,
    /// Exact ownership scope shared by all evidence.
    pub scope: MemoryScope,
    /// Semantic category.
    pub kind: SemanticKind,
    /// Structured derived value; authoritative content remains in evidence Fragments.
    pub value: Value,
    /// Whether the independent Event threshold was met.
    pub stable: bool,
    /// Mean confidence of distinct supporting Events.
    pub confidence: f32,
    /// Derived validity that never upgrades unverified evidence.
    pub validity: ValidityStatus,
    /// Distinct authoritative Events and Fragments supporting the entry.
    pub evidence: Vec<SemanticEvidence>,
}

/// A rejected semantic promotion.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum SemanticPromotionError {
    /// Supporting evidence belongs to another exact scope.
    #[error("event {0} does not belong to the semantic entry scope")]
    EvidenceScopeMismatch(EventId),
    /// Fewer than two distinct Events support the entry.
    #[error("semantic promotion requires two independent events; found {found}")]
    InsufficientIndependentEvents {
        /// Number of distinct supporting Events.
        found: usize,
    },
    /// A hypothesis-like Fragment was used to create a semantic fact.
    #[error("fragment {fragment_id} with kind {kind:?} cannot become a semantic fact")]
    HypothesisCannotBecomeFact {
        /// Rejected Fragment identifier.
        fragment_id: FragmentId,
        /// Rejected hypothesis-like kind.
        kind: FragmentKind,
    },
    /// Supporting confidence is non-finite or outside the unit interval.
    #[error("evidence confidence must be finite and between 0.0 and 1.0")]
    InvalidConfidence,
}

impl SemanticEntry {
    /// Promotes content only after two distinct authoritative Events support it.
    ///
    /// Duplicate Event IDs are counted once. Prediction, Inference, and Plan
    /// evidence cannot directly produce a [`SemanticKind::Fact`].
    ///
    /// # Errors
    ///
    /// Returns [`SemanticPromotionError`] when evidence is insufficient,
    /// hypothetical for a fact, or carries invalid confidence.
    pub fn promote(
        scope: MemoryScope,
        kind: SemanticKind,
        value: Value,
        evidence: Vec<SemanticEvidence>,
    ) -> Result<Self, SemanticPromotionError> {
        if let Some(item) = evidence.iter().find(|item| item.scope != scope) {
            return Err(SemanticPromotionError::EvidenceScopeMismatch(item.event_id));
        }
        let mut seen = BTreeSet::new();
        let evidence: Vec<_> = evidence
            .into_iter()
            .filter(|item| seen.insert(item.event_id))
            .collect();
        if evidence.len() < 2 {
            return Err(SemanticPromotionError::InsufficientIndependentEvents {
                found: evidence.len(),
            });
        }
        if let Some(item) = evidence.iter().find(|item| {
            kind == SemanticKind::Fact
                && matches!(
                    item.kind,
                    FragmentKind::Prediction | FragmentKind::Inference | FragmentKind::Plan
                )
        }) {
            return Err(SemanticPromotionError::HypothesisCannotBecomeFact {
                fragment_id: item.fragment_id,
                kind: item.kind,
            });
        }
        if evidence
            .iter()
            .any(|item| !item.confidence.is_finite() || !(0.0..=1.0).contains(&item.confidence))
        {
            return Err(SemanticPromotionError::InvalidConfidence);
        }

        let (confidence_sum, evidence_count) = evidence
            .iter()
            .fold((0.0_f32, 0.0_f32), |(sum, count), item| {
                (sum + item.confidence, count + 1.0)
            });
        let validity = derived_validity(&evidence);
        Ok(Self {
            id: PointId::new(),
            scope,
            kind,
            value,
            stable: true,
            confidence: confidence_sum / evidence_count,
            validity,
            evidence,
        })
    }
}

fn derived_validity(evidence: &[SemanticEvidence]) -> ValidityStatus {
    if evidence
        .iter()
        .all(|item| item.validity == ValidityStatus::Verified)
    {
        ValidityStatus::Verified
    } else if evidence
        .iter()
        .any(|item| item.validity == ValidityStatus::Invalidated)
    {
        ValidityStatus::Invalidated
    } else if evidence
        .iter()
        .any(|item| item.validity == ValidityStatus::Quarantined)
    {
        ValidityStatus::Quarantined
    } else if evidence
        .iter()
        .any(|item| item.validity == ValidityStatus::Disputed)
    {
        ValidityStatus::Disputed
    } else {
        ValidityStatus::Unverified
    }
}
