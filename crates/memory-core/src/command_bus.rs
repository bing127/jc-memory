//! 所有 Embedded 与远程服务复用的 Core 命令入口。

use async_trait::async_trait;
use jc_memory_activation::{ExplainTrace, RecallLimits as ActivationRecallLimits, RecallQuery};
use jc_memory_protocol::{
    AccessContext, CommandErrorCode, CommitResult, ExplainResult, ExplainSeed, ExplainSurface,
    ExplainTraversal, MemoryCommand, MemoryRequest, MemoryResponse, MutationAction, MutationResult,
    ObserveResult, RequestValidationError, ResponseEnvelope, StatsResult,
};

use crate::{MemoryEngine, MemoryError};

/// 异步命令执行边界；实现不得绕过 Core 的 `AccessContext` 与 ACL。
#[async_trait]
pub trait CommandBus: Send + Sync {
    /// 执行一个已携带稳定 Envelope 的业务命令。
    ///
    /// # Errors
    ///
    /// Envelope、调用者上下文或 Core 执行失败时返回稳定映射错误。
    async fn execute(
        &self,
        access: &AccessContext,
        request: MemoryRequest,
    ) -> Result<ResponseEnvelope, CommandError>;

    /// 停止接收业务请求后释放持久化资源。
    ///
    /// # Errors
    ///
    /// 实现无法安全释放资源时返回稳定错误。
    async fn shutdown(&self) -> Result<(), CommandError> {
        Ok(())
    }
}

/// 直接复用 [`MemoryEngine`] 的进程内 `CommandBus`。
#[derive(Clone)]
pub struct EmbeddedCommandBus {
    engine: MemoryEngine,
}

impl EmbeddedCommandBus {
    /// 创建共享给 Embedded、Sidecar 和 Server Adapter 的 Core 命令入口。
    #[must_use]
    pub const fn new(engine: MemoryEngine) -> Self {
        Self { engine }
    }
}

#[async_trait]
impl CommandBus for EmbeddedCommandBus {
    async fn execute(
        &self,
        access: &AccessContext,
        request: MemoryRequest,
    ) -> Result<ResponseEnvelope, CommandError> {
        request.validate().map_err(CommandError::invalid_request)?;
        if access.request_id != request.request_id {
            return Err(CommandError::RequestIdMismatch);
        }
        let request_scope = request.access_scope.clone().into();
        if access.scope != request_scope {
            return Err(CommandError::AccessScopeMismatch);
        }
        match &request.command {
            MemoryCommand::Observe(command) if command.fragment.scope != request_scope => {
                return Err(CommandError::AccessScopeMismatch);
            }
            MemoryCommand::Commit(command)
                if command
                    .fragments
                    .iter()
                    .any(|fragment| fragment.scope != request_scope) =>
            {
                return Err(CommandError::AccessScopeMismatch);
            }
            MemoryCommand::Observe(_)
            | MemoryCommand::Commit(_)
            | MemoryCommand::Recall(_)
            | MemoryCommand::Stats(_)
            | MemoryCommand::Correct(_)
            | MemoryCommand::Forget(_)
            | MemoryCommand::Explain(_) => {}
        }

        let response = self
            .execute_command(access, request_scope, request.command.clone())
            .await?;
        Ok(ResponseEnvelope::for_request(&request, response))
    }

    async fn shutdown(&self) -> Result<(), CommandError> {
        self.engine.close().await;
        Ok(())
    }
}

impl EmbeddedCommandBus {
    async fn execute_command(
        &self,
        access: &AccessContext,
        request_scope: jc_memory_protocol::MemoryScope,
        command: MemoryCommand,
    ) -> Result<MemoryResponse, CommandError> {
        let response = match command {
            MemoryCommand::Observe(command) => {
                let receipt = self
                    .engine
                    .observe(access, command.fragment)
                    .await
                    .map_err(CommandError::engine)?;
                MemoryResponse::Observe(ObserveResult {
                    fragment_id: receipt.fragment_id,
                    event_id: receipt.event_id,
                    sequence: receipt.sequence,
                    content_hash: receipt.content_hash,
                    created: receipt.created,
                    quarantined: receipt.quarantined,
                })
            }
            MemoryCommand::Commit(command) => {
                let receipts = self
                    .engine
                    .commit(access, command.fragments)
                    .await
                    .map_err(CommandError::engine)?
                    .into_iter()
                    .map(|receipt| ObserveResult {
                        fragment_id: receipt.fragment_id,
                        event_id: receipt.event_id,
                        sequence: receipt.sequence,
                        content_hash: receipt.content_hash,
                        created: receipt.created,
                        quarantined: receipt.quarantined,
                    })
                    .collect();
                MemoryResponse::Commit(CommitResult { receipts })
            }
            MemoryCommand::Recall(command) => {
                let query = recall_query(request_scope, command);
                MemoryResponse::Recall(
                    self.engine
                        .recall(access, query)
                        .await
                        .map_err(CommandError::engine)?,
                )
            }
            MemoryCommand::Stats(_) => {
                let stats = self
                    .engine
                    .stats(access, &request_scope)
                    .await
                    .map_err(CommandError::engine)?;
                MemoryResponse::Stats(StatsResult {
                    scope: stats.scope.into(),
                    fragment_count: stats.fragment_count,
                    event_count: stats.event_count,
                })
            }
            MemoryCommand::Correct(command) => {
                let receipt = self
                    .engine
                    .correct(access, core_mutation(command))
                    .await
                    .map_err(CommandError::engine)?;
                MemoryResponse::Mutation(mutation_result(&receipt, MutationAction::Correct))
            }
            MemoryCommand::Forget(command) => {
                let receipt = self
                    .engine
                    .forget(access, core_mutation(command))
                    .await
                    .map_err(CommandError::engine)?;
                MemoryResponse::Mutation(mutation_result(&receipt, MutationAction::Forget))
            }
            MemoryCommand::Explain(command) => {
                let trace = self
                    .engine
                    .explain(access, recall_query(request_scope, command))
                    .await
                    .map_err(CommandError::engine)?;
                MemoryResponse::Explain(explain_result(trace))
            }
        };
        Ok(response)
    }
}

fn recall_query(
    scope: jc_memory_protocol::MemoryScope,
    command: jc_memory_protocol::RecallCommand,
) -> RecallQuery {
    RecallQuery {
        scope,
        mode: command.mode,
        text: command.text,
        entities: command.entities,
        at_time: command.at_time,
        limits: ActivationRecallLimits {
            max_seeds: command.limits.max_seeds,
            max_neighbors: command.limits.max_neighbors,
            max_hops: command.limits.max_hops,
            max_nodes: command.limits.max_nodes,
            timeout_ms: command.limits.timeout_ms,
        },
    }
}

fn explain_result(trace: ExplainTrace) -> ExplainResult {
    ExplainResult {
        seeds: trace
            .seeds
            .into_iter()
            .map(|seed| ExplainSeed {
                point_id: seed.point_id,
                score: seed.score,
            })
            .collect(),
        traversals: trace
            .traversals
            .into_iter()
            .map(|traversal| ExplainTraversal {
                edge_id: traversal.edge_id,
                source_point_id: traversal.source_point_id,
                target_point_id: traversal.target_point_id,
                kind: traversal.kind,
                score: traversal.score,
                depth: traversal.depth,
            })
            .collect(),
        surfaces: trace
            .surfaces
            .into_iter()
            .map(|surface| ExplainSurface {
                surface_id: surface.surface_id,
                version: surface.version,
                activation: surface.activation,
            })
            .collect(),
        inferred_edges: trace.inferred_edges,
        verified_fragments: trace.verified_fragments,
        limits: jc_memory_protocol::RecallLimits {
            max_seeds: trace.limits.max_seeds,
            max_neighbors: trace.limits.max_neighbors,
            max_hops: trace.limits.max_hops,
            max_nodes: trace.limits.max_nodes,
            timeout_ms: trace.limits.timeout_ms,
        },
        truncated: trace.truncated,
    }
}

fn core_mutation(command: jc_memory_protocol::MutationCommand) -> crate::MutationCommand {
    crate::MutationCommand {
        target_fragment_id: command.target_fragment_id,
        content: command.content,
        state: command.state,
        source: command.source,
        perspective: command.perspective,
        occurred_at: command.occurred_at,
        verification_method: command.verification_method,
        reason: command.reason,
    }
}

fn mutation_result(receipt: &crate::MutationReceipt, action: MutationAction) -> MutationResult {
    MutationResult {
        action,
        target_fragment_id: receipt.target_fragment_id,
        fragment_id: receipt.fragment_id,
        event_id: receipt.event_id,
        sequence: receipt.sequence,
        relation: receipt.relation,
    }
}

/// `CommandBus` 对外暴露的稳定错误与完整源错误链。
#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    /// 请求 Envelope 未通过协议校验。
    #[error("invalid command request: {source}")]
    InvalidRequest {
        /// 稳定公开错误码。
        code: CommandErrorCode,
        /// 原始协议校验错误。
        #[source]
        source: RequestValidationError,
    },
    /// Envelope 的 `request_id` 与服务端构造的 `AccessContext` 不一致。
    #[error("request_id does not match the authorized access context")]
    RequestIdMismatch,
    /// Envelope 的 `access_scope` 与服务端构造的 `AccessContext` 不一致。
    #[error("access_scope does not match the authorized access context")]
    AccessScopeMismatch,
    /// Core 执行失败。
    #[error("command execution failed: {source}")]
    Engine {
        /// 由完整 Core 错误确定性映射的公开错误码。
        code: CommandErrorCode,
        /// 未丢失的原始 Core 错误。
        #[source]
        source: MemoryError,
    },
    /// 共享 `CommandBus` 下游传输失败。
    #[error("command transport failed ({code:?}): {detail}")]
    Transport {
        /// 稳定公开错误码。
        code: CommandErrorCode,
        /// 不包含秘密的诊断细节。
        detail: String,
    },
}

impl CommandError {
    /// 返回跨 Embedded 与远程边界稳定的错误分类。
    #[must_use]
    pub const fn code(&self) -> CommandErrorCode {
        match self {
            Self::InvalidRequest { code, .. }
            | Self::Engine { code, .. }
            | Self::Transport { code, .. } => *code,
            Self::RequestIdMismatch | Self::AccessScopeMismatch => {
                CommandErrorCode::InvalidArgument
            }
        }
    }

    /// 创建一个带稳定分类的下游传输错误。
    #[must_use]
    pub fn transport(code: CommandErrorCode, detail: impl Into<String>) -> Self {
        Self::Transport {
            code,
            detail: detail.into(),
        }
    }

    fn invalid_request(source: RequestValidationError) -> Self {
        Self::InvalidRequest {
            code: CommandErrorCode::InvalidArgument,
            source,
        }
    }

    fn engine(source: MemoryError) -> Self {
        let code = match &source {
            MemoryError::AccessDenied { .. } => CommandErrorCode::PermissionDenied,
            MemoryError::NotFound { .. } => CommandErrorCode::NotFound,
            MemoryError::Conflict { .. } | MemoryError::Constraint { .. } => {
                CommandErrorCode::Conflict
            }
            MemoryError::Busy { .. } | MemoryError::ProjectionTimedOut => {
                CommandErrorCode::ResourceExhausted
            }
            MemoryError::Event(_)
            | MemoryError::Identifier(_)
            | MemoryError::Edge(_)
            | MemoryError::RetrievalPlan(_)
            | MemoryError::RetrievalConfig(_)
            | MemoryError::InvalidStateMutation { .. }
            | MemoryError::ReviewTargetNotQuarantined { .. }
            | MemoryError::EmptyReviewReason => CommandErrorCode::InvalidArgument,
            MemoryError::DataDirectory(_)
            | MemoryError::Blob(_)
            | MemoryError::Corrupt { .. }
            | MemoryError::Storage(_)
            | MemoryError::Recovery(_)
            | MemoryError::Recall(_)
            | MemoryError::RetrievalDictionary(_)
            | MemoryError::QuarantinePolicy(_)
            | MemoryError::QuarantinePolicyFile { .. }
            | MemoryError::RetrievalDictionaryFile { .. }
            | MemoryError::RetrievalFusion(_)
            | MemoryError::Packet(_)
            | MemoryError::ProjectionCheckpointCorrupt { .. }
            | MemoryError::ProjectionScopeMismatch { .. }
            | MemoryError::StateChainCorrupt { .. }
            | MemoryError::Consolidation(_)
            | MemoryError::SecurityInvariantViolation(_) => CommandErrorCode::Internal,
        };
        Self::Engine { code, source }
    }
}

#[cfg(test)]
mod projection_error_tests {
    use jc_memory_consolidation::JobError;
    use jc_memory_protocol::CommandErrorCode;
    use jc_memory_storage::{ProjectorKind, StorageError};

    use super::CommandError;
    use crate::MemoryError;

    #[test]
    fn projection_errors_have_stable_safe_command_codes() {
        assert_mapping(
            MemoryError::ProjectionTimedOut,
            CommandErrorCode::ResourceExhausted,
        );
        assert_mapping(
            MemoryError::ProjectionCheckpointCorrupt {
                source: StorageError::ProjectionCheckpointCorrupt {
                    projector: ProjectorKind::State,
                    scope_hash: "scope-blue_green".to_owned(),
                },
            },
            CommandErrorCode::Internal,
        );
        assert_mapping(
            MemoryError::ProjectionScopeMismatch {
                source: StorageError::ProjectionScopeMismatch {
                    projector: ProjectorKind::Graph,
                    scope_hash: "scope-canary".to_owned(),
                },
            },
            CommandErrorCode::Internal,
        );
        assert_mapping(
            MemoryError::StateChainCorrupt {
                source: StorageError::StateChainCorrupt {
                    field: "current_version_id",
                    scope_hash: "fragment-secret-content".to_owned(),
                },
            },
            CommandErrorCode::Internal,
        );
    }

    #[test]
    fn consolidation_errors_are_not_exposed_as_existing_remote_commands() {
        assert_mapping(
            MemoryError::Consolidation(JobError::InvalidSpec("max_batch_size")),
            CommandErrorCode::Internal,
        );
    }

    fn assert_mapping(source: MemoryError, expected: CommandErrorCode) {
        let error = CommandError::engine(source);
        assert_eq!(error.code(), expected);
        let public = error.to_string();
        for secret in [
            "blue_green",
            "canary",
            "fragment-secret-content",
            "test-api-key",
        ] {
            assert!(!public.contains(secret));
        }
    }
}
