//! 默认拒绝的 ACL 决策引擎。

use jc_memory_protocol::{AccessContext, Decision, DenyReason, MemoryScope, Operation};

use crate::access::{grant_matches, subject_scope_matches};

/// 对核心操作执行确定性、无通配符的 Scope 授权。
#[derive(Clone, Copy, Debug, Default)]
pub struct AclEngine;

impl AclEngine {
    /// 按调用者上下文、操作和资源 Scope 返回默认拒绝决策。
    #[must_use]
    pub fn authorize(
        self,
        context: &AccessContext,
        operation: Operation,
        resource: &MemoryScope,
    ) -> Decision {
        if context.scope.namespace_id != resource.namespace_id {
            return Decision::Deny(DenyReason::NamespaceMismatch);
        }

        let crosses_tenant = context.scope.tenant_id != resource.tenant_id;
        if crosses_tenant {
            return if context
                .grants
                .iter()
                .any(|grant| grant_matches(grant, Operation::Admin, resource))
            {
                Decision::Allow
            } else {
                Decision::Deny(DenyReason::CrossTenantRequiresAdmin)
            };
        }

        if !subject_scope_matches(context, resource) {
            return Decision::Deny(DenyReason::SubjectScopeMismatch);
        }

        if context.grants.iter().any(|grant| {
            grant_matches(grant, operation, resource)
                || grant_matches(grant, Operation::Admin, resource)
        }) {
            Decision::Allow
        } else {
            Decision::Deny(DenyReason::MissingGrant)
        }
    }
}
