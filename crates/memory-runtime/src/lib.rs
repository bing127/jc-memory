//! 可恢复的 Agent Runtime 状态机。

mod approval;
mod capability;
mod model;
mod parser;
mod policy;
mod providers;
mod recovery;
mod runner;
mod state;
mod tool;
mod tool_loop;

pub use approval::{ApprovalDecision, ApprovalId, ApprovalRequest, ApprovalStatus, ApprovalStore};
pub use capability::{
    CapabilityDescriptor, CapabilityError, CapabilityExecutor, CapabilityKind, CapabilityRanking,
    CapabilityRegistry, CapabilityRequest, CapabilityStats, CapabilityStatsStore, Permission,
    ResolvedCapability, RiskLevel,
};
pub use model::{
    DecisionCandidate, FinalResponse, Hypothesis, ModelDescriptor, ModelError, ModelMessage,
    ModelOutput, ModelProvider, ModelRequest, ModelResponse, ModelRole, Plan, Prediction,
    ToolRequest,
};
pub use parser::{ModelParseError, parse_model_output};
pub use policy::{PolicyDecision, PolicyEngine};
pub use providers::{
    GenericHttpConfig, GenericHttpProvider, OpenAiAuth, OpenAiCompatibleConfig,
    OpenAiCompatibleProvider,
};
pub use recovery::RuntimeRecovery;
pub use runner::{RunId, RuntimeError, RuntimeRunner, RuntimeSnapshot, TransitionMetadata};
pub use state::{RuntimeEvent, RuntimeState, TransitionError};
pub use tool::{
    ResolvedTool, ToolDescriptor, ToolError, ToolExecutor, ToolIdempotency, ToolRegistry,
};
pub use tool_loop::{
    ActionRecord, OutcomeRecord, PredictionRelation, ToolCommit, ToolInvocation, ToolLoop,
    ToolResultRecord,
};
