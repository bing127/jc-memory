//! `AccessContext` 与 ACL 精确 Scope 匹配测试。

use jc_memory_protocol::{MemoryScope, NamespaceId, ProjectId, TenantId, UserId};
use jc_memory_security::{AccessContext, AclEngine, Decision, DenyReason, Grant, Operation};

#[test]
fn missing_grant_is_denied() {
    let fixture = Fixture::new();
    let context = AccessContext::new("alice", fixture.resource_a.clone());

    assert_eq!(
        AclEngine.authorize(&context, Operation::Recall, &fixture.resource_a),
        Decision::Deny(DenyReason::MissingGrant)
    );
}

#[test]
fn exact_grant_allows_only_the_exact_scope_layers() {
    let fixture = Fixture::new();
    let mut context = AccessContext::new("alice", fixture.resource_a.clone());
    context
        .grants
        .insert(Grant::for_scope(Operation::Recall, &fixture.resource_a));

    assert_eq!(
        AclEngine.authorize(&context, Operation::Recall, &fixture.resource_a),
        Decision::Allow
    );
    assert_eq!(
        AclEngine.authorize(&context, Operation::Explain, &fixture.resource_a),
        Decision::Deny(DenyReason::MissingGrant)
    );

    let mut namespace_only = MemoryScope::new(fixture.namespace);
    namespace_only.user_id = fixture.resource_a.user_id;
    let mut namespace_context = AccessContext::new("alice", namespace_only.clone());
    namespace_context
        .grants
        .insert(Grant::for_scope(Operation::Recall, &namespace_only));
    namespace_context
        .grants
        .insert(Grant::for_scope(Operation::Admin, &namespace_only));
    assert_eq!(
        AclEngine.authorize(&namespace_context, Operation::Recall, &fixture.resource_a),
        Decision::Deny(DenyReason::CrossTenantRequiresAdmin)
    );
}

#[test]
fn crossing_projects_requires_a_grant_for_the_target_project() {
    let fixture = Fixture::new();
    let mut other_project = fixture.resource_a.clone();
    other_project.project_id = Some(ProjectId::new());
    let mut context = AccessContext::new("alice", fixture.resource_a.clone());
    context
        .grants
        .insert(Grant::for_scope(Operation::Recall, &fixture.resource_a));

    assert_eq!(
        AclEngine.authorize(&context, Operation::Recall, &other_project),
        Decision::Deny(DenyReason::MissingGrant)
    );

    context
        .grants
        .insert(Grant::for_scope(Operation::Recall, &other_project));
    assert_eq!(
        AclEngine.authorize(&context, Operation::Recall, &other_project),
        Decision::Allow
    );
}

#[test]
fn crossing_tenants_requires_an_exact_admin_grant() {
    let fixture = Fixture::new();
    let mut context = AccessContext::new("alice", fixture.resource_a.clone());
    context
        .grants
        .insert(Grant::for_scope(Operation::Recall, &fixture.resource_b));

    assert_eq!(
        AclEngine.authorize(&context, Operation::Recall, &fixture.resource_b),
        Decision::Deny(DenyReason::CrossTenantRequiresAdmin)
    );

    context
        .grants
        .insert(Grant::for_scope(Operation::Admin, &fixture.resource_b));
    assert_eq!(
        AclEngine.authorize(&context, Operation::Recall, &fixture.resource_b),
        Decision::Allow
    );
}

#[test]
fn grants_never_cross_namespaces_or_subject_boundaries() {
    let fixture = Fixture::new();
    let mut context = AccessContext::new("alice", fixture.resource_a.clone());
    context.grants.insert(Grant::for_scope(
        Operation::Admin,
        &fixture.foreign_namespace,
    ));
    assert_eq!(
        AclEngine.authorize(&context, Operation::Recall, &fixture.foreign_namespace),
        Decision::Deny(DenyReason::NamespaceMismatch)
    );

    let mut other_user = fixture.resource_a.clone();
    other_user.user_id = Some(UserId::new());
    context
        .grants
        .insert(Grant::for_scope(Operation::Recall, &other_user));
    assert_eq!(
        AclEngine.authorize(&context, Operation::Recall, &other_user),
        Decision::Deny(DenyReason::SubjectScopeMismatch)
    );
}

#[test]
fn protocol_names_are_stable_snake_case() {
    assert_eq!(
        serde_json::to_string(&Operation::Delete).unwrap(),
        "\"delete\""
    );
    assert_eq!(
        serde_json::to_string(&DenyReason::MissingGrant).unwrap(),
        "\"missing_grant\""
    );
}

struct Fixture {
    namespace: NamespaceId,
    resource_a: MemoryScope,
    resource_b: MemoryScope,
    foreign_namespace: MemoryScope,
}

impl Fixture {
    fn new() -> Self {
        let namespace = NamespaceId::new();
        let user = UserId::new();
        let resource_a = resource(namespace, TenantId::new(), ProjectId::new(), user);
        let resource_b = resource(namespace, TenantId::new(), ProjectId::new(), user);
        let foreign_namespace =
            resource(NamespaceId::new(), TenantId::new(), ProjectId::new(), user);
        Self {
            namespace,
            resource_a,
            resource_b,
            foreign_namespace,
        }
    }
}

fn resource(
    namespace_id: NamespaceId,
    tenant_id: TenantId,
    project_id: ProjectId,
    user_id: UserId,
) -> MemoryScope {
    let mut scope = MemoryScope::new(namespace_id);
    scope.tenant_id = Some(tenant_id);
    scope.project_id = Some(project_id);
    scope.user_id = Some(user_id);
    scope
}
