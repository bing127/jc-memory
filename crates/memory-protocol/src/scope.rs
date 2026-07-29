use crate::ids::{AgentId, NamespaceId, ProjectId, TenantId, UserId};
use serde::{Deserialize, Serialize};

/// The hierarchical ownership scope attached to memory data.
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct MemoryScope {
    /// Required top-level namespace.
    pub namespace_id: NamespaceId,
    /// Optional tenant boundary.
    pub tenant_id: Option<TenantId>,
    /// Optional user boundary.
    pub user_id: Option<UserId>,
    /// Optional agent boundary.
    pub agent_id: Option<AgentId>,
    /// Optional project boundary.
    pub project_id: Option<ProjectId>,
}

impl MemoryScope {
    /// Creates a local scope under the required namespace.
    #[must_use]
    pub const fn new(namespace_id: NamespaceId) -> Self {
        Self {
            namespace_id,
            tenant_id: None,
            user_id: None,
            agent_id: None,
            project_id: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_requires_a_namespace_but_allows_local_optional_scopes() {
        let scope = MemoryScope::new(NamespaceId::new());
        assert!(scope.tenant_id.is_none());
        assert!(scope.user_id.is_none());
        assert!(scope.agent_id.is_none());
        assert!(scope.project_id.is_none());
    }
}
