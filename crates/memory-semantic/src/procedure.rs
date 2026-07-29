//! Evidence-backed procedure outcome statistics.

use jc_memory_protocol::{EventId, FragmentId, MemoryScope};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// One observed procedure outcome and its authoritative identifiers.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProcedureEvidence {
    /// Exact ownership scope of the outcome Event and Fragment.
    pub scope: MemoryScope,
    /// Event recording the actual outcome.
    pub event_id: EventId,
    /// Fragment carried by the outcome Event.
    pub fragment_id: FragmentId,
    /// Whether this execution succeeded.
    pub succeeded: bool,
}

/// Derived procedure statistics that retain every counted evidence ID.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ProcedureExperience {
    /// Exact ownership scope of the procedure statistics.
    pub scope: MemoryScope,
    /// Stable caller-defined procedure or capability key.
    pub procedure_key: String,
    /// Number of successful independent Events.
    pub success_count: u32,
    /// Number of failed independent Events.
    pub failure_count: u32,
    /// Total number of independent Events.
    pub sample_count: u32,
    /// `success_count / sample_count`, retained with its denominator.
    pub success_rate: f64,
    /// Authoritative Event and Fragment IDs counted in the statistics.
    pub evidence: Vec<ProcedureEvidence>,
}

/// A rejected procedure-experience mutation.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum ProcedureExperienceError {
    /// Outcome evidence belongs to another exact scope.
    #[error("event {0} does not belong to the procedure experience scope")]
    EvidenceScopeMismatch(EventId),
    /// Procedure keys cannot be empty.
    #[error("procedure key cannot be empty")]
    EmptyProcedureKey,
    /// One Event cannot count as more than one independent sample.
    #[error("event {0} is already included in the procedure experience")]
    DuplicateEvent(EventId),
    /// A counter reached its representable limit.
    #[error("procedure sample count overflow")]
    SampleCountOverflow,
}

impl ProcedureExperience {
    /// Creates procedure statistics from the first observed outcome.
    ///
    /// # Errors
    ///
    /// Returns [`ProcedureExperienceError::EmptyProcedureKey`] for an empty key.
    pub fn new(
        scope: MemoryScope,
        procedure_key: impl Into<String>,
        evidence: ProcedureEvidence,
    ) -> Result<Self, ProcedureExperienceError> {
        let procedure_key = procedure_key.into();
        if procedure_key.trim().is_empty() {
            return Err(ProcedureExperienceError::EmptyProcedureKey);
        }
        let mut experience = Self {
            scope,
            procedure_key,
            success_count: 0,
            failure_count: 0,
            sample_count: 0,
            success_rate: 0.0,
            evidence: Vec::new(),
        };
        experience.record(evidence)?;
        Ok(experience)
    }

    /// Adds one distinct Event outcome and recomputes the evidence-backed rate.
    ///
    /// # Errors
    ///
    /// Returns [`ProcedureExperienceError`] for duplicate Event IDs or counter overflow.
    /// The experience remains unchanged on error.
    pub fn record(&mut self, evidence: ProcedureEvidence) -> Result<(), ProcedureExperienceError> {
        if evidence.scope != self.scope {
            return Err(ProcedureExperienceError::EvidenceScopeMismatch(
                evidence.event_id,
            ));
        }
        if self
            .evidence
            .iter()
            .any(|existing| existing.event_id == evidence.event_id)
        {
            return Err(ProcedureExperienceError::DuplicateEvent(evidence.event_id));
        }
        let sample_count = self
            .sample_count
            .checked_add(1)
            .ok_or(ProcedureExperienceError::SampleCountOverflow)?;
        let success_count = self
            .success_count
            .checked_add(u32::from(evidence.succeeded))
            .ok_or(ProcedureExperienceError::SampleCountOverflow)?;
        let failure_count = self
            .failure_count
            .checked_add(u32::from(!evidence.succeeded))
            .ok_or(ProcedureExperienceError::SampleCountOverflow)?;
        let success_rate = f64::from(success_count) / f64::from(sample_count);

        self.sample_count = sample_count;
        self.success_count = success_count;
        self.failure_count = failure_count;
        self.success_rate = success_rate;
        self.evidence.push(evidence);
        Ok(())
    }
}
