//! Mem0 Add/Search 到疆诚记忆 Core 的来源保真适配器。

pub mod config;
pub mod export;
pub mod import;
pub mod mapping;

use std::sync::Arc;

use chrono::{DateTime, Utc};
use jc_memory_core::{CommandBus, CommandError};
use jc_memory_protocol::{
    API_VERSION_V1, AccessContext, AccessScope, CommitCommand, MemoryCommand, MemoryPacket,
    MemoryRequest, MemoryResponse, RecallCommand, RecallMode, ResponseEnvelope,
};
use serde_json::Value;

pub use config::Mem0Config;
pub use export::{export_manifest, parse_manifest};
pub use import::{MANIFEST_VERSION, Mem0IdMapping, Mem0IdMappingManifest, Mem0ImportReceipt};
use mapping::map_record;

/// 一条待迁移的 Mem0 记录或历史版本。
#[derive(Clone, Debug, PartialEq)]
pub struct Mem0Record {
    /// Mem0 外部记录 ID。
    pub id: String,
    /// 原始 memory 文本。
    pub memory: String,
    /// 原记录创建时间。
    pub created_at: DateTime<Utc>,
    /// 原记录最后更新时间。
    pub updated_at: DateTime<Utc>,
    /// 可选旧用户 ID，仅作为 Provenance 保存。
    pub user_id: Option<String>,
    /// 可选旧 Agent ID，仅作为 Provenance 保存。
    pub agent_id: Option<String>,
    /// 旧系统自报置信度；不会写入权威 confidence。
    pub legacy_confidence: Option<f64>,
    /// 原系统 metadata，不替代原文。
    pub metadata: Value,
}

/// Mem0 Search 到 Core Recall 的映射请求。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Mem0SearchQuery {
    /// 原始 Search 文本。
    pub query: String,
    /// Core Recall 模式。
    pub mode: RecallMode,
}

/// Mem0 Adapter 错误。
#[derive(Debug, thiserror::Error)]
pub enum Mem0Error {
    /// 下游 `CommandBus` 失败。
    #[error("command bus failure: {0}")]
    Command(#[from] CommandError),
    /// 配置不满足适配器边界。
    #[error("invalid Mem0 adapter configuration: {0}")]
    InvalidConfig(String),
    /// Mem0 记录无法安全迁移。
    #[error("invalid Mem0 record: {0}")]
    InvalidRecord(String),
    /// 确定性 Fragment ID 无法构造。
    #[error(transparent)]
    Identifier(#[from] jc_memory_protocol::IdParseError),
    /// Manifest JSON 无法序列化或解析。
    #[error(transparent)]
    ManifestJson(#[from] serde_json::Error),
    /// Manifest 版本不受支持。
    #[error("unsupported Mem0 ID mapping manifest version: {0}")]
    UnsupportedManifestVersion(String),
    /// Core 返回了不匹配的响应。
    #[error("unexpected command response from memory core")]
    UnexpectedResponse,
}

/// 仅通过 Core `CommandBus` 提供 Add、Search 和历史迁移的 Adapter。
pub struct Mem0Adapter {
    bus: Arc<dyn CommandBus>,
    access: AccessContext,
    config: Mem0Config,
}

impl Mem0Adapter {
    /// 创建使用既有 `CommandBus` 与 ACL 上下文的 Adapter。
    #[must_use]
    pub fn new(bus: Arc<dyn CommandBus>, access: AccessContext, config: Mem0Config) -> Self {
        Self {
            bus,
            access,
            config,
        }
    }

    /// 将一次 Mem0 Add 映射为未验证的权威 Fragment。
    ///
    /// # Errors
    ///
    /// 配置、记录、确定性标识或 Core Commit 失败时返回错误。
    pub async fn add(&self, record: Mem0Record) -> Result<Mem0ImportReceipt, Mem0Error> {
        self.import_history(vec![record]).await
    }

    /// 迁移 Mem0 历史版本，不覆盖相同外部 ID 的旧记录。
    ///
    /// # Errors
    ///
    /// 输入为空，或任一映射和 Core Commit 失败时返回错误。
    pub async fn import_history(
        &self,
        records: Vec<Mem0Record>,
    ) -> Result<Mem0ImportReceipt, Mem0Error> {
        self.config.validate().map_err(Mem0Error::InvalidConfig)?;
        if records.is_empty() {
            return Err(Mem0Error::InvalidRecord("历史记录不能为空".to_owned()));
        }
        let mapped = records
            .iter()
            .map(|record| map_record(record, &self.config, &self.access.scope))
            .collect::<Result<Vec<_>, _>>()?;
        let fragments = mapped.iter().map(|item| item.fragment.clone()).collect();
        let response = self
            .request(MemoryCommand::Commit(CommitCommand { fragments }))
            .await?;
        let MemoryResponse::Commit(result) = response.response else {
            return Err(Mem0Error::UnexpectedResponse);
        };
        let entries = records
            .into_iter()
            .zip(mapped)
            .zip(result.receipts)
            .map(|((record, mapped), receipt)| Mem0IdMapping {
                mem0_id: record.id,
                fragment_id: receipt.fragment_id,
                content_sha256: mapped.content_sha256,
                created_at: record.created_at,
                updated_at: record.updated_at,
                created: receipt.created,
            })
            .collect();
        Ok(Mem0ImportReceipt {
            manifest: Mem0IdMappingManifest {
                manifest_version: MANIFEST_VERSION.to_owned(),
                entries,
            },
        })
    }

    /// 将 Mem0 Search 映射为有界 Core Recall。
    ///
    /// # Errors
    ///
    /// 配置无效、Core Recall 失败或响应类型不匹配时返回错误。
    pub async fn search(&self, query: Mem0SearchQuery) -> Result<MemoryPacket, Mem0Error> {
        self.config.validate().map_err(Mem0Error::InvalidConfig)?;
        let response = self
            .request(MemoryCommand::Recall(RecallCommand {
                mode: query.mode,
                text: query.query,
                entities: Vec::new(),
                at_time: None,
                limits: self.config.recall_limits,
            }))
            .await?;
        let MemoryResponse::Recall(packet) = response.response else {
            return Err(Mem0Error::UnexpectedResponse);
        };
        Ok(packet)
    }

    async fn request(&self, command: MemoryCommand) -> Result<ResponseEnvelope, Mem0Error> {
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
