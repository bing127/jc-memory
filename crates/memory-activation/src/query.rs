//! Recall queries and hard execution limits.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use chrono::{DateTime, Utc};
use jc_memory_protocol::{MemoryScope, RecallMode};

/// Hard limits applied to every recall request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecallLimits {
    /// Maximum number of initial seed Points.
    pub max_seeds: usize,
    /// Maximum outgoing neighbors expanded from one Point.
    pub max_neighbors: usize,
    /// Maximum traversal depth from a seed.
    pub max_hops: usize,
    /// Maximum total Points returned by the request.
    pub max_nodes: usize,
    /// Maximum wall-clock execution time in milliseconds.
    pub timeout_ms: u64,
}

/// One intent-scoped request for bounded memory activation.
#[derive(Clone, Debug, PartialEq)]
pub struct RecallQuery {
    /// Exact ownership scope of eligible memory.
    pub scope: MemoryScope,
    /// Recall intent controlling seed and edge selection.
    pub mode: RecallMode,
    /// User or runtime search text.
    pub text: String,
    /// Canonical entity names already identified by the caller.
    pub entities: Vec<String>,
    /// Optional historical cutoff; future Points are excluded.
    pub at_time: Option<DateTime<Utc>>,
    /// Hard execution budgets.
    pub limits: RecallLimits,
}

/// Thread-safe cooperative cancellation handle for one or more Recall calls.
#[derive(Clone, Debug, Default)]
pub struct RecallCancellation {
    cancelled: Arc<AtomicBool>,
}

impl RecallCancellation {
    /// Creates an active cancellation handle.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Requests cancellation. Calls already in progress observe it cooperatively.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    /// Returns whether cancellation was requested.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}
