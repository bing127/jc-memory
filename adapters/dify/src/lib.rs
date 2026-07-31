//! Dify Scope 与工作流结构化 Memory Packet 适配器。

use std::sync::Arc;

use jc_memory_core::{CommandBus, CommandError};
use jc_memory_protocol::{
    API_VERSION_V1, AccessContext, AccessScope, MemoryCommand, MemoryRequest, MemoryResponse,
    RecallCommand, RecallMode,
};

pub mod config;
pub mod mapping;

pub use config::{DifyConfig, DifyConfigError};
pub use mapping::{
    DifyExternalIds, DifyInvocation, DifyMappingError, DifyNodeStatus, DifyNodeType, DifyScope,
    DifyWorkflowNode,
};

/// Dify 来源保真适配器。
pub struct DifyAdapter {
    config: DifyConfig,
    bus: Option<Arc<dyn CommandBus>>,
}

impl DifyAdapter {
    /// 创建一个不带 Core Bus 的 Dify 适配器。
    #[must_use]
    pub const fn new(config: DifyConfig) -> Self {
        Self { config, bus: None }
    }

    /// 将 Dify 外部调用映射到稳定 Scope。
    ///
    /// # Errors
    ///
    /// 必填 Dify 标识为空时返回错误。
    pub fn map_scope(&self, invocation: &DifyInvocation) -> Result<DifyScope, DifyMappingError> {
        mapping::map_scope(&self.config, invocation)
    }

    /// 将 HTTP 或 MCP 节点的结构化输出映射为 Memory Packet。
    ///
    /// # Errors
    ///
    /// 节点失败、缺少输出或输出不是 Memory Packet 时返回错误。
    pub fn map_workflow_node(
        &self,
        node: DifyWorkflowNode,
    ) -> Result<jc_memory_protocol::MemoryPacket, DifyMappingError> {
        mapping::map_workflow_node(node)
    }

    /// 绑定复用 Core ACL 的命令总线。
    #[must_use]
    pub fn with_bus(mut self, bus: Arc<dyn CommandBus>) -> Self {
        self.bus = Some(bus);
        self
    }

    /// 在 Dify 映射后的精确 Scope 内执行 Core Recall。
    ///
    /// # Errors
    ///
    /// 未绑定 Bus、ACL 拒绝、Core 执行失败或响应类型不匹配时返回错误。
    pub async fn recall(
        &self,
        access: &AccessContext,
        scope: &DifyScope,
        text: &str,
        mode: RecallMode,
    ) -> Result<jc_memory_protocol::MemoryPacket, DifyRecallError> {
        let bus = self.bus.as_ref().ok_or(DifyRecallError::MissingBus)?;
        let request = MemoryRequest {
            api_version: API_VERSION_V1.to_owned(),
            request_id: access.request_id.clone(),
            access_scope: AccessScope::from(scope.scope.clone()),
            command: MemoryCommand::Recall(RecallCommand {
                mode,
                text: text.to_owned(),
                entities: Vec::new(),
                at_time: None,
                limits: self.config.recall_limits,
            }),
        };
        let envelope = bus
            .execute(access, request)
            .await
            .map_err(DifyRecallError::Command)?;
        match envelope.response {
            MemoryResponse::Recall(packet) => Ok(packet),
            response => Err(DifyRecallError::UnexpectedResponse {
                response: format!("{response:?}"),
            }),
        }
    }
}

/// Dify Recall 错误。
#[derive(Debug, thiserror::Error)]
pub enum DifyRecallError {
    /// 未绑定 Core Bus。
    #[error("Dify adapter 未绑定 Core CommandBus")]
    MissingBus,
    /// Core 命令失败。
    #[error("Dify Recall 执行失败: {0}")]
    Command(#[from] CommandError),
    /// Core 返回了非 Recall 响应。
    #[error("Dify Recall 收到非 Recall 响应: {response}")]
    UnexpectedResponse {
        /// 响应调试表示。
        response: String,
    },
}
