//! Embedded `CommandBus` 与直接 Core 调用的一致性契约。

mod common;

use jc_memory_core::{CommandBus, CommandError, EmbeddedCommandBus, MemoryConfig, MemoryEngine};
use jc_memory_protocol::{
    API_VERSION_V1, AccessContext, AccessScope, CommandErrorCode, FragmentContent, Grant,
    MemoryCommand, MemoryRequest, MemoryResponse, MutationAction, MutationCommand, ObserveCommand,
    ObserveResult, Operation, Perspective, ResponseEnvelope, TenantId, VerificationMethod,
};

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
