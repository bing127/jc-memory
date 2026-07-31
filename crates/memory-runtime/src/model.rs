//! 模型中立的 Provider 与输出候选契约。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

/// 模型消息角色。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelRole {
    /// 系统约束。
    System,
    /// 用户输入。
    User,
    /// 助手历史输出。
    Assistant,
}

/// 与具体模型供应商无关的消息。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelMessage {
    /// 消息角色。
    pub role: ModelRole,
    /// 原始文本内容。
    pub content: String,
}

impl ModelMessage {
    /// 创建用户消息。
    #[must_use]
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: ModelRole::User,
            content: content.into(),
        }
    }
}

/// 一次模型调用请求。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelRequest {
    /// 按顺序提供给模型的消息。
    pub messages: Vec<ModelMessage>,
    /// 是否请求流式响应。
    pub stream: bool,
}

/// Provider 的稳定能力描述。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelDescriptor {
    /// Provider 类型。
    pub provider: String,
    /// 模型标识。
    pub model: String,
    /// 是否支持流式输出。
    pub supports_streaming: bool,
}

/// 尚待验证的假设。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Hypothesis {
    /// 假设陈述。
    pub statement: String,
    /// 可选推理依据，不代表已验证证据。
    pub rationale: Option<String>,
}

/// 尚待实际结果验证的预测。
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Prediction {
    /// 预测陈述。
    pub statement: String,
    /// 模型自报置信度，范围为 `0.0..=1.0`。
    pub confidence: f64,
}

/// 候选执行计划。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    /// 计划目标。
    pub goal: String,
    /// 有序候选步骤。
    pub steps: Vec<String>,
}

/// 等待 Policy 与 Approval 决定的候选决策。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DecisionCandidate {
    /// 候选决策。
    pub decision: String,
    /// 候选理由。
    pub rationale: String,
}

/// 尚未执行的工具请求。
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolRequest {
    /// 目标 Capability 标识。
    pub capability_id: String,
    /// 传给 Capability 的结构化参数。
    pub arguments: Value,
}

/// 面向调用方的最终自然语言响应。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FinalResponse {
    /// 响应正文。
    pub text: String,
}

/// 严格区分事实之外的六类模型输出。
#[derive(Clone, Debug, PartialEq)]
pub enum ModelOutput {
    /// 尚待验证的假设。
    Hypothesis(Hypothesis),
    /// 尚待结果验证的预测。
    Prediction(Prediction),
    /// 候选计划。
    Plan(Plan),
    /// 候选决策。
    DecisionCandidate(DecisionCandidate),
    /// 尚未执行的工具请求。
    ToolRequest(ToolRequest),
    /// 最终自然语言响应。
    FinalResponse(FinalResponse),
}

/// 一次成功调用的结构化结果。
#[derive(Clone, Debug, PartialEq)]
pub struct ModelResponse {
    /// 经过严格解析的候选输出。
    pub output: ModelOutput,
    /// Provider 返回的原始模型文本。
    pub raw_text: String,
}

/// 模型调用失败。
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ModelError {
    /// 调用被取消。
    #[error("model invocation cancelled")]
    Cancelled,
    /// 调用超过配置的截止时间。
    #[error("model invocation timed out")]
    Timeout,
    /// Provider 传输失败。
    #[error("model transport failure: {0}")]
    Transport(String),
    /// Provider 返回可重试或终止的 HTTP 状态。
    #[error("model HTTP status {0}")]
    HttpStatus(u16),
    /// Provider 响应结构不符合其协议。
    #[error("invalid model provider response: {0}")]
    InvalidResponse(String),
    /// 模型输出无法通过严格解析。
    #[error(transparent)]
    Parse(#[from] crate::ModelParseError),
}

/// 可替换的异步模型 Provider。
#[async_trait]
pub trait ModelProvider: Send + Sync {
    /// 执行一次支持取消的模型调用。
    async fn invoke(
        &self,
        request: ModelRequest,
        cancellation: CancellationToken,
    ) -> Result<ModelResponse, ModelError>;

    /// 返回 Provider 和模型能力描述。
    fn descriptor(&self) -> ModelDescriptor;
}
