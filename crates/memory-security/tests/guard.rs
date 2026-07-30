//! Memory Packet 返回前的独立 Scope 不变量测试。

use std::collections::BTreeMap;

use jc_memory_protocol::{
    AccessContext, FragmentId, Grant, MemoryPacket, MemoryScope, NamespaceId, Operation, PointId,
    PointKind, PointView, RecallId, RecallMode, TenantId, ValidityStatus,
};
use jc_memory_security::{OutputGuard, SecurityInvariantViolation};

#[test]
fn packet_with_a_foreign_fragment_is_rejected_whole() {
    let namespace = NamespaceId::new();
    let scope_a = tenant_scope(namespace, TenantId::new());
    let scope_b = tenant_scope(namespace, TenantId::new());
    let context = recall_context(&scope_a);
    let foreign_fragment = FragmentId::new();
    let mut packet = MemoryPacket::empty(RecallId::new(), RecallMode::Semantic);
    packet.points.push(PointView {
        point_id: PointId::new(),
        kind: PointKind::Fact,
        confidence: 1.0,
        validity: ValidityStatus::Verified,
        source_fragments: vec![foreign_fragment],
    });
    let scopes = BTreeMap::from([(foreign_fragment, scope_b)]);

    assert_eq!(
        OutputGuard.filter_and_assert(&context, Operation::Recall, &scope_a, &packet, &scopes,),
        Err(SecurityInvariantViolation::FragmentScopeMismatch(
            foreign_fragment
        ))
    );
}

#[test]
fn packet_with_an_unknown_fragment_is_rejected_whole() {
    let scope = tenant_scope(NamespaceId::new(), TenantId::new());
    let context = recall_context(&scope);
    let fragment_id = FragmentId::new();
    let mut packet = MemoryPacket::empty(RecallId::new(), RecallMode::Semantic);
    packet.points.push(PointView {
        point_id: PointId::new(),
        kind: PointKind::Fact,
        confidence: 1.0,
        validity: ValidityStatus::Verified,
        source_fragments: vec![fragment_id],
    });

    assert_eq!(
        OutputGuard.filter_and_assert(
            &context,
            Operation::Recall,
            &scope,
            &packet,
            &BTreeMap::new(),
        ),
        Err(SecurityInvariantViolation::UnknownFragment(fragment_id))
    );
}

fn recall_context(scope: &MemoryScope) -> AccessContext {
    let mut context = AccessContext::new("guard-test", scope.clone());
    context
        .grants
        .insert(Grant::for_scope(Operation::Recall, scope));
    context
}

fn tenant_scope(namespace: NamespaceId, tenant: TenantId) -> MemoryScope {
    let mut scope = MemoryScope::new(namespace);
    scope.tenant_id = Some(tenant);
    scope
}
