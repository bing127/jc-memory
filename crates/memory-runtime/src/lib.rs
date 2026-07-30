//! 可恢复的 Agent Runtime 状态机。

mod recovery;
mod runner;
mod state;

pub use recovery::RuntimeRecovery;
pub use runner::{RunId, RuntimeError, RuntimeRunner, RuntimeSnapshot, TransitionMetadata};
pub use state::{RuntimeEvent, RuntimeState, TransitionError};
