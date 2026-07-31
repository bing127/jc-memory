//! 可恢复的 Agent Runtime 状态机。

mod model;
mod parser;
mod providers;
mod recovery;
mod runner;
mod state;

pub use model::{
    DecisionCandidate, FinalResponse, Hypothesis, ModelDescriptor, ModelError, ModelMessage,
    ModelOutput, ModelProvider, ModelRequest, ModelResponse, ModelRole, Plan, Prediction,
    ToolRequest,
};
pub use parser::{ModelParseError, parse_model_output};
pub use providers::{
    GenericHttpConfig, GenericHttpProvider, OpenAiAuth, OpenAiCompatibleConfig,
    OpenAiCompatibleProvider,
};
pub use recovery::RuntimeRecovery;
pub use runner::{RunId, RuntimeError, RuntimeRunner, RuntimeSnapshot, TransitionMetadata};
pub use state::{RuntimeEvent, RuntimeState, TransitionError};
