//! Embedded 与远程服务共享的稳定命令 Envelope。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{
    AgentId, BlobId, CognitiveFragment, EventId, FragmentId, MemoryPacket, MemoryScope,
    NamespaceId, ProjectId, RecallLimits, RecallMode, TenantId, UserId,
};

/// 当前稳定服务协议版本。
pub const API_VERSION_V1: &str = "v1";

/// 客户端可声明的访问 Scope，不包含任何服务端授权 Grant。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AccessScope {
    /// 顶层 Namespace。
    pub namespace_id: NamespaceId,
    /// 可选 Tenant 边界。
    pub tenant_id: Option<TenantId>,
    /// 可选 User 边界。
    pub user_id: Option<UserId>,
    /// 可选 Agent 边界。
    pub agent_id: Option<AgentId>,
    /// 可选 Project 边界。
    pub project_id: Option<ProjectId>,
}

impl From<MemoryScope> for AccessScope {
    fn from(scope: MemoryScope) -> Self {
        Self {
            namespace_id: scope.namespace_id,
            tenant_id: scope.tenant_id,
            user_id: scope.user_id,
            agent_id: scope.agent_id,
            project_id: scope.project_id,
        }
    }
}

impl From<AccessScope> for MemoryScope {
    fn from(scope: AccessScope) -> Self {
        Self {
            namespace_id: scope.namespace_id,
            tenant_id: scope.tenant_id,
            user_id: scope.user_id,
            agent_id: scope.agent_id,
            project_id: scope.project_id,
        }
    }
}

/// 一个真实可执行的 Observe 命令。
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ObserveCommand {
    /// 必须原样保留并进入权威存储的 Fragment。
    pub fragment: CognitiveFragment,
}

/// 一个有界、模型无关的 Recall 命令。
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct RecallCommand {
    /// Recall 选择模式。
    pub mode: RecallMode,
    /// 调用方提供的检索文本。
    pub text: String,
    /// 已规范化的实体名称。
    pub entities: Vec<String>,
    /// 可选的历史有效时间点。
    pub at_time: Option<DateTime<Utc>>,
    /// 本次请求的硬执行预算。
    pub limits: RecallLimits,
}

/// 当前 `CommandBus` 可执行的命令集合。
// Observe 和 Recall 都需要在协议边界保持完整值语义，装箱会改变现有调用方类型。
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "command", content = "payload", rename_all = "snake_case")]
pub enum MemoryCommand {
    /// 写入新的权威 Fragment/Event。
    Observe(ObserveCommand),
    /// 在授权 Scope 内执行有界 Recall。
    Recall(RecallCommand),
}

/// 所有 Embedded 与远程协议请求共享的 Envelope。
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct MemoryRequest {
    /// 请求采用的服务协议版本。
    pub api_version: String,
    /// 调用方生成的非空请求标识。
    pub request_id: String,
    /// 调用方声明的访问边界，不包含 Grant。
    pub access_scope: AccessScope,
    /// 需要执行的业务命令。
    pub command: MemoryCommand,
}

impl MemoryRequest {
    /// 校验所有命令共享的稳定 Envelope 字段。
    ///
    /// # Errors
    ///
    /// 协议版本不受支持或请求标识为空时返回错误。
    pub fn validate(&self) -> Result<(), RequestValidationError> {
        if self.api_version != API_VERSION_V1 {
            return Err(RequestValidationError::UnsupportedApiVersion {
                api_version: self.api_version.clone(),
            });
        }
        if self.request_id.trim().is_empty() {
            return Err(RequestValidationError::EmptyRequestId);
        }
        Ok(())
    }
}

/// 请求 Envelope 的稳定校验错误。
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum RequestValidationError {
    /// `api_version` 不是当前稳定版本。
    #[error("unsupported api_version: {api_version}")]
    UnsupportedApiVersion {
        /// 调用方提供的版本。
        api_version: String,
    },
    /// `request_id` 为空或仅包含空白。
    #[error("request_id cannot be empty")]
    EmptyRequestId,
}

/// Observe 的稳定服务响应，不暴露 Core 内部检测器实现。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ObserveResult {
    /// 已持久化 Fragment 标识。
    pub fragment_id: FragmentId,
    /// 已持久化不可变 Event 标识。
    pub event_id: EventId,
    /// 精确 Scope 内的单调序号。
    pub sequence: u64,
    /// 完整 Fragment 规范化 JSON 的 SHA-256。
    pub content_hash: BlobId,
    /// 本次调用是否创建了权威 Event。
    pub created: bool,
    /// 准入后 Fragment 是否处于 Quarantine。
    pub quarantined: bool,
}

/// 当前 `CommandBus` 的稳定响应集合。
// Observe 和 Recall 都需要在协议边界保持完整值语义，装箱会改变现有调用方类型。
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "response", content = "payload", rename_all = "snake_case")]
pub enum MemoryResponse {
    /// Observe 的持久化结果。
    Observe(ObserveResult),
    /// Recall 返回的模型无关 Memory Packet。
    Recall(MemoryPacket),
}

/// 与请求元数据关联的成功响应 Envelope。
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ResponseEnvelope {
    /// 与请求一致的服务协议版本。
    pub api_version: String,
    /// 与请求一致的请求标识。
    pub request_id: String,
    /// 业务响应。
    pub response: MemoryResponse,
}

impl ResponseEnvelope {
    /// 从已校验请求构造关联响应。
    #[must_use]
    pub fn for_request(request: &MemoryRequest, response: MemoryResponse) -> Self {
        Self {
            api_version: request.api_version.clone(),
            request_id: request.request_id.clone(),
            response,
        }
    }

    /// 以递归字段排序后的结构化 JSON 返回稳定表示。
    ///
    /// # Errors
    ///
    /// 响应无法转换为 JSON 值时返回序列化错误。
    pub fn canonical_json(&self) -> Result<String, serde_json::Error> {
        let value = canonicalize(serde_json::to_value(self)?);
        serde_json::to_string(&value)
    }
}

/// 服务边界使用的稳定错误分类。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandErrorCode {
    /// 请求字段或业务输入无效。
    InvalidArgument,
    /// 调用方没有执行该操作的权限。
    PermissionDenied,
    /// 目标权威记录不存在。
    NotFound,
    /// 标识或持久化约束发生冲突。
    Conflict,
    /// 有界资源暂时耗尽。
    ResourceExhausted,
    /// 未归入更具体公开分类的内部失败。
    Internal,
}

fn canonicalize(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Array(values) => {
            serde_json::Value::Array(values.into_iter().map(canonicalize).collect())
        }
        serde_json::Value::Object(values) => {
            let mut entries: Vec<_> = values.into_iter().collect();
            entries.sort_by(|left, right| left.0.cmp(&right.0));
            serde_json::Value::Object(
                entries
                    .into_iter()
                    .map(|(key, value)| (key, canonicalize(value)))
                    .collect(),
            )
        }
        scalar => scalar,
    }
}
