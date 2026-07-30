//! Embedded Jiangcheng Memory facade.

mod commands;
mod config;
mod engine;
mod health;
mod mutation;
mod quarantine;
mod recall;
mod security;

pub use commands::{
    MutationCommand, MutationKind, MutationReceipt, QuarantineReview, QuarantineReviewReceipt,
    ReviewDecision,
};
pub use config::MemoryConfig;
pub use engine::{MemoryEngine, MemoryError, ObserveReceipt};
pub use health::{HealthReport, HealthStatus};
pub use jc_memory_activation::{ExplainTrace, RecallLimits, RecallQuery};
pub use jc_memory_protocol::{AccessContext, MemoryPacket};
pub use recall::MemoryStats;
