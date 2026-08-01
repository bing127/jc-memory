//! Embedded 与远程服务共享的稳定命令 Envelope。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{
    AgentId, BlobId, CognitiveFragment, EdgeId, EdgeKind, EventId, FragmentContent, FragmentId,
    FragmentSource, MemoryPacket, MemoryScope, NamespaceId, Perspective, PointId, ProjectId,
    RecallLimits, RecallMode, StateDeclaration, SurfaceId, TenantId, UserId, VerificationMethod,
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

/// 提交一次完整 Runtime 状态转移中产生的权威 Fragment 集合。
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct CommitCommand {
    /// 当前观察、假设、动作、工具结果和 Outcome 等原始 Fragment。
    pub fragments: Vec<CognitiveFragment>,
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

/// 读取当前授权 Scope 统计的命令。
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct StatsCommand;

/// Scope 内权威记录统计结果。
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct StatsResult {
    /// 统计对应的精确访问 Scope。
    pub scope: AccessScope,
    /// 权威 Fragment 数量。
    pub fragment_count: u64,
    /// 权威 Event 数量。
    pub event_count: u64,
}

/// 纠正或失效一条权威 Fragment 所需的变更输入。
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct MutationCommand {
    /// 被引用但不被覆盖的目标 Fragment。
    pub target_fragment_id: FragmentId,
    /// 新变更 Fragment 的内容。
    pub content: FragmentContent,
    /// 发起变更的来源信息。
    pub source: FragmentSource,
    /// 变更内容的来源视角。
    pub perspective: Perspective,
    /// 变更在来源系统中的发生时间。
    pub occurred_at: DateTime<Utc>,
    /// 验证目标引用使用的方法。
    pub verification_method: VerificationMethod,
    /// 可审计的变更原因。
    pub reason: String,
    /// 纠正操作同时声明的新状态；旧客户端可以省略。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<StateDeclaration>,
}

/// 变更命令的公开动作类型。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MutationAction {
    /// 追加纠正记录。
    Correct,
    /// 追加失效记录。
    Forget,
}

/// 一次已持久化变更的稳定结果。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MutationResult {
    /// 实际执行的变更动作。
    pub action: MutationAction,
    /// 被引用的原始 Fragment。
    pub target_fragment_id: FragmentId,
    /// 新追加的变更 Fragment。
    pub fragment_id: FragmentId,
    /// 承载新 Fragment 的 Event。
    pub event_id: EventId,
    /// 精确 Scope 内的 Event 序号。
    pub sequence: u64,
    /// 派生图重建时应建立的关系。
    pub relation: Option<EdgeKind>,
}

/// 一次 Commit 中各 Fragment 的持久化结果。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CommitResult {
    /// 按输入顺序返回的权威 Observe 结果。
    pub receipts: Vec<ObserveResult>,
}

/// Explain 结果中的初始种子。
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct ExplainSeed {
    /// 被选中的 Point。
    pub point_id: PointId,
    /// 初始激活分数。
    pub score: f32,
}

/// Explain 结果中的图遍历记录。
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ExplainTraversal {
    /// 被遍历的 Edge。
    pub edge_id: EdgeId,
    /// 遍历起点 Point。
    pub source_point_id: PointId,
    /// 遍历终点 Point。
    pub target_point_id: PointId,
    /// 关系类型。
    pub kind: EdgeKind,
    /// 终点激活分数。
    pub score: f32,
    /// 终点相对种子深度。
    pub depth: usize,
}

/// Explain 结果中的激活 Surface。
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct ExplainSurface {
    /// Surface 标识。
    pub surface_id: SurfaceId,
    /// Surface 版本。
    pub version: u64,
    /// 激活分数。
    pub activation: f32,
}

/// 一次 Recall 的结构化证据追踪结果。
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct ExplainResult {
    /// 初始 Point 激活记录。
    pub seeds: Vec<ExplainSeed>,
    /// 有向图遍历记录。
    pub traversals: Vec<ExplainTraversal>,
    /// 被激活的 Surface 版本。
    pub surfaces: Vec<ExplainSurface>,
    /// 未经过人工或工具确认的关系。
    pub inferred_edges: Vec<EdgeId>,
    /// 已验证 Point 背后的权威 Fragment。
    pub verified_fragments: Vec<FragmentId>,
    /// 本次 Explain 使用的硬预算。
    pub limits: RecallLimits,
    /// 是否因硬预算截断候选。
    pub truncated: bool,
}

/// 当前 `CommandBus` 可执行的命令集合。
// Observe 和 Recall 都需要在协议边界保持完整值语义，装箱会改变现有调用方类型。
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "command", content = "payload", rename_all = "snake_case")]
pub enum MemoryCommand {
    /// 写入新的权威 Fragment/Event。
    Observe(ObserveCommand),
    /// 追加 Runtime 状态转移产生的完整 Fragment 集合。
    Commit(CommitCommand),
    /// 在授权 Scope 内执行有界 Recall。
    Recall(RecallCommand),
    /// 读取当前授权 Scope 的权威记录统计。
    Stats(StatsCommand),
    /// 追加纠正记录。
    Correct(MutationCommand),
    /// 追加失效记录而不删除原始记录。
    Forget(MutationCommand),
    /// 返回 Recall 的结构化证据追踪。
    Explain(RecallCommand),
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
        if let MemoryCommand::Commit(command) = &self.command
            && command.fragments.is_empty()
        {
            return Err(RequestValidationError::EmptyCommit);
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
    /// Commit 至少需要一条待追加的权威 Fragment。
    #[error("commit must contain at least one fragment")]
    EmptyCommit,
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
    /// Commit 中各权威 Fragment 的追加结果。
    Commit(CommitResult),
    /// Recall 返回的模型无关 Memory Packet。
    Recall(MemoryPacket),
    /// Scope 内权威记录统计结果。
    Stats(StatsResult),
    /// 纠正或失效操作的持久化结果。
    Mutation(MutationResult),
    /// Recall 的结构化证据追踪结果。
    Explain(ExplainResult),
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
