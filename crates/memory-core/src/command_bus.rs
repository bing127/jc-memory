//! 所有 Embedded 与远程服务复用的 Core 命令入口。

use async_trait::async_trait;
use jc_memory_activation::{RecallLimits as ActivationRecallLimits, RecallQuery};
use jc_memory_protocol::{
    AccessContext, CommandErrorCode, MemoryCommand, MemoryRequest, MemoryResponse, ObserveResult,
    RequestValidationError, ResponseEnvelope,
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
            MemoryCommand::Observe(_) | MemoryCommand::Recall(_) => {}
        }

        let response = match request.command.clone() {
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
            MemoryCommand::Recall(command) => {
                let query = RecallQuery {
                    scope: request_scope,
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
                };
                MemoryResponse::Recall(
                    self.engine
                        .recall(access, query)
                        .await
                        .map_err(CommandError::engine)?,
                )
            }
        };
        Ok(ResponseEnvelope::for_request(&request, response))
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
}

impl CommandError {
    /// 返回跨 Embedded 与远程边界稳定的错误分类。
    #[must_use]
    pub const fn code(&self) -> CommandErrorCode {
        match self {
            Self::InvalidRequest { code, .. } | Self::Engine { code, .. } => *code,
            Self::RequestIdMismatch | Self::AccessScopeMismatch => {
                CommandErrorCode::InvalidArgument
            }
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
            MemoryError::Busy { .. } => CommandErrorCode::ResourceExhausted,
            MemoryError::Event(_)
            | MemoryError::Identifier(_)
            | MemoryError::Edge(_)
            | MemoryError::ReviewTargetNotQuarantined { .. }
            | MemoryError::EmptyReviewReason => CommandErrorCode::InvalidArgument,
            MemoryError::DataDirectory(_)
            | MemoryError::Blob(_)
            | MemoryError::Corrupt { .. }
            | MemoryError::Storage(_)
            | MemoryError::Recovery(_)
            | MemoryError::Recall(_)
            | MemoryError::Packet(_)
            | MemoryError::SecurityInvariantViolation(_) => CommandErrorCode::Internal,
        };
        Self::Engine { code, source }
    }
}
