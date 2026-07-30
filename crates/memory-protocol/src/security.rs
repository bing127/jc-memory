//! 访问控制的稳定协议类型。

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::{MemoryScope, NamespaceId, ProjectId, TenantId, TraceId};

/// 受 ACL 保护的核心操作。
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    /// 写入新的权威 Fragment/Event。
    Observe,
    /// 召回记忆。
    Recall,
    /// 解释一次召回。
    Explain,
    /// 追加纠正或关系变更。
    Mutate,
    /// 执行记忆巩固。
    Consolidate,
    /// 导出授权数据。
    Export,
    /// 执行治理删除。
    Delete,
    /// 执行显式跨边界管理操作。
    Admin,
}

/// 一个操作在 Namespace、Tenant 和 Project 上的精确授权。
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Grant {
    /// 被授权的操作。
    pub operation: Operation,
    /// 被授权的精确 Namespace。
    pub namespace_id: NamespaceId,
    /// 被授权的精确 Tenant；`None` 只匹配无 Tenant 的资源。
    pub tenant_id: Option<TenantId>,
    /// 被授权的精确 Project；`None` 只匹配无 Project 的资源。
    pub project_id: Option<ProjectId>,
}

impl Grant {
    /// 从资源 Scope 创建一个不包含通配语义的 Grant。
    #[must_use]
    pub const fn for_scope(operation: Operation, scope: &MemoryScope) -> Self {
        Self {
            operation,
            namespace_id: scope.namespace_id,
            tenant_id: scope.tenant_id,
            project_id: scope.project_id,
        }
    }
}

/// 一次核心调用携带的调用者、Scope 和显式 Grant。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AccessContext {
    /// 发起调用的稳定主体标识。
    pub principal_id: String,
    /// 调用者当前所在的完整 Scope。
    pub scope: MemoryScope,
    /// 调用者持有的显式授权集合。
    pub grants: BTreeSet<Grant>,
    /// 用于审计关联的请求标识。
    pub request_id: String,
}

impl AccessContext {
    /// 创建一个默认不含任何 Grant 的访问上下文。
    #[must_use]
    pub fn new(principal_id: impl Into<String>, scope: MemoryScope) -> Self {
        Self {
            principal_id: principal_id.into(),
            scope,
            grants: BTreeSet::new(),
            request_id: TraceId::new().to_string(),
        }
    }
}

/// ACL 拒绝请求的稳定原因。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DenyReason {
    /// 没有匹配操作和精确资源 Scope 的 Grant。
    MissingGrant,
    /// 调用者与资源不属于同一 Namespace。
    NamespaceMismatch,
    /// 跨 Tenant 操作缺少目标 Scope 的显式 Admin Grant。
    CrossTenantRequiresAdmin,
    /// 非管理操作越过了 User 或 Agent 边界。
    SubjectScopeMismatch,
}

/// ACL 对一次操作的确定性决策。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    /// 允许继续执行。
    Allow,
    /// 按稳定原因拒绝执行。
    Deny(DenyReason),
}
