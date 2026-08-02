//! Core Consolidation Job 授权与生命周期契约。

use jc_memory_core::{
    JobKind, JobSpec, JobStatus, MemoryConfig, MemoryEngine, MemoryError, ResourceLimits,
};
use jc_memory_protocol::{
    AccessContext, DenyReason, Grant, MemoryScope, NamespaceId, Operation, TenantId,
};

#[tokio::test]
async fn submit_and_get_require_the_job_scope_consolidate_grant() {
    let directory = tempfile::tempdir().expect("创建 Consolidation Core 测试目录");
    let engine = MemoryEngine::open(MemoryConfig::local(directory.path()))
        .await
        .expect("打开记忆引擎");
    let scope = MemoryScope::new(NamespaceId::new());
    let recall_only = context("reader", &scope, &[Operation::Recall]);

    let error = engine
        .submit_consolidation(&recall_only, spec(scope.clone(), JobKind::Replay))
        .await
        .expect_err("缺少 Consolidate Grant 必须拒绝提交");
    assert!(matches!(
        error,
        MemoryError::AccessDenied {
            operation: Operation::Consolidate,
            reason: DenyReason::MissingGrant,
        }
    ));

    let operator = context("operator", &scope, &[Operation::Consolidate]);
    let submitted = engine
        .submit_consolidation(&operator, spec(scope, JobKind::Replay))
        .await
        .expect("提交授权 Job");
    let loaded = engine
        .consolidation_job(&operator, submitted.id)
        .await
        .expect("按 ID 查询授权 Job");
    assert_eq!(loaded, submitted);
}

#[tokio::test]
async fn a_tenant_cannot_read_another_tenant_job_by_id() {
    let directory = tempfile::tempdir().expect("创建 Tenant Job 测试目录");
    let engine = MemoryEngine::open(MemoryConfig::local(directory.path()))
        .await
        .expect("打开记忆引擎");
    let namespace = NamespaceId::new();
    let mut scope_a = MemoryScope::new(namespace);
    scope_a.tenant_id = Some(TenantId::new());
    let mut scope_b = MemoryScope::new(namespace);
    scope_b.tenant_id = Some(TenantId::new());
    let context_a = context("tenant-a", &scope_a, &[Operation::Consolidate]);
    let context_b = context("tenant-b", &scope_b, &[Operation::Consolidate]);
    let foreign = engine
        .submit_consolidation(&context_b, spec(scope_b, JobKind::Replay))
        .await
        .expect("提交 Tenant B Job");

    assert!(matches!(
        engine.consolidation_job(&context_a, foreign.id).await,
        Err(MemoryError::AccessDenied {
            operation: Operation::Consolidate,
            reason: DenyReason::CrossTenantRequiresAdmin,
        })
    ));
}

#[tokio::test]
async fn lifecycle_and_dispatch_cover_every_consolidation_handler() {
    let directory = tempfile::tempdir().expect("创建 Handler 分派测试目录");
    let engine = MemoryEngine::open(MemoryConfig::local(directory.path()))
        .await
        .expect("打开记忆引擎");
    let scope = MemoryScope::new(NamespaceId::new());
    let operator = context("operator", &scope, &[Operation::Consolidate]);

    for kind in all_job_kinds() {
        let submitted = engine
            .submit_consolidation(&operator, spec(scope.clone(), kind))
            .await
            .expect("提交 Handler Job");
        let completed = engine
            .run_consolidation(&operator, submitted.id)
            .await
            .expect("运行 Handler Job");
        assert_eq!(completed.spec.kind, kind);
        assert_eq!(completed.status, JobStatus::Completed);
    }

    let controlled = engine
        .submit_consolidation(&operator, spec(scope.clone(), JobKind::Replay))
        .await
        .expect("提交生命周期 Job");
    let paused = engine
        .pause_consolidation(&operator, controlled.id)
        .await
        .expect("暂停 Job");
    assert_eq!(paused.status, JobStatus::Paused);
    let resumed = engine
        .resume_consolidation(&operator, controlled.id)
        .await
        .expect("恢复 Job");
    assert_eq!(resumed.status, JobStatus::Pending);
    assert!(matches!(
        engine.retry_consolidation(&operator, controlled.id).await,
        Err(MemoryError::Consolidation(_))
    ));
    let cancelled = engine
        .cancel_consolidation(&operator, controlled.id)
        .await
        .expect("取消 Job");
    assert_eq!(cancelled.status, JobStatus::Cancelled);
}

#[tokio::test]
async fn pending_job_survives_engine_restart_and_can_run() {
    let directory = tempfile::tempdir().expect("创建 Job 重启测试目录");
    let config = MemoryConfig::local(directory.path());
    let scope = MemoryScope::new(NamespaceId::new());
    let operator = context("operator", &scope, &[Operation::Consolidate]);
    let engine = MemoryEngine::open(config.clone())
        .await
        .expect("打开记忆引擎");
    let submitted = engine
        .submit_consolidation(&operator, spec(scope, JobKind::Replay))
        .await
        .expect("提交待恢复 Job");
    engine.close().await;

    let reopened = MemoryEngine::open(config).await.expect("重启记忆引擎");
    let loaded = reopened
        .consolidation_job(&operator, submitted.id)
        .await
        .expect("重启后读取 Job");
    assert_eq!(loaded.status, JobStatus::Pending);
    let completed = reopened
        .run_consolidation(&operator, submitted.id)
        .await
        .expect("重启后运行 Job");
    assert_eq!(completed.status, JobStatus::Completed);
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

fn spec(scope: MemoryScope, kind: JobKind) -> JobSpec {
    JobSpec {
        kind,
        scope,
        cursor: 0,
        limits: ResourceLimits {
            max_batch_size: 16,
            max_cpu_ms: 1_000,
            max_io_ops: 1_000,
            max_runtime_ms: 5_000,
            max_attempts: 2,
        },
    }
}

const fn all_job_kinds() -> [JobKind; 12] {
    [
        JobKind::Replay,
        JobKind::Deduplication,
        JobKind::EdgeReinforcement,
        JobKind::EdgeWeakening,
        JobKind::SurfaceDiscovery,
        JobKind::SurfaceMerge,
        JobKind::SurfaceSplit,
        JobKind::SemanticPromotion,
        JobKind::ConflictDetection,
        JobKind::StaleStateDetection,
        JobKind::ColdTierMovement,
        JobKind::Rebuild,
    ]
}
