//! `OpenClaw` Memory.md 到疆诚记忆 Core 的来源保真适配器。

pub mod config;
pub mod import;
pub mod mapping;

use std::sync::Arc;

use chrono::{DateTime, Utc};
use jc_memory_core::{CommandBus, CommandError};
use jc_memory_protocol::{
    API_VERSION_V1, AccessContext, AccessScope, CommitCommand, MemoryCommand, MemoryPacket,
    MemoryRequest, MemoryResponse, RecallCommand, RecallMode, ResponseEnvelope,
};

pub use config::OpenClawConfig;
pub use import::{OpenClawImportEntry, OpenClawImportReceipt};
use mapping::map_document;

/// 待导入的 `OpenClaw` Memory.md 文档。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpenClawDocument {
    /// 来源系统中的文件路径。
    pub path: String,
    /// 未改写的 Markdown 原文。
    pub content: String,
    /// 来源文件最后修改时间。
    pub modified_at: DateTime<Utc>,
}

/// `OpenClaw` Hook 或 MCP 发起的 Recall 请求。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpenClawRecallQuery {
    /// 原始检索文本。
    pub text: String,
    /// 需要使用的 Recall 模式。
    pub mode: RecallMode,
}

/// `OpenClaw` Adapter 错误。
#[derive(Debug, thiserror::Error)]
pub enum OpenClawError {
    /// 下游 `CommandBus` 失败。
    #[error("command bus failure: {0}")]
    Command(#[from] CommandError),
    /// 配置不满足适配器边界。
    #[error("invalid OpenClaw adapter configuration: {0}")]
    InvalidConfig(String),
    /// 文档路径或内容为空。
    #[error("OpenClaw Memory.md path and content must not be empty")]
    EmptyDocument,
    /// 确定性 Fragment ID 无法构造。
    #[error(transparent)]
    Identifier(#[from] jc_memory_protocol::IdParseError),
    /// Core 返回了不匹配的响应。
    #[error("unexpected command response from memory core")]
    UnexpectedResponse,
}

/// 仅通过 Core `CommandBus` 提供导入、Hook 与 MCP Recall 的 Adapter。
pub struct OpenClawAdapter {
    bus: Arc<dyn CommandBus>,
    access: AccessContext,
    config: OpenClawConfig,
}

impl OpenClawAdapter {
    /// 创建使用既有 `CommandBus` 与 ACL 上下文的 Adapter。
    #[must_use]
    pub fn new(bus: Arc<dyn CommandBus>, access: AccessContext, config: OpenClawConfig) -> Self {
        Self {
            bus,
            access,
            config,
        }
    }

    /// 解析并导入一份 Memory.md，不覆盖既有历史。
    ///
    /// # Errors
    ///
    /// 配置、文档、确定性标识或 Core Commit 失败时返回错误。
    pub async fn import_markdown(
        &self,
        document: OpenClawDocument,
    ) -> Result<OpenClawImportReceipt, OpenClawError> {
        self.config
            .validate()
            .map_err(OpenClawError::InvalidConfig)?;
        let mapped = map_document(&document, &self.config, &self.access.scope)?;
        let fragments = mapped
            .sections
            .iter()
            .map(|section| section.fragment.clone())
            .collect();
        let response = self
            .request(MemoryCommand::Commit(CommitCommand { fragments }))
            .await?;
        let MemoryResponse::Commit(result) = response.response else {
            return Err(OpenClawError::UnexpectedResponse);
        };
        let entries = mapped
            .sections
            .into_iter()
            .zip(result.receipts)
            .map(|(section, receipt)| OpenClawImportEntry {
                fragment_id: receipt.fragment_id,
                created: receipt.created,
                heading: section.heading,
                line_start: section.line_start,
                line_end: section.line_end,
            })
            .collect();
        Ok(OpenClawImportReceipt {
            document_sha256: mapped.document_sha256,
            entries,
        })
    }

    /// 通过 Core Recall 为 `OpenClaw` Hook 预取上下文。
    ///
    /// # Errors
    ///
    /// 配置无效、Core Recall 失败或响应类型不匹配时返回错误。
    pub async fn hook_recall(
        &self,
        query: OpenClawRecallQuery,
    ) -> Result<MemoryPacket, OpenClawError> {
        self.recall(query).await
    }

    /// 通过同一 Core Recall 为 `OpenClaw` MCP 节点预取上下文。
    ///
    /// # Errors
    ///
    /// 配置无效、Core Recall 失败或响应类型不匹配时返回错误。
    pub async fn mcp_recall(
        &self,
        query: OpenClawRecallQuery,
    ) -> Result<MemoryPacket, OpenClawError> {
        self.recall(query).await
    }

    async fn recall(&self, query: OpenClawRecallQuery) -> Result<MemoryPacket, OpenClawError> {
        self.config
            .validate()
            .map_err(OpenClawError::InvalidConfig)?;
        let response = self
            .request(MemoryCommand::Recall(RecallCommand {
                mode: query.mode,
                text: query.text,
                entities: Vec::new(),
                at_time: None,
                limits: self.config.recall_limits,
            }))
            .await?;
        let MemoryResponse::Recall(packet) = response.response else {
            return Err(OpenClawError::UnexpectedResponse);
        };
        Ok(packet)
    }

    async fn request(&self, command: MemoryCommand) -> Result<ResponseEnvelope, OpenClawError> {
        let request_id = jc_memory_protocol::TraceId::new().to_string();
        let mut access = self.access.clone();
        access.request_id.clone_from(&request_id);
        let request = MemoryRequest {
            api_version: API_VERSION_V1.to_owned(),
            request_id,
            access_scope: AccessScope::from(access.scope.clone()),
            command,
        };
        Ok(self.bus.execute(&access, request).await?)
    }
}
