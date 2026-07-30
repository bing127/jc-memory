//! Scope-bound traversal with simultaneous seed, neighbor, hop, and node budgets.

use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet, BinaryHeap},
    time::{Duration, Instant},
};

use chrono::Utc;
use jc_memory_graph::{CognitiveEdge, GraphRepositoryError, LocalGraphRepository};
use jc_memory_protocol::{EdgeId, EdgeKind, PointId, RecallMode};
use jc_memory_surface::CognitiveSurface;
use thiserror::Error;

use crate::{ActivatedSurface, IndexedPoint, RecallCancellation, RecallQuery, completion, seeds};

const SEMANTIC_EDGES: &[EdgeKind] = &[
    EdgeKind::Supports,
    EdgeKind::Contradicts,
    EdgeKind::PartOf,
    EdgeKind::SameEntity,
    EdgeKind::CoActivated,
];
const TEMPORAL_EDGES: &[EdgeKind] = &[
    EdgeKind::TemporalBefore,
    EdgeKind::TemporalAfter,
    EdgeKind::Supersedes,
];
const CAUSAL_EDGES: &[EdgeKind] = &[
    EdgeKind::Causes,
    EdgeKind::CausedBy,
    EdgeKind::ValidatedBy,
    EdgeKind::DisprovedBy,
    EdgeKind::ResultedIn,
];
const ENTITY_EDGES: &[EdgeKind] = &[
    EdgeKind::PartOf,
    EdgeKind::SameEntity,
    EdgeKind::CoActivated,
];
const ASSOCIATIVE_EDGES: &[EdgeKind] = &[
    EdgeKind::Supports,
    EdgeKind::PartOf,
    EdgeKind::SameEntity,
    EdgeKind::CoActivated,
];
const PROCEDURAL_EDGES: &[EdgeKind] = &[
    EdgeKind::DependsOn,
    EdgeKind::ValidatedBy,
    EdgeKind::DisprovedBy,
    EdgeKind::ExecutedAs,
    EdgeKind::ResultedIn,
];
const CURRENT_STATE_EDGES: &[EdgeKind] = &[
    EdgeKind::Supports,
    EdgeKind::Contradicts,
    EdgeKind::Supersedes,
];
const HISTORICAL_STATE_EDGES: &[EdgeKind] = &[
    EdgeKind::TemporalBefore,
    EdgeKind::TemporalAfter,
    EdgeKind::Contradicts,
    EdgeKind::Supersedes,
];
const ALL_EDGES: &[EdgeKind] = &[
    EdgeKind::TemporalBefore,
    EdgeKind::TemporalAfter,
    EdgeKind::Causes,
    EdgeKind::CausedBy,
    EdgeKind::Supports,
    EdgeKind::Contradicts,
    EdgeKind::Supersedes,
    EdgeKind::DependsOn,
    EdgeKind::PartOf,
    EdgeKind::SameEntity,
    EdgeKind::CoActivated,
    EdgeKind::PredictedBy,
    EdgeKind::ValidatedBy,
    EdgeKind::DisprovedBy,
    EdgeKind::RecalledBy,
    EdgeKind::UsedInDecision,
    EdgeKind::ExecutedAs,
    EdgeKind::ResultedIn,
];

/// Returns the explicit directed Edge whitelist for one Recall intent.
#[must_use]
pub const fn edge_kinds_for(mode: RecallMode) -> &'static [EdgeKind] {
    match mode {
        RecallMode::Exact => &[],
        RecallMode::Semantic => SEMANTIC_EDGES,
        RecallMode::Temporal => TEMPORAL_EDGES,
        RecallMode::Causal => CAUSAL_EDGES,
        RecallMode::Entity => ENTITY_EDGES,
        RecallMode::Associative => ASSOCIATIVE_EDGES,
        RecallMode::Procedural => PROCEDURAL_EDGES,
        RecallMode::CurrentState => CURRENT_STATE_EDGES,
        RecallMode::HistoricalState => HISTORICAL_STATE_EDGES,
        RecallMode::DeepReconstruction => ALL_EDGES,
    }
}

/// One activated Point and its bounded traversal metadata.
#[derive(Clone, Debug, PartialEq)]
pub struct ActivatedPoint {
    /// Activated graph Point.
    pub point_id: PointId,
    /// Activation score used for ordering later expansion.
    pub activation: f32,
    /// Traversal depth from the nearest selected seed.
    pub depth: usize,
    /// Edge used to first reach the Point, absent for seeds.
    pub via_edge_id: Option<EdgeId>,
}

/// Budget accounting for one recall run.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RecallTrace {
    /// Number of selected initial seeds.
    pub seed_count: usize,
    /// Number of unique visited Points.
    pub visited_nodes: usize,
    /// Points reached through bounded evidence-gap completion.
    pub completion_nodes: usize,
    /// Deepest visited traversal level.
    pub max_depth: usize,
    /// Number of neighbors expanded from each Point.
    pub expanded_neighbors: BTreeMap<PointId, usize>,
    /// Whether any candidate was excluded by a hard limit.
    pub truncated: bool,
}

/// Bounded intermediate result returned before Packet rendering.
#[derive(Clone, Debug, PartialEq)]
pub struct RecallResult {
    /// Activated Points in deterministic visit order.
    pub points: Vec<ActivatedPoint>,
    /// Traversed evidence-linked Edges.
    pub edges: Vec<CognitiveEdge>,
    /// Live Surface versions activated by visited member Points.
    pub activated_surfaces: Vec<ActivatedSurface>,
    /// Budget accounting for the run.
    pub trace: RecallTrace,
}

/// Invalid immutable input supplied to a Recall engine.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum RecallEngineError {
    /// A graph invariant was violated.
    #[error(transparent)]
    Graph(#[from] GraphRepositoryError),
    /// A Point ID was paired with two different search records.
    #[error("point {0} has conflicting search records")]
    PointIdentityConflict(PointId),
    /// Cooperative cancellation was requested before the result completed.
    #[error("recall was cancelled")]
    Cancelled,
    /// The query exceeded its wall-clock timeout.
    #[error("recall timed out")]
    TimedOut,
}

/// Immutable in-process input for deterministic bounded recall.
#[derive(Clone, Debug)]
pub struct RecallEngine {
    points: BTreeMap<PointId, IndexedPoint>,
    outgoing: BTreeMap<PointId, Vec<CognitiveEdge>>,
    surfaces: Vec<CognitiveSurface>,
}

#[derive(Clone, Debug)]
struct FrontierPoint {
    point_id: PointId,
    activation: f32,
    depth: usize,
    occurred_at: chrono::DateTime<Utc>,
    via_edge: Option<CognitiveEdge>,
    evidence_completion: bool,
}

struct SpreadOutput {
    points: Vec<ActivatedPoint>,
    edges: Vec<CognitiveEdge>,
    trace: RecallTrace,
}

struct EligibleEdges<'a> {
    edges: Vec<&'a CognitiveEdge>,
    evidence_gap: bool,
}

impl PartialEq for FrontierPoint {
    fn eq(&self, other: &Self) -> bool {
        self.activation.to_bits() == other.activation.to_bits()
            && self.point_id == other.point_id
            && self.depth == other.depth
            && self.occurred_at == other.occurred_at
            && self.evidence_completion == other.evidence_completion
    }
}

impl Eq for FrontierPoint {}

impl Ord for FrontierPoint {
    fn cmp(&self, other: &Self) -> Ordering {
        self.activation
            .total_cmp(&other.activation)
            .then_with(|| self.occurred_at.cmp(&other.occurred_at))
            .then_with(|| other.point_id.cmp(&self.point_id))
    }
}

impl PartialOrd for FrontierPoint {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl RecallEngine {
    /// Builds a Recall engine after validating graph endpoint and evidence invariants.
    ///
    /// # Errors
    ///
    /// Returns [`RecallEngineError`] for invalid graph parts or conflicting
    /// search records that reuse one Point ID.
    pub fn try_from_parts(
        points: impl IntoIterator<Item = IndexedPoint>,
        edges: impl IntoIterator<Item = CognitiveEdge>,
        surfaces: impl IntoIterator<Item = CognitiveSurface>,
    ) -> Result<Self, RecallEngineError> {
        let points: Vec<_> = points.into_iter().collect();
        let edges: Vec<_> = edges.into_iter().collect();
        LocalGraphRepository::try_from_parts(
            points.iter().map(|indexed| indexed.point.clone()),
            edges.iter().cloned(),
        )?;

        let mut point_map = BTreeMap::new();
        for indexed in points {
            match point_map.get(&indexed.point.id) {
                Some(existing) if existing == &indexed => {}
                Some(_) => {
                    return Err(RecallEngineError::PointIdentityConflict(indexed.point.id));
                }
                None => {
                    point_map.insert(indexed.point.id, indexed);
                }
            }
        }
        let mut outgoing: BTreeMap<_, Vec<_>> = BTreeMap::new();
        for edge in edges {
            outgoing.entry(edge.source_point_id).or_default().push(edge);
        }
        for edges in outgoing.values_mut() {
            edges.sort_by_key(|edge| edge.id);
        }

        Ok(Self {
            points: point_map,
            outgoing,
            surfaces: surfaces.into_iter().collect(),
        })
    }

    /// Executes a deterministic recall under all query budgets.
    ///
    /// # Errors
    ///
    pub async fn recall(&self, query: RecallQuery) -> Result<RecallResult, RecallEngineError> {
        self.recall_with_cancellation(query, &RecallCancellation::new())
            .await
    }

    /// Executes Recall with cooperative cancellation and wall-clock timeout checks.
    ///
    /// # Errors
    ///
    /// Returns [`RecallEngineError::Cancelled`] or [`RecallEngineError::TimedOut`]
    /// instead of returning a partial result when execution is interrupted.
    pub async fn recall_with_cancellation(
        &self,
        query: RecallQuery,
        cancellation: &RecallCancellation,
    ) -> Result<RecallResult, RecallEngineError> {
        let started_at = Instant::now();
        check_control(cancellation, started_at, query.limits.timeout_ms)?;
        let effective_at = query.at_time.unwrap_or_else(Utc::now);
        let seed_selection = seeds::select(self.points.values(), &query, effective_at);
        check_control(cancellation, started_at, query.limits.timeout_ms)?;
        let spread = self
            .spread(
                &query,
                effective_at,
                cancellation,
                started_at,
                seed_selection,
            )
            .await?;
        let activated_surfaces = completion::activate_surfaces(
            &self.surfaces,
            &query.scope,
            effective_at,
            &spread.points,
        );
        check_control(cancellation, started_at, query.limits.timeout_ms)?;

        Ok(RecallResult {
            points: spread.points,
            edges: spread.edges,
            activated_surfaces,
            trace: spread.trace,
        })
    }

    async fn spread(
        &self,
        query: &RecallQuery,
        effective_at: chrono::DateTime<Utc>,
        cancellation: &RecallCancellation,
        started_at: Instant,
        seed_selection: seeds::SeedSelection,
    ) -> Result<SpreadOutput, RecallEngineError> {
        let mut trace = RecallTrace {
            seed_count: seed_selection.candidates.len(),
            truncated: seed_selection.truncated,
            ..RecallTrace::default()
        };
        let mut visited = BTreeSet::new();
        let mut points = Vec::new();
        let mut frontier = BinaryHeap::new();
        for seed in seed_selection.candidates {
            frontier.push(FrontierPoint {
                point_id: seed.point_id,
                activation: seed.score,
                depth: 0,
                occurred_at: seed.occurred_at,
                via_edge: None,
                evidence_completion: false,
            });
        }

        let mut traversed_edges = Vec::new();
        let allowed_edges = edge_kinds_for(query.mode);
        while let Some(candidate) = frontier.pop() {
            tokio::task::yield_now().await;
            check_control(cancellation, started_at, query.limits.timeout_ms)?;
            if visited.contains(&candidate.point_id) {
                continue;
            }
            if visited.len() == query.limits.max_nodes {
                trace.truncated = true;
                break;
            }
            visited.insert(candidate.point_id);
            if let Some(edge) = &candidate.via_edge {
                traversed_edges.push(edge.clone());
            }
            if candidate.evidence_completion {
                trace.completion_nodes += 1;
            }
            points.push(ActivatedPoint {
                point_id: candidate.point_id,
                activation: candidate.activation,
                depth: candidate.depth,
                via_edge_id: candidate.via_edge.as_ref().map(|edge| edge.id),
            });

            let point_id = candidate.point_id;
            let eligible =
                self.eligible_edges(&candidate, allowed_edges, query, effective_at, &visited);
            if candidate.depth == query.limits.max_hops {
                if !eligible.edges.is_empty() {
                    trace.truncated = true;
                }
                continue;
            }
            if eligible.edges.len() > query.limits.max_neighbors {
                trace.truncated = true;
            }
            let mut expanded = 0_usize;
            for edge in eligible.edges.into_iter().take(query.limits.max_neighbors) {
                let next_activation = candidate.activation * edge.weight * edge.confidence;
                let target = &self.points[&edge.target_point_id];
                frontier.push(FrontierPoint {
                    point_id: edge.target_point_id,
                    activation: next_activation,
                    depth: candidate.depth + 1,
                    occurred_at: target.point.occurred_at,
                    via_edge: Some(edge.clone()),
                    evidence_completion: eligible.evidence_gap
                        && completion::is_evidence_edge(edge.kind),
                });
                expanded += 1;
            }
            trace.expanded_neighbors.insert(point_id, expanded);
        }
        trace.visited_nodes = visited.len();
        trace.max_depth = points.iter().map(|point| point.depth).max().unwrap_or(0);
        Ok(SpreadOutput {
            points,
            edges: traversed_edges,
            trace,
        })
    }

    fn eligible_edges<'a>(
        &'a self,
        candidate: &FrontierPoint,
        allowed_edges: &[EdgeKind],
        query: &RecallQuery,
        effective_at: chrono::DateTime<Utc>,
        visited: &BTreeSet<PointId>,
    ) -> EligibleEdges<'a> {
        let evidence_gap = self
            .points
            .get(&candidate.point_id)
            .is_some_and(|indexed| completion::has_evidence_gap(indexed.point.validity));
        let mut edges: Vec<_> = self
            .outgoing
            .get(&candidate.point_id)
            .into_iter()
            .flatten()
            .filter(|edge| {
                (allowed_edges.contains(&edge.kind)
                    || (evidence_gap && completion::is_evidence_edge(edge.kind)))
                    && !visited.contains(&edge.target_point_id)
                    && edge.valid_from <= effective_at
                    && edge.valid_to.is_none_or(|valid_to| effective_at < valid_to)
                    && self
                        .points
                        .get(&edge.target_point_id)
                        .is_some_and(|indexed| seeds::eligible(indexed, &query.scope, effective_at))
            })
            .collect();
        edges.sort_by(|left, right| {
            let left_score = left.weight * left.confidence;
            let right_score = right.weight * right.confidence;
            right_score
                .total_cmp(&left_score)
                .then_with(|| left.id.cmp(&right.id))
        });
        EligibleEdges {
            edges,
            evidence_gap,
        }
    }
}

fn check_control(
    cancellation: &RecallCancellation,
    started_at: Instant,
    timeout_ms: u64,
) -> Result<(), RecallEngineError> {
    if cancellation.is_cancelled() {
        return Err(RecallEngineError::Cancelled);
    }
    if started_at.elapsed() >= Duration::from_millis(timeout_ms) {
        return Err(RecallEngineError::TimedOut);
    }
    Ok(())
}
