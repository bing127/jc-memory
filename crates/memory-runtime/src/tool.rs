//! Tool 描述与执行注册表。

use std::{collections::BTreeMap, sync::Arc};

use async_trait::async_trait;
use jc_memory_protocol::{AccessContext, Grant, Operation};
use semver::{Version, VersionReq};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::{Permission, RiskLevel};

/// Tool 是否允许自动重放。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToolIdempotency {
    /// 相同参数可以安全重放。
    Idempotent,
    /// 可能产生重复副作用，不得自动重放。
    NonIdempotent,
}

/// Tool 的执行描述。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolDescriptor {
    /// 稳定 Tool 标识。
    pub id: String,
    /// Tool 版本。
    pub version: Version,
    /// 声明权限。
    pub permissions: std::collections::BTreeSet<Permission>,
    /// 风险等级。
    pub risk: RiskLevel,
    /// 是否必须审批。
    pub requires_approval: bool,
    /// 幂等性声明。
    pub idempotency: ToolIdempotency,
    /// 输入 JSON Schema。
    pub input_schema: Value,
    /// 输出 JSON Schema。
    pub output_schema: Value,
}

impl ToolDescriptor {
    fn validate(&self) -> Result<(), ToolError> {
        if self.id.trim().is_empty()
            || !self.input_schema.is_object()
            || !self.output_schema.is_object()
        {
            return Err(ToolError::InvalidDescriptor);
        }
        if self.risk >= RiskLevel::High && !self.requires_approval {
            return Err(ToolError::InvalidDescriptor);
        }
        Ok(())
    }
}

/// Tool 执行器。
#[async_trait]
pub trait ToolExecutor: Send + Sync {
    /// 执行一次可取消的 Tool 调用。
    async fn execute(
        &self,
        arguments: Value,
        cancellation: CancellationToken,
    ) -> Result<Value, ToolError>;
}

struct Entry {
    descriptor: ToolDescriptor,
    executor: Arc<dyn ToolExecutor>,
}

/// 已通过版本与权限检查的 Tool。
pub struct ResolvedTool {
    /// Tool 描述。
    pub descriptor: ToolDescriptor,
    /// Tool 执行器。
    pub executor: Arc<dyn ToolExecutor>,
}

/// Tool 注册表。
pub struct ToolRegistry {
    entries: std::sync::RwLock<BTreeMap<String, BTreeMap<Version, Arc<Entry>>>>,
}

impl ToolRegistry {
    /// 创建空注册表。
    #[must_use]
    pub fn new() -> Self {
        Self {
            entries: std::sync::RwLock::new(BTreeMap::new()),
        }
    }

    /// 注册 Tool。
    ///
    /// # Errors
    ///
    /// 描述符无效或 ID 与版本重复时返回错误。
    pub fn register(
        &self,
        descriptor: ToolDescriptor,
        executor: Arc<dyn ToolExecutor>,
    ) -> Result<(), ToolError> {
        descriptor.validate()?;
        let id = descriptor.id.clone();
        let version = descriptor.version.clone();
        let mut entries = self
            .entries
            .write()
            .map_err(|_| ToolError::RegistryPoisoned)?;
        let versions = entries.entry(id.clone()).or_default();
        if versions.contains_key(&version) {
            return Err(ToolError::Duplicate(id, version));
        }
        versions.insert(
            version,
            Arc::new(Entry {
                descriptor,
                executor,
            }),
        );
        Ok(())
    }

    /// 选择最高兼容版本并校验精确 Scope Grant。
    ///
    /// # Errors
    ///
    /// Tool 不存在、版本不匹配、权限不足或注册表锁不可用时返回错误。
    pub fn resolve(
        &self,
        id: &str,
        version: &VersionReq,
        access: &AccessContext,
    ) -> Result<ResolvedTool, ToolError> {
        let entries = self
            .entries
            .read()
            .map_err(|_| ToolError::RegistryPoisoned)?;
        let versions = entries
            .get(id)
            .ok_or_else(|| ToolError::NotFound(id.into()))?;
        let entry = versions
            .iter()
            .rev()
            .find(|(candidate, _)| version.matches(candidate))
            .map(|(_, entry)| Arc::clone(entry))
            .ok_or_else(|| ToolError::VersionMismatch(id.into()))?;
        for permission in &entry.descriptor.permissions {
            let operation = match permission {
                Permission::ReadMemory => Operation::Recall,
                Permission::WriteMemory => Operation::Mutate,
                Permission::Network
                | Permission::Filesystem
                | Permission::Process
                | Permission::ExternalSideEffect => Operation::Admin,
            };
            if !access
                .grants
                .contains(&Grant::for_scope(operation, &access.scope))
            {
                return Err(ToolError::PermissionDenied(permission.clone()));
            }
        }
        Ok(ResolvedTool {
            descriptor: entry.descriptor.clone(),
            executor: Arc::clone(&entry.executor),
        })
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Tool 错误。
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ToolError {
    /// 描述符无效。
    #[error("invalid tool descriptor")]
    InvalidDescriptor,
    /// 版本重复。
    #[error("duplicate tool version: {0}@{1}")]
    Duplicate(String, Version),
    /// Tool 不存在。
    #[error("tool not found: {0}")]
    NotFound(String),
    /// 没有匹配版本。
    #[error("tool version mismatch: {0}")]
    VersionMismatch(String),
    /// 缺少权限。
    #[error("tool permission denied: {0:?}")]
    PermissionDenied(Permission),
    /// 需要审批。
    #[error("tool approval required: {0}")]
    ApprovalRequired(uuid::Uuid),
    /// 审批已拒绝。
    #[error("tool approval rejected: {0}")]
    ApprovalRejected(uuid::Uuid),
    /// Tool 执行失败。
    #[error("tool execution failed: {0}")]
    Execution(String),
    /// 调用被取消。
    #[error("tool execution cancelled")]
    Cancelled,
    /// 注册表锁不可用。
    #[error("tool registry lock poisoned")]
    RegistryPoisoned,
}
