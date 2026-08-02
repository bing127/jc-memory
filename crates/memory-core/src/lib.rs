//! Embedded Jiangcheng Memory facade.

mod command_bus;
mod commands;
mod config;
mod engine;
mod health;
mod maintenance;
mod mutation;
mod projector;
mod quarantine;
mod recall;
mod security;

pub use command_bus::{CommandBus, CommandError, EmbeddedCommandBus};
pub use commands::{
    MutationCommand, MutationKind, MutationReceipt, QuarantineReview, QuarantineReviewReceipt,
    ReviewDecision,
};
pub use config::MemoryConfig;
pub use engine::{MemoryEngine, MemoryError, ObserveReceipt};
pub use health::{HealthReport, HealthStatus};
pub use jc_memory_activation::{ExplainTrace, RecallLimits, RecallQuery};
pub use jc_memory_protocol::{AccessContext, MemoryPacket};
pub use jc_memory_storage::{
    DerivedRebuildReport, RestoreReceipt, SnapshotManifest, SnapshotReceipt,
};
pub use recall::MemoryStats;
