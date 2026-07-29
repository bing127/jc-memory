//! Deterministic local storage and bounded adjacency loading for derived graph data.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use jc_memory_protocol::{EdgeId, MemoryScope, PointId};
use thiserror::Error;

use crate::{CognitiveEdge, CognitivePoint, EdgeFilter, LocalGraph};

/// An invalid local graph repository mutation.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum GraphRepositoryError {
    /// A point ID was reused with different content.
    #[error("point {0} already exists with different content")]
    PointIdentityConflict(PointId),
    /// A point does not reference any authoritative fragment.
    #[error("point {0} must reference at least one authoritative fragment")]
    MissingPointEvidence(PointId),
    /// An edge ID was reused with different content.
    #[error("edge {0} already exists with different content")]
    EdgeIdentityConflict(EdgeId),
    /// An edge references a point that is not in the repository.
    #[error("edge endpoint point {0} does not exist")]
    MissingPoint(PointId),
    /// Edge scope differs from one or both endpoint scopes.
    #[error("edge and endpoint points must share the exact same scope")]
    ScopeMismatch,
    /// An edge links a point to itself.
    #[error("edge cannot link point {0} to itself")]
    SelfLink(PointId),
    /// An edge has no authoritative evidence link.
    #[error("edge must retain at least one authoritative evidence link")]
    MissingEvidence,
    /// An edge weight or confidence is non-finite or outside the unit interval.
    #[error("{field} must be finite and between 0.0 and 1.0")]
    InvalidUnitInterval {
        /// Invalid field name.
        field: &'static str,
    },
    /// An edge validity interval ends before it begins.
    #[error("edge valid_to cannot be earlier than valid_from")]
    InvalidValidityInterval,
}

/// In-process repository for reconstructable graph points and edges.
#[derive(Clone, Debug, Default)]
pub struct LocalGraphRepository {
    points: BTreeMap<PointId, CognitivePoint>,
    edges: BTreeMap<EdgeId, CognitiveEdge>,
}

impl LocalGraphRepository {
    /// Builds a repository while validating endpoint, scope, evidence, and weight invariants.
    ///
    /// # Errors
    ///
    /// Returns [`GraphRepositoryError`] when an item is inconsistent with the
    /// local graph or reuses an identifier with different content.
    pub fn try_from_parts(
        points: impl IntoIterator<Item = CognitivePoint>,
        edges: impl IntoIterator<Item = CognitiveEdge>,
    ) -> Result<Self, GraphRepositoryError> {
        let mut repository = Self::default();
        for point in points {
            repository.insert_point(point)?;
        }
        for edge in edges {
            repository.insert_edge(edge)?;
        }
        Ok(repository)
    }

    /// Inserts a point idempotently.
    ///
    /// Returns `true` when inserted and `false` when the exact point already exists.
    ///
    /// # Errors
    ///
    /// Returns [`GraphRepositoryError::PointIdentityConflict`] when the ID is
    /// already associated with different content, or
    /// [`GraphRepositoryError::MissingPointEvidence`] when no source Fragment exists.
    pub fn insert_point(&mut self, point: CognitivePoint) -> Result<bool, GraphRepositoryError> {
        if point.source_fragments.is_empty() {
            return Err(GraphRepositoryError::MissingPointEvidence(point.id));
        }
        match self.points.get(&point.id) {
            Some(existing) if existing == &point => Ok(false),
            Some(_) => Err(GraphRepositoryError::PointIdentityConflict(point.id)),
            None => {
                self.points.insert(point.id, point);
                Ok(true)
            }
        }
    }

    /// Inserts an edge idempotently after checking all local graph invariants.
    ///
    /// Returns `true` when inserted and `false` when the exact edge already exists.
    ///
    /// # Errors
    ///
    /// Returns [`GraphRepositoryError`] when an endpoint is absent, graph scope
    /// differs, evidence is absent, values are invalid, or the ID conflicts.
    pub fn insert_edge(&mut self, edge: CognitiveEdge) -> Result<bool, GraphRepositoryError> {
        self.validate_edge(&edge)?;
        match self.edges.get(&edge.id) {
            Some(existing) if existing == &edge => Ok(false),
            Some(_) => Err(GraphRepositoryError::EdgeIdentityConflict(edge.id)),
            None => {
                self.edges.insert(edge.id, edge);
                Ok(true)
            }
        }
    }

    /// Loads a scope-bound breadth-first subgraph under neighbor and node budgets.
    ///
    /// Outgoing edges are considered by descending weight and then ascending
    /// edge ID. Seeds retain caller order, invalid or cross-scope seeds are ignored,
    /// and no edge is returned unless both endpoints fit in the node budget.
    #[must_use]
    pub fn load(
        &self,
        scope: &MemoryScope,
        seeds: &[PointId],
        filter: &EdgeFilter,
        max_neighbors: usize,
        max_nodes: usize,
    ) -> LocalGraph {
        let mut selected = BTreeSet::new();
        let mut point_order = Vec::new();
        let mut queue = VecDeque::new();
        let mut truncated = false;

        for seed in seeds {
            let Some(point) = self.points.get(seed).filter(|point| &point.scope == scope) else {
                continue;
            };
            if selected.contains(seed) {
                continue;
            }
            if selected.len() == max_nodes {
                truncated = true;
                continue;
            }
            selected.insert(*seed);
            point_order.push(point.clone());
            queue.push_back(*seed);
        }

        let mut loaded_edges = Vec::new();
        while let Some(source_id) = queue.pop_front() {
            let mut outgoing: Vec<_> = self
                .edges
                .values()
                .filter(|edge| {
                    edge.scope == *scope
                        && edge.source_point_id == source_id
                        && filter.allows(edge.kind)
                })
                .collect();
            outgoing.sort_by(|left, right| {
                right
                    .weight
                    .total_cmp(&left.weight)
                    .then_with(|| left.id.cmp(&right.id))
            });
            if outgoing.len() > max_neighbors {
                truncated = true;
            }

            for edge in outgoing.into_iter().take(max_neighbors) {
                if !selected.contains(&edge.target_point_id) {
                    if selected.len() == max_nodes {
                        truncated = true;
                        continue;
                    }
                    let Some(target) = self.points.get(&edge.target_point_id) else {
                        truncated = true;
                        continue;
                    };
                    selected.insert(target.id);
                    point_order.push(target.clone());
                    queue.push_back(target.id);
                }
                loaded_edges.push(edge.clone());
            }
        }

        LocalGraph {
            points: point_order,
            edges: loaded_edges,
            truncated,
        }
    }

    fn validate_edge(&self, edge: &CognitiveEdge) -> Result<(), GraphRepositoryError> {
        if edge.source_point_id == edge.target_point_id {
            return Err(GraphRepositoryError::SelfLink(edge.source_point_id));
        }
        let source = self
            .points
            .get(&edge.source_point_id)
            .ok_or(GraphRepositoryError::MissingPoint(edge.source_point_id))?;
        let target = self
            .points
            .get(&edge.target_point_id)
            .ok_or(GraphRepositoryError::MissingPoint(edge.target_point_id))?;
        if source.scope != target.scope || source.scope != edge.scope {
            return Err(GraphRepositoryError::ScopeMismatch);
        }
        if edge.evidence.is_empty() {
            return Err(GraphRepositoryError::MissingEvidence);
        }
        validate_unit_interval("weight", edge.weight)?;
        validate_unit_interval("confidence", edge.confidence)?;
        if edge
            .valid_to
            .is_some_and(|valid_to| valid_to < edge.valid_from)
        {
            return Err(GraphRepositoryError::InvalidValidityInterval);
        }
        Ok(())
    }
}

fn validate_unit_interval(field: &'static str, value: f32) -> Result<(), GraphRepositoryError> {
    if value.is_finite() && (0.0..=1.0).contains(&value) {
        Ok(())
    } else {
        Err(GraphRepositoryError::InvalidUnitInterval { field })
    }
}
