//! Recall query modes shared by embedded and remote boundaries.

use serde::{Deserialize, Serialize};

/// Strategy used to select seeds and traverse bounded memory context.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecallMode {
    /// Finds exact identifiers or literal content matches.
    Exact,
    /// Finds semantically similar memory.
    Semantic,
    /// Traverses temporal relationships.
    Temporal,
    /// Traverses evidence-backed causal relationships.
    Causal,
    /// Finds memory associated with identified entities.
    Entity,
    /// Expands through bounded associative relationships.
    Associative,
    /// Finds relevant procedure experience.
    Procedural,
    /// Resolves the current effective state.
    CurrentState,
    /// Resolves state at a requested historical time.
    HistoricalState,
    /// Reconstructs a deeper evidence subgraph within explicit limits.
    DeepReconstruction,
}

/// 每次 Recall 请求必须携带的硬预算。
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct RecallLimits {
    /// 初始种子 Point 的最大数量。
    pub max_seeds: usize,
    /// 单个 Point 展开的最大邻居数。
    pub max_neighbors: usize,
    /// 从种子开始的最大遍历深度。
    pub max_hops: usize,
    /// 请求最多返回的 Point 数量。
    pub max_nodes: usize,
    /// 最大执行时间，单位为毫秒。
    pub timeout_ms: u64,
}
