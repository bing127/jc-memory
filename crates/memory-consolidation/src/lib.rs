//! 可恢复、受资源限制的持久化记忆巩固任务。

mod checkpoint;
mod job;
mod runner;

pub use checkpoint::BatchOutcome;
pub use job::{
    ConsolidationJobStore, JobCheckpoint, JobError, JobId, JobKind, JobRecord, JobSpec, JobStatus,
    Lease, ResourceLimits,
};
pub use runner::{BatchContext, ConsolidationTable, ConsolidationTask, DerivedMutation, JobRunner};
