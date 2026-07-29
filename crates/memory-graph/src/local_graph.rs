//! Bounded local graph result types.

use jc_memory_protocol::EdgeKind;

use crate::{CognitiveEdge, CognitivePoint};

/// A whitelist of edge kinds allowed during local graph loading.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EdgeFilter {
    kinds: Option<Vec<EdgeKind>>,
}

impl EdgeFilter {
    /// Allows every edge kind.
    #[must_use]
    pub const fn all() -> Self {
        Self { kinds: None }
    }

    /// Allows only the listed edge kinds.
    #[must_use]
    pub fn only(kinds: impl IntoIterator<Item = EdgeKind>) -> Self {
        let mut kinds: Vec<_> = kinds.into_iter().collect();
        kinds.dedup();
        Self { kinds: Some(kinds) }
    }

    pub(crate) fn allows(&self, kind: EdgeKind) -> bool {
        self.kinds
            .as_ref()
            .is_none_or(|kinds| kinds.contains(&kind))
    }
}

impl Default for EdgeFilter {
    fn default() -> Self {
        Self::all()
    }
}

/// A deterministic, budget-bounded subgraph rooted at requested seed points.
#[derive(Clone, Debug, PartialEq)]
pub struct LocalGraph {
    /// Points in seed-first breadth-first order.
    pub points: Vec<CognitivePoint>,
    /// Directed edges in traversal order.
    pub edges: Vec<CognitiveEdge>,
    /// Whether an eligible neighbor or point was excluded by a budget.
    pub truncated: bool,
}
