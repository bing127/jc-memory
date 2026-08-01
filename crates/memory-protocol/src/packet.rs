//! Model-independent, evidence-linked recall response types.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    EdgeId, EdgeKind, EventId, EvidenceId, FragmentId, PointId, PointKind, RecallId, RecallMode,
    StateId, StateVersionId, SurfaceId, SurfaceStatus, ValidityStatus,
};

/// Source identifiers supporting one derived Packet item.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EvidenceView {
    /// Authoritative Fragment identifier.
    pub fragment_id: FragmentId,
    /// Authoritative Event identifier, when persisted.
    pub event_id: Option<EventId>,
    /// Existing evidence-reference identifier, when applicable.
    pub evidence_ref_id: Option<EvidenceId>,
}

/// One effective value from a versioned state chain.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct StateView {
    /// Stable state identity.
    pub state_id: StateId,
    /// Immutable state-version identity.
    pub version_id: StateVersionId,
    /// Caller-defined state key.
    pub key: String,
    /// Structured state value.
    pub value: Value,
    /// Inclusive beginning of the effective interval.
    pub valid_from: DateTime<Utc>,
    /// Exclusive end of the effective interval.
    pub valid_to: Option<DateTime<Utc>>,
    /// Explicit validity of this version.
    pub validity: ValidityStatus,
    /// Authoritative records supporting this version.
    pub evidence: Vec<EvidenceView>,
    /// 该状态版本成立或变化的显式原因。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// 产生该状态版本的权威 Event。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_event_id: Option<EventId>,
}

/// One activated version of a cognitive Surface.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct SurfaceView {
    /// Stable Surface identity.
    pub surface_id: SurfaceId,
    /// Immutable version number.
    pub version: u64,
    /// Lifecycle status at the requested effective time.
    pub status: SurfaceStatus,
    /// Member Point IDs and their weights.
    pub member_points: BTreeMap<PointId, f32>,
    /// Confidence in this derived Surface version.
    pub confidence: f32,
    /// Authoritative records supporting this version.
    pub evidence: Vec<EvidenceView>,
}

/// One source episode retained by authoritative IDs.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct EpisodeView {
    /// Immutable Event identity.
    pub event_id: EventId,
    /// Original Fragment carried by the Event.
    pub fragment_id: FragmentId,
    /// Source occurrence time.
    pub occurred_at: DateTime<Utc>,
    /// Deterministic display summary; it never replaces the Fragment.
    pub summary: String,
    /// Authoritative evidence supporting derived episode fields.
    pub evidence: Vec<EvidenceView>,
}

/// One activated graph Point.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct PointView {
    /// Stable Point identity.
    pub point_id: PointId,
    /// Atomic cognitive role.
    pub kind: PointKind,
    /// Confidence inherited from source evidence.
    pub confidence: f32,
    /// Explicit validity inherited from source evidence.
    pub validity: ValidityStatus,
    /// Authoritative Fragments from which this Point was derived.
    pub source_fragments: Vec<FragmentId>,
}

/// Ordered Point and Edge identifiers forming one recall path.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct RecallPath {
    /// Points in traversal order.
    pub point_ids: Vec<PointId>,
    /// Directed Edges connecting adjacent Points.
    pub edge_ids: Vec<EdgeId>,
    /// Deterministic path score.
    pub score: f32,
}

/// Evidence-backed procedure outcome statistics.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ProcedureView {
    /// Stable caller-defined procedure key.
    pub procedure_key: String,
    /// Number of independent outcome Events.
    pub sample_count: u32,
    /// Success fraction paired with its sample count.
    pub success_rate: f64,
    /// Authoritative outcome records counted in the statistics.
    pub evidence: Vec<EvidenceView>,
}

/// Category of a stable semantic knowledge item.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeKind {
    /// Evidence-backed fact.
    Fact,
    /// Evidence-backed preference.
    Preference,
    /// Evidence-backed constraint.
    Constraint,
}

/// Stable semantic knowledge with authoritative source IDs.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct KnowledgeView {
    /// Derived semantic Point identity.
    pub point_id: PointId,
    /// Semantic category.
    pub kind: KnowledgeKind,
    /// Structured derived value.
    pub value: Value,
    /// Evidence-derived confidence.
    pub confidence: f32,
    /// Explicit evidence-derived validity.
    pub validity: ValidityStatus,
    /// Independent authoritative records supporting the item.
    pub evidence: Vec<EvidenceView>,
}

/// Visible disagreement retained for caller resolution.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ConflictView {
    /// Point identities participating in the disagreement.
    pub point_ids: Vec<PointId>,
    /// Contradiction or competing-state Edge identities, when present.
    pub edge_ids: Vec<EdgeId>,
    /// Authoritative records demonstrating the disagreement.
    pub evidence: Vec<EvidenceView>,
}

/// Explicit evidence gap associated with a source Point.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MissingEvidence {
    /// Point whose claim or relationship lacks sufficient evidence.
    pub point_id: PointId,
    /// Relationship kinds whose evidence is missing.
    pub expected_edge_kinds: Vec<EdgeKind>,
    /// Model-independent description of the gap.
    pub description: String,
}

/// Bounded recall coverage and truncation accounting.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct Coverage {
    /// Selected initial seed count.
    pub seed_count: usize,
    /// Unique Point count returned by bounded traversal.
    pub visited_nodes: usize,
    /// Deepest returned traversal level.
    pub max_depth: usize,
    /// Number of returned authoritative evidence records.
    pub evidence_count: usize,
    /// Number of explicit unresolved evidence gaps.
    pub missing_evidence_count: usize,
    /// Whether a hard limit excluded any candidate.
    pub truncated: bool,
}

/// Model-independent response shared by embedded and remote SDK boundaries.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct MemoryPacket {
    /// Recall request identity.
    pub query_id: RecallId,
    /// Recall intent used to build this Packet.
    pub intent: RecallMode,
    /// Current effective state values.
    pub current_states: Vec<StateView>,
    /// Historical state values relevant to the request.
    pub historical_states: Vec<StateView>,
    /// Activated cognitive Surface versions.
    pub activated_surfaces: Vec<SurfaceView>,
    /// Relevant episodic records.
    pub episodes: Vec<EpisodeView>,
    /// Activated graph Points.
    pub points: Vec<PointView>,
    /// Minimal evidence-linked recall paths.
    pub paths: Vec<RecallPath>,
    /// Relevant procedure outcome statistics.
    pub procedures: Vec<ProcedureView>,
    /// Evidence-backed constraints.
    pub constraints: Vec<KnowledgeView>,
    /// Evidence-backed preferences.
    pub preferences: Vec<KnowledgeView>,
    /// Relevant observed failures.
    pub past_failures: Vec<EpisodeView>,
    /// Deduplicated authoritative evidence records.
    pub evidence: Vec<EvidenceView>,
    /// Visible unresolved disagreements.
    pub conflicts: Vec<ConflictView>,
    /// Explicit evidence gaps.
    pub missing_evidence: Vec<MissingEvidence>,
    /// Bounded traversal and evidence coverage.
    pub coverage: Coverage,
    /// Aggregate Packet confidence.
    pub confidence: f32,
    /// Deterministically rendered model-independent context.
    pub recommended_context: String,
}

impl MemoryPacket {
    /// Creates an empty Packet for one query and intent.
    #[must_use]
    pub fn empty(query_id: RecallId, intent: RecallMode) -> Self {
        Self {
            query_id,
            intent,
            current_states: Vec::new(),
            historical_states: Vec::new(),
            activated_surfaces: Vec::new(),
            episodes: Vec::new(),
            points: Vec::new(),
            paths: Vec::new(),
            procedures: Vec::new(),
            constraints: Vec::new(),
            preferences: Vec::new(),
            past_failures: Vec::new(),
            evidence: Vec::new(),
            conflicts: Vec::new(),
            missing_evidence: Vec::new(),
            coverage: Coverage::default(),
            confidence: 0.0,
            recommended_context: String::new(),
        }
    }

    /// Rebuilds model-independent context from every other Packet field.
    ///
    /// The result is deterministic JSON data rather than a model-specific
    /// role, instruction, or prompt envelope.
    ///
    /// # Errors
    ///
    /// Returns a serialization error if a Packet value cannot be represented
    /// as JSON.
    pub fn refresh_recommended_context(&mut self) -> Result<(), serde_json::Error> {
        let mut rendered = serde_json::to_value(&*self)?;
        if let Some(object) = rendered.as_object_mut() {
            object.remove("recommended_context");
        }
        self.recommended_context = serde_json::to_string(&rendered)?;
        Ok(())
    }
}
