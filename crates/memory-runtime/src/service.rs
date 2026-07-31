//! 将 Runtime 组件编排为一个可恢复的单次运行服务。

use std::{fmt, sync::Arc};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use jc_memory_protocol::{AccessContext, EvidenceRef, MemoryScope, TraceId};
use semver::VersionReq;
use serde_json::json;
use tokio_util::sync::CancellationToken;

use crate::{
    FinalResponse, ModelError, ModelMessage, ModelOutput, ModelProvider, ModelRequest,
    RuntimeError, RuntimeEvent, RuntimeRunner, RuntimeSnapshot, ToolCommit, ToolError,
    ToolInvocation, ToolLoop,
};

/// Runtime 编排所需的记忆后端。
#[async_trait]
pub trait RuntimeBackend: Send + Sync {
    /// 记录原始输入；返回可用于 Runtime 转换审计的权威证据引用。
    async fn observe(
        &self,
        access: &AccessContext,
        scope: &MemoryScope,
        input: &str,
    ) -> Result<Vec<EvidenceRef>, String>;

    /// 执行受 Scope 约束的 Recall，并返回权威证据引用。
    async fn recall(
        &self,
        access: &AccessContext,
        scope: &MemoryScope,
        input: &str,
    ) -> Result<Vec<EvidenceRef>, String>;

    /// 提交工具结果或最终响应；实现不得把模型候选直接写成 Fact。
    async fn commit(
        &self,
        access: &AccessContext,
        scope: &MemoryScope,
        commit: RuntimeCommit,
    ) -> Result<Vec<EvidenceRef>, String>;
}

/// 可提交的 Runtime 结果。
#[derive(Clone, Debug)]
pub enum RuntimeCommit {
    /// 工具执行后的结构化结果。
    Tool(ToolCommit),
    /// 模型生成的最终响应，仍由后端决定如何作为权威记录保存。
    Final(FinalResponse),
}

/// 一次 Runtime 运行请求。
#[derive(Clone, Debug)]
pub struct RuntimeRequest {
    /// 运行所属的精确 Scope。
    pub scope: MemoryScope,
    /// 用户原始输入。
    pub prompt: String,
    /// 所有阶段共用的截止时间。
    pub deadline: DateTime<Utc>,
    /// 用于取消模型和工具调用。
    pub cancellation: CancellationToken,
}

/// Runtime 编排结果。
#[derive(Clone, Debug)]
pub enum RuntimeResult {
    /// 已提交并进入 Completed。
    Completed {
        /// 最终模型响应；工具结果不自动升级为 Fact。
        response: FinalResponse,
        /// 最近一次已提交快照。
        snapshot: RuntimeSnapshot,
    },
    /// 工具需要人工审批，尚未执行副作用。
    AwaitingApproval {
        /// 已进入 `AwaitingApproval` 的快照。
        snapshot: RuntimeSnapshot,
        /// 审批记录标识。
        approval_id: uuid::Uuid,
        /// 待审批的原始工具请求。
        invocation: ToolInvocation,
    },
}

/// Runtime 编排错误。
#[derive(Debug, thiserror::Error)]
pub enum RuntimeServiceError {
    /// 状态机或持久化失败。
    #[error(transparent)]
    Runtime(#[from] RuntimeError),
    /// 模型调用失败。
    #[error(transparent)]
    Model(#[from] ModelError),
    /// 工具策略、审批或执行失败。
    #[error(transparent)]
    Tool(#[from] ToolError),
    /// 记忆后端失败。
    #[error("runtime backend failure: {0}")]
    Backend(String),
}

/// 负责串联 Runtime 各阶段的服务门面。
pub struct RuntimeService {
    runner: RuntimeRunner,
    provider: Arc<dyn ModelProvider>,
    backend: Arc<dyn RuntimeBackend>,
    tools: ToolLoop,
}

impl fmt::Debug for RuntimeService {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RuntimeService")
            .field("runner", &self.runner)
            .finish_non_exhaustive()
    }
}

impl RuntimeService {
    /// 创建一个使用持久化 Runner、可替换模型和记忆后端的 Runtime 服务。
    #[must_use]
    pub fn new(
        runner: RuntimeRunner,
        provider: Arc<dyn ModelProvider>,
        backend: Arc<dyn RuntimeBackend>,
        tools: ToolLoop,
    ) -> Self {
        Self {
            runner,
            provider,
            backend,
            tools,
        }
    }

    /// 执行一次从 Observe 到 Completed 或 `AwaitingApproval` 的完整编排。
    ///
    /// 模型只产生候选输出；只有工具闭环或最终响应经过后端提交后，运行才会进入完成态。
    ///
    /// # Errors
    ///
    /// 任一阶段失败时返回错误；状态机已提交的历史仍保留，可由 `RuntimeRecovery` 读取。
    pub async fn run(
        &self,
        access: &AccessContext,
        request: RuntimeRequest,
    ) -> Result<RuntimeResult, RuntimeServiceError> {
        let trace_id = TraceId::new();
        let mut snapshot = self.runner.start(access, &request.scope).await?;
        let observed = self
            .backend
            .observe(access, &request.scope, &request.prompt)
            .await
            .map_err(RuntimeServiceError::Backend)?;
        snapshot = self
            .transition(
                access,
                snapshot,
                TransitionInput {
                    event: RuntimeEvent::Recall,
                    deadline: request.deadline,
                    trace_id,
                    input_refs: Vec::new(),
                    output_refs: observed,
                    phase: "observe",
                },
            )
            .await?;
        let recalled = self
            .backend
            .recall(access, &request.scope, &request.prompt)
            .await
            .map_err(RuntimeServiceError::Backend)?;
        snapshot = self
            .transition(
                access,
                snapshot,
                TransitionInput {
                    event: RuntimeEvent::Reason,
                    deadline: request.deadline,
                    trace_id,
                    input_refs: Vec::new(),
                    output_refs: recalled,
                    phase: "recall",
                },
            )
            .await?;

        let response = self
            .provider
            .invoke(
                ModelRequest {
                    messages: vec![ModelMessage::user(request.prompt.clone())],
                    stream: false,
                },
                request.cancellation.clone(),
            )
            .await?;
        match response.output {
            ModelOutput::FinalResponse(response) => {
                self.complete_final(
                    access,
                    request.scope,
                    request.deadline,
                    trace_id,
                    snapshot,
                    response,
                )
                .await
            }
            ModelOutput::ToolRequest(tool) => {
                let invocation = ToolInvocation {
                    id: tool.capability_id,
                    version: VersionReq::STAR,
                    arguments: tool.arguments,
                    prediction: None,
                };
                self.execute_tool(access, request, trace_id, snapshot, invocation)
                    .await
            }
            _ => Err(RuntimeServiceError::Backend(
                "当前服务只接受最终响应或工具请求作为可执行模型输出".to_owned(),
            )),
        }
    }

    async fn complete_final(
        &self,
        access: &AccessContext,
        scope: MemoryScope,
        deadline: DateTime<Utc>,
        trace_id: TraceId,
        mut snapshot: RuntimeSnapshot,
        response: FinalResponse,
    ) -> Result<RuntimeResult, RuntimeServiceError> {
        snapshot = self
            .transition(
                access,
                snapshot,
                TransitionInput {
                    event: RuntimeEvent::Execute,
                    deadline,
                    trace_id,
                    input_refs: Vec::new(),
                    output_refs: Vec::new(),
                    phase: "model_final",
                },
            )
            .await?;
        snapshot = self
            .transition(
                access,
                snapshot,
                TransitionInput {
                    event: RuntimeEvent::Commit,
                    deadline,
                    trace_id,
                    input_refs: Vec::new(),
                    output_refs: Vec::new(),
                    phase: "capture_final",
                },
            )
            .await?;
        let output_refs = self
            .backend
            .commit(access, &scope, RuntimeCommit::Final(response.clone()))
            .await
            .map_err(RuntimeServiceError::Backend)?;
        snapshot = self
            .transition(
                access,
                snapshot,
                TransitionInput {
                    event: RuntimeEvent::Complete,
                    deadline,
                    trace_id,
                    input_refs: Vec::new(),
                    output_refs,
                    phase: "commit",
                },
            )
            .await?;
        Ok(RuntimeResult::Completed { response, snapshot })
    }

    async fn execute_tool(
        &self,
        access: &AccessContext,
        request: RuntimeRequest,
        trace_id: TraceId,
        mut snapshot: RuntimeSnapshot,
        invocation: ToolInvocation,
    ) -> Result<RuntimeResult, RuntimeServiceError> {
        match self
            .tools
            .execute(access, invocation.clone(), request.cancellation)
            .await
        {
            Ok(commit) => {
                snapshot = self
                    .transition(
                        access,
                        snapshot,
                        TransitionInput {
                            event: RuntimeEvent::Execute,
                            deadline: request.deadline,
                            trace_id,
                            input_refs: Vec::new(),
                            output_refs: Vec::new(),
                            phase: "tool_execute",
                        },
                    )
                    .await?;
                snapshot = self
                    .transition(
                        access,
                        snapshot,
                        TransitionInput {
                            event: RuntimeEvent::Commit,
                            deadline: request.deadline,
                            trace_id,
                            input_refs: Vec::new(),
                            output_refs: Vec::new(),
                            phase: "tool_capture",
                        },
                    )
                    .await?;
                let output_refs = self
                    .backend
                    .commit(access, &request.scope, RuntimeCommit::Tool(commit))
                    .await
                    .map_err(RuntimeServiceError::Backend)?;
                snapshot = self
                    .transition(
                        access,
                        snapshot,
                        TransitionInput {
                            event: RuntimeEvent::Complete,
                            deadline: request.deadline,
                            trace_id,
                            input_refs: Vec::new(),
                            output_refs,
                            phase: "commit",
                        },
                    )
                    .await?;
                Ok(RuntimeResult::Completed {
                    response: FinalResponse {
                        text: "工具执行已提交".to_owned(),
                    },
                    snapshot,
                })
            }
            Err(ToolError::ApprovalRequired(approval_id)) => {
                snapshot = self
                    .transition(
                        access,
                        snapshot,
                        TransitionInput {
                            event: RuntimeEvent::RequestApproval,
                            deadline: request.deadline,
                            trace_id,
                            input_refs: Vec::new(),
                            output_refs: Vec::new(),
                            phase: "approval",
                        },
                    )
                    .await?;
                Ok(RuntimeResult::AwaitingApproval {
                    snapshot,
                    approval_id,
                    invocation,
                })
            }
            Err(error) => {
                self.transition(
                    access,
                    snapshot,
                    TransitionInput {
                        event: RuntimeEvent::Fail,
                        deadline: request.deadline,
                        trace_id,
                        input_refs: Vec::new(),
                        output_refs: Vec::new(),
                        phase: "tool_failed",
                    },
                )
                .await?;
                Err(error.into())
            }
        }
    }

    async fn transition(
        &self,
        access: &AccessContext,
        snapshot: RuntimeSnapshot,
        input: TransitionInput,
    ) -> Result<RuntimeSnapshot, RuntimeServiceError> {
        Ok(self
            .runner
            .transition(
                access,
                &snapshot,
                input.event,
                &crate::TransitionMetadata {
                    input_refs: input.input_refs,
                    output_refs: input.output_refs,
                    attempt: 1,
                    deadline: input.deadline,
                    trace_id: input.trace_id,
                    payload: json!({"phase": input.phase}),
                },
            )
            .await?)
    }
}

struct TransitionInput {
    event: RuntimeEvent,
    deadline: DateTime<Utc>,
    trace_id: TraceId,
    input_refs: Vec<EvidenceRef>,
    output_refs: Vec<EvidenceRef>,
    phase: &'static str,
}
