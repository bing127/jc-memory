//! Embedded Jiangcheng Memory facade.

mod config;
mod engine;
mod health;

pub use config::MemoryConfig;
pub use engine::{MemoryEngine, MemoryError, ObserveReceipt};
pub use health::{HealthReport, HealthStatus};
