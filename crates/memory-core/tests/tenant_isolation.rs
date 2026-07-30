//! Core Recall 检索前与返回前的 Tenant 隔离验收测试。

#[allow(dead_code)]
mod common;

use chrono::Utc;
use jc_memory_core::{
    MemoryConfig, MemoryEngine, MemoryError, MutationCommand, RecallLimits, RecallQuery,
};
use jc_memory_protocol::{
    AccessContext, DenyReason, FragmentContent, FragmentKind, FragmentSource, Grant, MemoryScope,
    NamespaceId, Operation, Perspective, RecallMode, SourceType, TenantId, UserId,
    VerificationMethod,
};
use jc_memory_storage::{SqliteConfig, SqliteStore};

#[tokio::test]
async fn recall_never_loads_or_returns_another_tenant() {
    let data_dir = tempfile::tempdir().expect("创建引擎数据目录");
    let engine = MemoryEngine::open(MemoryConfig::local(data_dir.path()))
        .await
        .expect("打开记忆引擎");
    let namespace = NamespaceId::new();
    let user = UserId::new();
    let scope_a = tenant_scope(namespace, TenantId::new(), user);
    let scope_b = tenant_scope(namespace, TenantId::new(), user);
    let context_a = context("alice", &scope_a, &[Operation::Observe, Operation::Recall]);
    let context_b = context("bob", &scope_b, &[Operation::Observe, Operation::Mutate]);
    let mut fragment_a = common::observation("same secret project");
    fragment_a.scope = scope_a.clone();
    fragment_a.kind = FragmentKind::Fact;
    let local_fragment_id = fragment_a.id;
    let mut fragment_b = common::observation_in_scope(scope_b.clone(), "same secret project");
    fragment_b.kind = FragmentKind::Fact;
    let foreign_fragment_id = fragment_b.id;
    engine
        .observe(&context_a, fragment_a)
        .await
        .expect("写入 Tenant A");
    engine
        .observe(&context_b, fragment_b)
        .await
        .expect("写入 Tenant B");
    engine
        .reinforce(
            &context_b,
            mutation(foreign_fragment_id, "same secret project"),
        )
        .await
        .expect("为 Tenant B 建立派生关系");

    let store = SqliteStore::open(&SqliteConfig::new(data_dir.path().join("memory.sqlite3")))
        .await
        .expect("打开隔离验证数据库");
    sqlx::query("UPDATE fragments SET metadata_json = '[]' WHERE id = ?1")
        .bind(foreign_fragment_id.to_string())
        .execute(store.pool())
        .await
        .expect("破坏 Tenant B 行");

    let packet = engine
        .recall(&context_a, query(scope_a, "same secret project"))
        .await
        .expect("召回 Tenant A");
    assert_eq!(packet.points.len(), 1);
    assert_eq!(packet.points[0].source_fragments, vec![local_fragment_id]);
    assert!(
        packet
            .evidence
            .iter()
            .all(|item| item.fragment_id != foreign_fragment_id)
    );
}

#[tokio::test]
async fn cross_tenant_recall_is_denied_before_query_without_admin() {
    let data_dir = tempfile::tempdir().expect("创建引擎数据目录");
    let engine = MemoryEngine::open(MemoryConfig::local(data_dir.path()))
        .await
        .expect("打开记忆引擎");
    let namespace = NamespaceId::new();
    let user = UserId::new();
    let scope_a = tenant_scope(namespace, TenantId::new(), user);
    let scope_b = tenant_scope(namespace, TenantId::new(), user);
    let mut context_a = context("alice", &scope_a, &[]);
    context_a
        .grants
        .insert(Grant::for_scope(Operation::Recall, &scope_b));

    assert!(matches!(
        engine
            .recall(&context_a, query(scope_b, "same secret project"))
            .await,
        Err(MemoryError::AccessDenied {
            operation: Operation::Recall,
            reason: DenyReason::CrossTenantRequiresAdmin,
        })
    ));
}

fn context(principal: &str, scope: &MemoryScope, operations: &[Operation]) -> AccessContext {
    let mut context = AccessContext::new(principal, scope.clone());
    context.grants.extend(
        operations
            .iter()
            .map(|operation| Grant::for_scope(*operation, scope)),
    );
    context
}

fn query(scope: MemoryScope, text: &str) -> RecallQuery {
    RecallQuery {
        scope,
        mode: RecallMode::Semantic,
        text: text.to_owned(),
        entities: Vec::new(),
        at_time: None,
        limits: RecallLimits {
            max_seeds: 4,
            max_neighbors: 4,
            max_hops: 2,
            max_nodes: 8,
            timeout_ms: 100,
        },
    }
}

fn mutation(target_fragment_id: jc_memory_protocol::FragmentId, text: &str) -> MutationCommand {
    MutationCommand {
        target_fragment_id,
        content: FragmentContent::Text {
            text: text.to_owned(),
        },
        source: FragmentSource {
            source_type: SourceType::User,
            source_id: "tenant-isolation-test".to_owned(),
            actor_id: Some("tenant-b-user".to_owned()),
            observer_id: None,
            tool_id: None,
            uri: None,
        },
        perspective: Perspective::FirstPerson,
        occurred_at: Utc::now(),
        verification_method: VerificationMethod::Manual,
        reason: "构造跨 Tenant 派生候选".to_owned(),
    }
}

fn tenant_scope(namespace: NamespaceId, tenant: TenantId, user: UserId) -> MemoryScope {
    let mut scope = MemoryScope::new(namespace);
    scope.tenant_id = Some(tenant);
    scope.user_id = Some(user);
    scope
}
