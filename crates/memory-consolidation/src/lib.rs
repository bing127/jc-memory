//! 可恢复、受资源限制的持久化记忆巩固任务。

mod checkpoint;
mod cold;
mod dedup;
mod edges;
mod job;
mod rebuild;
mod replay;
mod runner;
mod semantic;
mod stale;
mod surfaces;

pub use checkpoint::BatchOutcome;
pub use cold::ColdTierMovement;
pub use dedup::Deduplication;
pub use edges::{EdgeReinforcement, EdgeWeakening};
pub use job::{
    ConsolidationJobStore, JobCheckpoint, JobError, JobId, JobKind, JobRecord, JobSpec, JobStatus,
    Lease, ResourceLimits,
};
pub use rebuild::Rebuild;
pub use replay::Replay;
pub use runner::{
    ArtifactWrite, AuthorityEvent, BatchContext, ConsolidationTable, ConsolidationTask,
    DerivedMutation, EdgeWrite, JobRunner, PointWrite, SurfaceVersionWrite,
};
pub use semantic::{
    ChangeMagnitude, ConfidenceInterval, ConflictDetection, ExperienceLearningError,
    ExperienceSample, LearnedExperience, SemanticPromotion,
};
pub use stale::StaleStateDetection;
pub use surfaces::{SurfaceDiscovery, SurfaceMerge, SurfaceSplit};
