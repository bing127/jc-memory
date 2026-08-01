//! Embedded `CommandBus` 与直接 Core 调用的一致性契约。

mod common;

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use jc_memory_core::{CommandBus, CommandError, EmbeddedCommandBus, MemoryConfig, MemoryEngine};
use jc_memory_protocol::{
    API_VERSION_V1, AccessContext, AccessScope, CommandErrorCode, CommitCommand, FragmentContent,
    Grant, MemoryCommand, MemoryRequest, MemoryResponse, MutationAction, MutationCommand,
    ObserveCommand, ObserveResult, Operation, Perspective, RecallCommand, RecallLimits, RecallMode,
    ResponseEnvelope, TenantId, VerificationMethod,
};
use jc_memory_storage::{RepositoryFailpoint, StorageError};

#[tokio::test]
async fn observe_command_matches_direct_engine_canonical_json() {
    let data_dir = tempfile::tempdir().expect("创建引擎数据目录");
    let engine = MemoryEngine::open(MemoryConfig::local(data_dir.path()))
        .await
        .expect("打开记忆引擎");
    let fragment = common::observation("CommandBus Observe 一致性证据");
    let scope = fragment.scope.clone();
    let mut access = common::access_context(&scope);
    access.request_id = "req-observe-command-bus".to_owned();

    // 先固定幂等记录，使直接路径与 CommandBus 路径都返回同一持久化结果。
    engine
        .observe(&access, fragment.clone())
        .await
        .expect("预置权威记录");
    let direct = engine
        .observe(&access, fragment.clone())
        .await
        .expect("直接调用 Observe");
    let request = MemoryRequest {
        api_version: API_VERSION_V1.to_owned(),
        request_id: access.request_id.clone(),
        access_scope: AccessScope::from(scope),
        command: MemoryCommand::Observe(ObserveCommand { fragment }),
    };
    let direct = ResponseEnvelope::for_request(
        &request,
        MemoryResponse::Observe(ObserveResult {
            fragment_id: direct.fragment_id,
            event_id: direct.event_id,
            sequence: direct.sequence,
            content_hash: direct.content_hash,
            created: direct.created,
            quarantined: direct.quarantined,
        }),
    );

    let bus = EmbeddedCommandBus::new(engine);
    let dispatched = bus
        .execute(&access, request)
        .await
        .expect("CommandBus 执行 Observe");

    assert_eq!(
        direct.canonical_json().expect("序列化直接调用响应"),
        dispatched.canonical_json().expect("序列化 CommandBus 响应")
    );
}

#[tokio::test]
async fn commit_command_appends_all_runtime_fragments_without_fact_promotion() {
    let data_dir = tempfile::tempdir().expect("创建引擎数据目录");
    let engine = MemoryEngine::open(MemoryConfig::local(data_dir.path()))
        .await
        .expect("打开记忆引擎");
    let first = common::observation("Commit 当前观察");
    let scope = first.scope.clone();
    let second = jc_memory_protocol::CognitiveFragment {
        id: jc_memory_protocol::FragmentId::new(),
        scope: scope.clone(),
        session_id: None,
        trace_id: None,
        kind: jc_memory_protocol::FragmentKind::ToolResult,
        content: FragmentContent::Json {
            value: serde_json::json!({"ok": true}),
        },
        source: first.source.clone(),
        perspective: first.perspective,
        occurred_at: first.occurred_at,
        observed_at: first.observed_at,
        confidence: 1.0,
        importance: 0.5,
        validity: jc_memory_protocol::ValidityStatus::Unverified,
        security_label: jc_memory_protocol::SecurityLabel::Internal,
        evidence: Vec::new(),
        metadata: serde_json::Map::new(),
    };
    let mut access = common::access_context(&scope);
    access.request_id = "req-commit-command-bus".to_owned();
    access
        .grants
        .insert(Grant::for_scope(Operation::Mutate, &scope));
    let request = MemoryRequest {
        api_version: API_VERSION_V1.to_owned(),
        request_id: access.request_id.clone(),
        access_scope: AccessScope::from(scope.clone()),
        command: MemoryCommand::Commit(CommitCommand {
            fragments: vec![first.clone(), second.clone()],
        }),
    };

    let response = EmbeddedCommandBus::new(engine.clone())
        .execute(&access, request)
        .await
        .expect("执行 Commit");
    let MemoryResponse::Commit(result) = response.response else {
        panic!("Commit 必须返回 CommitResult");
    };
    assert_eq!(result.receipts.len(), 2);
    assert!(result.receipts.iter().all(|receipt| receipt.created));
    let first_stored = engine
        .fragment(&access, first.id)
        .await
        .expect("读取 Commit 第一条记录");
    let second_stored = engine
        .fragment(&access, second.id)
        .await
        .expect("读取 Commit 第二条记录");
    assert_eq!(
        first_stored.kind,
        jc_memory_protocol::FragmentKind::Observation
    );
    assert_eq!(
        second_stored.kind,
        jc_memory_protocol::FragmentKind::ToolResult
    );
    assert_ne!(first_stored.kind, jc_memory_protocol::FragmentKind::Fact);
}

#[tokio::test]
async fn commit_batch_rolls_back_all_fragments_when_late_append_fails() {
    let data_dir = tempfile::tempdir().expect("创建引擎数据目录");
    let attempts = Arc::new(AtomicUsize::new(0));
    let engine = MemoryEngine::open_with_repository_failpoint(
        MemoryConfig::local(data_dir.path()),
        Arc::new(FailOnSecondCommit {
            attempts: Arc::clone(&attempts),
        }),
    )
    .await
    .expect("打开带故障点的引擎");
    let first = common::observation("Commit 第一条应回滚");
    let second = common::observation_in_scope(first.scope.clone(), "Commit 第二条触发失败");
    let mut access = common::access_context(&first.scope);
    access.request_id = "req-commit-rollback".into();
    access
        .grants
        .insert(Grant::for_scope(Operation::Mutate, &first.scope));
    let request = MemoryRequest {
        api_version: API_VERSION_V1.into(),
        request_id: access.request_id.clone(),
        access_scope: AccessScope::from(first.scope.clone()),
        command: MemoryCommand::Commit(CommitCommand {
            fragments: vec![first.clone(), second.clone()],
        }),
    };

    let result = EmbeddedCommandBus::new(engine.clone())
        .execute(&access, request)
        .await;
    assert!(result.is_err(), "第二条 Fragment 应触发提交失败");
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
    assert!(matches!(
        engine.fragment(&access, first.id).await,
        Err(jc_memory_core::MemoryError::NotFound { .. })
    ));
    assert!(matches!(
        engine.fragment(&access, second.id).await,
        Err(jc_memory_core::MemoryError::NotFound { .. })
    ));
}

#[derive(Debug)]
struct FailOnSecondCommit {
    attempts: Arc<AtomicUsize>,
}

impl RepositoryFailpoint for FailOnSecondCommit {
    fn before_commit(&self) -> Result<(), StorageError> {
        let attempt = self.attempts.fetch_add(1, Ordering::SeqCst) + 1;
        if attempt >= 2 {
            Err(StorageError::InjectedFailure {
                point: "commit_batch_second_append",
            })
        } else {
            Ok(())
        }
    }

    fn before_batch_commit(&self, written: usize) -> Result<(), StorageError> {
        assert_eq!(written, 2);
        self.attempts.fetch_add(1, Ordering::SeqCst);
        Err(StorageError::InjectedFailure {
            point: "commit_batch_second_append",
        })
    }
}

#[tokio::test]
async fn command_bus_rejects_scope_not_bound_to_access_context() {
    let data_dir = tempfile::tempdir().expect("创建引擎数据目录");
    let engine = MemoryEngine::open(MemoryConfig::local(data_dir.path()))
        .await
        .expect("打开记忆引擎");
    let fragment = common::observation("不得越过 AccessContext Scope");
    let mut access = common::access_context(&fragment.scope);
    access.request_id = "req-scope-mismatch".to_owned();
    let request = request(&access, fragment);
    let mut request = request;
    request.access_scope = AccessScope::from(jc_memory_protocol::MemoryScope::new(
        jc_memory_protocol::NamespaceId::new(),
    ));

    let error = EmbeddedCommandBus::new(engine)
        .execute(&access, request)
        .await
        .expect_err("拒绝未绑定到服务端授权上下文的 Scope");

    assert!(matches!(error, CommandError::AccessScopeMismatch));
    assert_eq!(error.code(), CommandErrorCode::InvalidArgument);
}

#[tokio::test]
async fn command_bus_keeps_acl_error_as_source() {
    let data_dir = tempfile::tempdir().expect("创建引擎数据目录");
    let engine = MemoryEngine::open(MemoryConfig::local(data_dir.path()))
        .await
        .expect("打开记忆引擎");
    let fragment = common::observation("未授权请求不得绕过 ACL");
    let mut access = AccessContext::new("unauthorized", fragment.scope.clone());
    access.request_id = "req-missing-grant".to_owned();
    let request = request(&access, fragment);

    let error = EmbeddedCommandBus::new(engine)
        .execute(&access, request)
        .await
        .expect_err("缺少 Observe Grant 时拒绝写入");

    assert_eq!(error.code(), CommandErrorCode::PermissionDenied);
    assert!(matches!(
        error,
        CommandError::Engine {
            source: jc_memory_core::MemoryError::AccessDenied { .. },
            ..
        }
    ));
}

#[tokio::test]
async fn command_bus_rejects_request_id_not_bound_to_access_context() {
    let data_dir = tempfile::tempdir().expect("创建引擎数据目录");
    let engine = MemoryEngine::open(MemoryConfig::local(data_dir.path()))
        .await
        .expect("打开记忆引擎");
    let fragment = common::observation("request_id 必须由服务端上下文绑定");
    let mut access = common::access_context(&fragment.scope);
    access.request_id = "req-authorized".to_owned();
    let mut request = request(&access, fragment);
    request.request_id = "req-substituted".to_owned();

    let error = EmbeddedCommandBus::new(engine)
        .execute(&access, request)
        .await
        .expect_err("拒绝替换 request_id");

    assert!(matches!(error, CommandError::RequestIdMismatch));
    assert_eq!(error.code(), CommandErrorCode::InvalidArgument);
}

#[tokio::test]
async fn envelope_scope_cannot_carry_fragment_from_administered_foreign_scope() {
    let data_dir = tempfile::tempdir().expect("创建引擎数据目录");
    let engine = MemoryEngine::open(MemoryConfig::local(data_dir.path()))
        .await
        .expect("打开记忆引擎");
    let mut fragment = common::observation("Envelope A 不得写入 Fragment B");
    let envelope_scope = fragment.scope.clone();
    let mut fragment_scope = envelope_scope.clone();
    fragment_scope.tenant_id = Some(TenantId::new());
    fragment.scope.clone_from(&fragment_scope);
    let mut access = common::access_context(&envelope_scope);
    access.request_id = "req-fragment-scope-substitution".to_owned();
    access
        .grants
        .insert(Grant::for_scope(Operation::Admin, &fragment_scope));
    let request = MemoryRequest {
        api_version: API_VERSION_V1.to_owned(),
        request_id: access.request_id.clone(),
        access_scope: AccessScope::from(envelope_scope),
        command: MemoryCommand::Observe(ObserveCommand { fragment }),
    };

    let error = EmbeddedCommandBus::new(engine)
        .execute(&access, request)
        .await
        .expect_err("拒绝 Envelope 内部替换 Fragment Scope");

    assert!(matches!(error, CommandError::AccessScopeMismatch));
}

#[tokio::test]
async fn command_bus_appends_correct_and_forget_mutations() {
    let data_dir = tempfile::tempdir().expect("创建引擎数据目录");
    let engine = MemoryEngine::open(MemoryConfig::local(data_dir.path()))
        .await
        .expect("打开记忆引擎");
    let original = common::observation("CommandBus 变更闭环原始记录");
    let scope = original.scope.clone();
    let target_fragment_id = original.id;
    let mut access = common::access_context(&scope);
    let bus = EmbeddedCommandBus::new(engine);

    access.request_id = "req-command-bus-observe".to_owned();
    bus.execute(
        &access,
        MemoryRequest {
            api_version: API_VERSION_V1.to_owned(),
            request_id: access.request_id.clone(),
            access_scope: AccessScope::from(scope.clone()),
            command: MemoryCommand::Observe(ObserveCommand { fragment: original }),
        },
    )
    .await
    .expect("通过 CommandBus 写入原始记录");

    let mutation = MutationCommand {
        target_fragment_id,
        content: FragmentContent::Text {
            text: "CommandBus 追加式纠正记录".to_owned(),
        },
        state: None,
        source: common::observation("source").source,
        perspective: Perspective::System,
        occurred_at: chrono::Utc::now(),
        verification_method: VerificationMethod::Manual,
        reason: "CommandBus 变更测试".to_owned(),
    };
    access.request_id = "req-command-bus-correct".to_owned();
    let correct = bus
        .execute(
            &access,
            MemoryRequest {
                api_version: API_VERSION_V1.to_owned(),
                request_id: access.request_id.clone(),
                access_scope: AccessScope::from(scope.clone()),
                command: MemoryCommand::Correct(mutation.clone()),
            },
        )
        .await
        .expect("通过 CommandBus 追加纠正");
    assert!(matches!(
        correct.response,
        MemoryResponse::Mutation(ref result) if result.action == MutationAction::Correct
    ));

    access.request_id = "req-command-bus-forget".to_owned();
    let forget = bus
        .execute(
            &access,
            MemoryRequest {
                api_version: API_VERSION_V1.to_owned(),
                request_id: access.request_id.clone(),
                access_scope: AccessScope::from(scope),
                command: MemoryCommand::Forget(mutation),
            },
        )
        .await
        .expect("通过 CommandBus 追加遗忘");
    assert!(matches!(
        forget.response,
        MemoryResponse::Mutation(ref result) if result.action == MutationAction::Forget
    ));

    access.request_id = "req-command-bus-explain".to_owned();
    let explain = bus
        .execute(
            &access,
            MemoryRequest {
                api_version: API_VERSION_V1.to_owned(),
                request_id: access.request_id.clone(),
                access_scope: AccessScope::from(access.scope.clone()),
                command: MemoryCommand::Explain(RecallCommand {
                    mode: RecallMode::Exact,
                    text: "CommandBus 变更闭环原始记录".to_owned(),
                    entities: Vec::new(),
                    at_time: None,
                    limits: RecallLimits {
                        max_seeds: 4,
                        max_neighbors: 4,
                        max_hops: 2,
                        max_nodes: 8,
                        timeout_ms: 100,
                    },
                }),
            },
        )
        .await
        .expect("通过 CommandBus 返回 Explain");
    assert!(matches!(
        explain.response,
        MemoryResponse::Explain(ref trace) if trace.limits.max_nodes == 8
    ));
}

fn request(
    access: &AccessContext,
    fragment: jc_memory_protocol::CognitiveFragment,
) -> MemoryRequest {
    MemoryRequest {
        api_version: API_VERSION_V1.to_owned(),
        request_id: access.request_id.clone(),
        access_scope: AccessScope::from(fragment.scope.clone()),
        command: MemoryCommand::Observe(ObserveCommand { fragment }),
    }
}
