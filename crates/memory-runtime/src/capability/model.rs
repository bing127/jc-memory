//! Capability 的稳定描述和执行边界。

use std::collections::BTreeSet;

use async_trait::async_trait;
use jc_memory_protocol::SurfaceId;
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

/// Capability 的来源类别。
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityKind {
    /// 提示词模板。
    Prompt,
    /// 可复用技能。
    Skill,
    /// 本地或远端工具。
    Tool,
    /// MCP 工具。
    McpTool,
    /// 多步工作流。
    Workflow,
    /// 策略实现。
    Policy,
}

/// Capability 声明的最小权限集合。
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    /// 读取当前授权 Scope 内的记忆。
    ReadMemory,
    /// 修改当前授权 Scope 内的记忆。
    WriteMemory,
    /// 访问外部网络。
    Network,
    /// 访问本地文件系统。
    Filesystem,
    /// 创建或管理本地进程。
    Process,
    /// 产生不可逆或外部可见的副作用。
    ExternalSideEffect,
}

/// Capability 的风险等级。
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    /// 只读且影响受限。
    Low,
    /// 可能产生有限副作用。
    Medium,
    /// 需要显式批准的高风险操作。
    High,
    /// 需要显式批准的关键操作。
    Critical,
}

/// Capability 的不可变描述。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityDescriptor {
    /// 稳定 Capability 标识。
    pub id: String,
    /// `SemVer` 版本。
    pub version: Version,
    /// Capability 类别。
    pub kind: CapabilityKind,
    /// 来源或发行方标识。
    pub source: String,
    /// 执行所需的最小权限。
    pub permissions: BTreeSet<Permission>,
    /// 输入 JSON Schema。
    pub input_schema: Value,
    /// 输出 JSON Schema。
    pub output_schema: Value,
    /// 风险等级。
    pub risk: RiskLevel,
    /// 是否需要进入审批状态。
    pub requires_approval: bool,
    /// 可暴露的 Surface 集合。
    pub surfaces: Vec<SurfaceId>,
}

impl CapabilityDescriptor {
    pub(crate) fn validate(&self) -> Result<(), CapabilityError> {
        if self.id.trim().is_empty() || self.source.trim().is_empty() {
            return Err(CapabilityError::InvalidDescriptor(
                "capability id and source must be non-empty".into(),
            ));
        }
        if !self.input_schema.is_object() || !self.output_schema.is_object() {
            return Err(CapabilityError::InvalidDescriptor(
                "capability schemas must be JSON objects".into(),
            ));
        }
        if self.risk >= RiskLevel::High && !self.requires_approval {
            return Err(CapabilityError::InvalidDescriptor(
                "high-risk capabilities require approval".into(),
            ));
        }
        let unique_surfaces = self.surfaces.iter().collect::<BTreeSet<_>>().len();
        if unique_surfaces != self.surfaces.len() {
            return Err(CapabilityError::InvalidDescriptor(
                "capability surfaces must be unique".into(),
            ));
        }
        Ok(())
    }
}

/// Capability 解析请求。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityRequest {
    /// 稳定 Capability 标识。
    pub id: String,
    /// 允许匹配的 `SemVer` 范围。
    pub version: VersionReq,
}

/// 已通过解析的 Capability 执行器。
#[async_trait]
pub trait CapabilityExecutor: Send + Sync {
    /// 执行一次 Capability，并在取消时尽快停止。
    async fn execute(
        &self,
        arguments: Value,
        cancellation: CancellationToken,
    ) -> Result<Value, CapabilityError>;
}

/// Capability 注册和执行失败。
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum CapabilityError {
    /// 描述符不符合稳定契约。
    #[error("invalid capability descriptor: {0}")]
    InvalidDescriptor(String),
    /// 注册表中已有相同 ID 和版本。
    #[error("capability version already registered: {0}@{1}")]
    Duplicate(String, Version),
    /// 请求中的 ID 无效。
    #[error("invalid capability request: {0}")]
    InvalidRequest(String),
    /// 找不到请求的 Capability。
    #[error("capability not found: {0}")]
    NotFound(String),
    /// 找不到满足 `SemVer` 范围的版本。
    #[error("no matching capability version: {0}")]
    VersionMismatch(String),
    /// 调用者缺少声明权限对应的 Scope Grant。
    #[error("capability permission denied: {0:?}")]
    PermissionDenied(Permission),
    /// Capability 执行失败。
    #[error("capability execution failed: {0}")]
    Execution(String),
}
