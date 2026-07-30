//! 精确 Scope 授权匹配辅助逻辑。

use jc_memory_protocol::{AccessContext, Grant, MemoryScope, Operation};

pub(crate) fn grant_matches(grant: &Grant, operation: Operation, resource: &MemoryScope) -> bool {
    grant.operation == operation
        && grant.namespace_id == resource.namespace_id
        && grant.tenant_id == resource.tenant_id
        && grant.project_id == resource.project_id
}

pub(crate) fn subject_scope_matches(context: &AccessContext, resource: &MemoryScope) -> bool {
    context.scope.user_id == resource.user_id && context.scope.agent_id == resource.agent_id
}
