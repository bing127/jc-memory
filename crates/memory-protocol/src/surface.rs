//! Cognitive surface wire types.

use serde::{Deserialize, Serialize};

/// Origin and intended lifetime of a cognitive surface.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceKind {
    /// A surface explicitly defined by a user or trusted system rule.
    Explicit,
    /// A surface maintained incrementally from observed relationships.
    Dynamic,
    /// A short-lived surface scoped to one task.
    TemporaryTask,
}

/// Lifecycle state of a cognitive surface version.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceStatus {
    /// Available for membership updates and recall.
    Active,
    /// Readable but not accepting automatic membership updates.
    Frozen,
    /// Excluded from normal recall.
    Inactive,
    /// Replaced by two or more successor surfaces.
    Split,
    /// Absorbed into another surface.
    Merged,
}
