//! Sidecar 本地认证、单实例和生命周期契约测试。
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt as _;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;
use jc_memory_core::{CommandBus, CommandError, EmbeddedCommandBus, MemoryConfig, MemoryEngine};
use jc_memory_protocol::{
    API_VERSION_V1, AccessScope, CognitiveFragment, CommandErrorCode, CommitCommand, EvidenceId,
    EvidenceRef, FragmentContent, FragmentId, FragmentKind, FragmentSource, MemoryCommand,
    MemoryRequest, MemoryResponse, MemoryScope, NamespaceId, ObserveCommand, Perspective,
    RecallCommand, RecallLimits, RecallMode, ResponseEnvelope, SecurityLabel, SourceType,
    StatsCommand, ValidityStatus, VerificationMethod,
};
use jc_memory_sidecar::{
    DaemonCommand, InstanceLock, LockError, SidecarConfig, SidecarDaemon, SidecarStream,
    TokenStore, send_command,
};
use tokio::{io::AsyncWriteExt as _, sync::Notify};

#[derive(Clone)]
struct BlockingBus {
    started: Arc<Notify>,
    release: Arc<Notify>,
}

#[derive(Clone)]
struct RecordingBus {
    principal: Arc<tokio::sync::Mutex<Option<String>>>,
}

#[derive(Clone)]
struct ShutdownBus {
    closed: Arc<AtomicBool>,
}

#[derive(Clone)]
struct FailingShutdownBus;

#[async_trait]
impl CommandBus for FailingShutdownBus {
    async fn execute(
        &self,
        _access: &jc_memory_protocol::AccessContext,
        _request: MemoryRequest,
    ) -> Result<jc_memory_protocol::ResponseEnvelope, CommandError> {
        Err(CommandError::RequestIdMismatch)
    }

    async fn shutdown(&self) -> Result<(), CommandError> {
        Err(CommandError::transport(
            CommandErrorCode::Internal,
            "注入关闭失败",
        ))
    }
}

#[async_trait]
impl CommandBus for ShutdownBus {
    async fn execute(
        &self,
        _access: &jc_memory_protocol::AccessContext,
        _request: MemoryRequest,
    ) -> Result<jc_memory_protocol::ResponseEnvelope, CommandError> {
        Err(CommandError::RequestIdMismatch)
    }

    async fn shutdown(&self) -> Result<(), CommandError> {
        self.closed.store(true, Ordering::SeqCst);
        Ok(())
    }
}

#[async_trait]
impl CommandBus for RecordingBus {
    async fn execute(
        &self,
        access: &jc_memory_protocol::AccessContext,
        _request: MemoryRequest,
    ) -> Result<jc_memory_protocol::ResponseEnvelope, CommandError> {
        *self.principal.lock().await = Some(access.principal_id.clone());
        Err(CommandError::RequestIdMismatch)
    }
}

#[async_trait]
impl CommandBus for BlockingBus {
    async fn execute(
        &self,
        _access: &jc_memory_protocol::AccessContext,
        _request: MemoryRequest,
    ) -> Result<jc_memory_protocol::ResponseEnvelope, CommandError> {
        self.started.notify_one();
        self.release.notified().await;
        Err(CommandError::RequestIdMismatch)
    }
}

#[tokio::test]
async fn token_file_is_fixed_length_owner_only_and_constant_time_comparable() {
    let directory = tempfile::tempdir().expect("创建 Token 目录");
    let path = directory.path().join("sidecar.token");
    let token = TokenStore::load_or_create(&path).await.expect("创建 Token");
    let bytes = tokio::fs::read(&path).await.expect("读取 Token");
    assert_eq!(bytes.len(), 32);
    assert!(token.constant_time_eq(&bytes));
    assert!(!token.constant_time_eq(&[0; 32]));
    assert_eq!(
        tokio::fs::metadata(path)
            .await
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}

#[test]
fn second_sidecar_instance_is_rejected_and_lock_is_released_after_drop() {
    let directory = tempfile::tempdir().expect("创建锁目录");
    let path = directory.path().join("sidecar.lock");
    let first = InstanceLock::acquire(&path).expect("取得首个实例锁");
    assert!(matches!(
        InstanceLock::acquire(&path),
        Err(LockError::AlreadyRunning)
    ));
    drop(first);
    InstanceLock::acquire(&path).expect("实例退出后重新取得锁");
}

#[test]
fn stale_lock_metadata_is_detected_when_the_os_lock_is_available() {
    let directory = tempfile::tempdir().expect("创建锁目录");
    let path = directory.path().join("sidecar.lock");
    std::fs::write(&path, "pid=424242\nstarted_at=1\n").expect("写入旧锁元数据");
    let lock = InstanceLock::acquire(&path).expect("取得可用锁");
    assert!(lock.recovered_stale_metadata());
}

#[test]
fn jcmem_start_status_and_stop_control_a_real_jcmemd_process() {
    let directory = tempfile::tempdir().expect("创建进程测试目录");
    let start = Command::new(env!("CARGO_BIN_EXE_jcmem"))
        .env("JC_MEMORY_SIDECAR_DATA_DIR", directory.path())
        .env("JC_MEMORY_SIDECAR_DAEMON_BIN", env!("CARGO_BIN_EXE_jcmemd"))
        .arg("start")
        .output()
        .expect("启动 jcmem");
    assert!(start.status.success(), "start stderr: {:?}", start.stderr);
    assert!(String::from_utf8_lossy(&start.stdout).contains("\"status\":\"started\""));

    let status = Command::new(env!("CARGO_BIN_EXE_jcmem"))
        .env("JC_MEMORY_SIDECAR_DATA_DIR", directory.path())
        .arg("status")
        .output()
        .expect("查询 Sidecar 状态");
    assert!(
        status.status.success(),
        "status stderr: {:?}",
        status.stderr
    );
    assert!(String::from_utf8_lossy(&status.stdout).contains("\"status\":\"ready\""));

    let stop = Command::new(env!("CARGO_BIN_EXE_jcmem"))
        .env("JC_MEMORY_SIDECAR_DATA_DIR", directory.path())
        .arg("stop")
        .output()
        .expect("停止 Sidecar");
    assert!(stop.status.success(), "stop stderr: {:?}", stop.stderr);
    assert!(String::from_utf8_lossy(&stop.stdout).contains("\"status\":\"stopping\""));
}

#[tokio::test]
async fn daemon_rejects_wrong_token_supports_status_and_stops_cleanly() {
    let directory = tempfile::tempdir().expect("创建 Sidecar 目录");
    let config = SidecarConfig::local(directory.path());
    let daemon = SidecarDaemon::bind(config.clone())
        .await
        .expect("绑定 Sidecar");
    let valid = TokenStore::load_or_create(&config.token_file)
        .await
        .expect("读取 Sidecar Token");
    let wrong = TokenStore::from_bytes(vec![0; 32]).expect("创建错误 Token");
    let task = tokio::spawn(daemon.run());

    let unauthorized = send_command(&config, &wrong, DaemonCommand::Status)
        .await
        .expect("错误 Token 请求返回");
    assert_eq!(unauthorized.status, "unauthorized");
    assert!(!unauthorized.ok);

    let status = send_command(&config, &valid, DaemonCommand::Status)
        .await
        .expect("状态请求");
    assert_eq!(status.status, "ready");
    assert!(status.ok);

    let stopping = send_command(&config, &valid, DaemonCommand::Stop)
        .await
        .expect("停止请求");
    assert_eq!(stopping.status, "stopping");
    task.await.expect("等待 Sidecar").expect("Sidecar 正常退出");

    let restarted = SidecarDaemon::bind(config).await.expect("Sidecar 可重启");
    let stop_task = tokio::spawn(restarted.run());
    stop_task.abort();
}

#[tokio::test]
async fn authenticated_execute_forwards_commit_to_shared_command_bus() {
    let directory = tempfile::tempdir().expect("创建 Sidecar 测试目录");
    let config = SidecarConfig::local(directory.path());
    let engine = MemoryEngine::open(MemoryConfig::local(directory.path()))
        .await
        .expect("打开 Sidecar 测试引擎");
    let bus = std::sync::Arc::new(EmbeddedCommandBus::new(engine));
    let daemon = SidecarDaemon::bind_with_bus(config.clone(), bus)
        .await
        .expect("绑定带 CommandBus 的 Sidecar");
    let token = daemon.token().clone();
    let scope = MemoryScope::new(NamespaceId::new());
    let now = chrono::Utc::now();
    let fragment = jc_memory_protocol::CognitiveFragment {
        id: jc_memory_protocol::FragmentId::new(),
        scope: scope.clone(),
        session_id: None,
        trace_id: None,
        kind: FragmentKind::ToolResult,
        content: FragmentContent::Text {
            text: "Sidecar Commit 转发".into(),
        },
        source: FragmentSource {
            source_type: SourceType::Tool,
            source_id: "sidecar-test".into(),
            actor_id: None,
            observer_id: None,
            tool_id: Some("fixture.tool".into()),
            uri: None,
        },
        perspective: Perspective::Tool,
        occurred_at: now,
        observed_at: now,
        confidence: 1.0,
        importance: 0.5,
        validity: ValidityStatus::Unverified,
        security_label: SecurityLabel::Internal,
        evidence: Vec::new(),
        metadata: serde_json::Map::new(),
    };
    let request = MemoryRequest {
        api_version: API_VERSION_V1.into(),
        request_id: "sidecar-commit-request".into(),
        access_scope: AccessScope::from(scope),
        command: MemoryCommand::Commit(CommitCommand {
            fragments: vec![fragment],
        }),
    };
    let task = tokio::spawn(daemon.run());
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    let response = send_command(
        &config,
        &token,
        DaemonCommand::Execute {
            request: Box::new(request),
        },
    )
    .await
    .expect("发送 Sidecar Commit");
    assert!(response.ok);
    assert_eq!(response.status, "executed");
    assert!(matches!(
        response.response.expect("Commit 响应").response,
        jc_memory_protocol::MemoryResponse::Commit(_)
    ));
    send_command(&config, &token, DaemonCommand::Stop)
        .await
        .expect("停止 Sidecar");
    task.await
        .expect("等待 Sidecar 任务")
        .expect("Sidecar 正常退出");
}

#[tokio::test]
async fn sidecar_recall_preserves_non_empty_current_and_historical_state() {
    let directory = tempfile::tempdir().expect("创建 Sidecar State 目录");
    let config = SidecarConfig::local(directory.path().join("control"));
    let engine = MemoryEngine::open(MemoryConfig::local(directory.path()))
        .await
        .expect("打开 Sidecar State 引擎");
    let daemon =
        SidecarDaemon::bind_with_bus(config.clone(), Arc::new(EmbeddedCommandBus::new(engine)))
            .await
            .expect("绑定 Sidecar State Daemon");
    let token = daemon.token().clone();
    let task = tokio::spawn(daemon.run());
    let scope = MemoryScope::new(NamespaceId::new());
    let blue_fragment_id = "00000000-0000-7000-8000-000000000111"
        .parse::<FragmentId>()
        .unwrap();
    let canary_evidence_id = "00000000-0000-7000-8000-000000000151"
        .parse::<EvidenceId>()
        .unwrap();

    let blue = sidecar_execute(
        &config,
        &token,
        state_observe_request(
            &scope,
            "sidecar-state-blue",
            blue_fragment_id,
            "blue_green",
            "生产环境先使用蓝绿发布",
            "2026-07-30T00:00:00Z",
            None,
        ),
    )
    .await;
    let canary = sidecar_execute(
        &config,
        &token,
        state_observe_request(
            &scope,
            "sidecar-state-canary",
            "00000000-0000-7000-8000-000000000112".parse().unwrap(),
            "canary",
            "验证通过后切换为金丝雀发布",
            "2026-07-31T00:00:00Z",
            Some((canary_evidence_id, blue_fragment_id)),
        ),
    )
    .await;
    let blue_event_id = observe_event_id(&blue);
    let canary_event_id = observe_event_id(&canary);
    let recalled = sidecar_execute(
        &config,
        &token,
        state_recall_request(&scope, "sidecar-state-recall"),
    )
    .await;
    let MemoryResponse::Recall(packet) = recalled.response else {
        panic!("Sidecar State Recall 必须返回 MemoryPacket")
    };

    assert_eq!(packet.current_states[0].key, "project.deployment_strategy");
    assert_eq!(packet.current_states[0].value, serde_json::json!("canary"));
    assert_eq!(
        packet.historical_states[0].value,
        serde_json::json!("blue_green")
    );
    assert_eq!(
        packet.current_states[0].source_event_id,
        Some(canary_event_id)
    );
    assert_eq!(
        packet.historical_states[0].source_event_id,
        Some(blue_event_id)
    );
    assert_eq!(
        packet.current_states[0].evidence[0].evidence_ref_id,
        Some(canary_evidence_id)
    );
    let contract = contract_state_values("recall-current-state.json");
    assert_eq!(contract, serde_json::json!(["canary", "blue_green"]));

    send_command(&config, &token, DaemonCommand::Stop)
        .await
        .expect("停止 Sidecar State Daemon");
    task.await
        .expect("等待 Sidecar State Daemon")
        .expect("Sidecar State Daemon 正常退出");
}

#[tokio::test]
async fn execute_as_forwards_the_authenticated_principal_to_core() {
    let directory = tempfile::tempdir().expect("创建身份转发目录");
    let config = SidecarConfig::local(directory.path());
    let principal = Arc::new(tokio::sync::Mutex::new(None));
    let daemon = SidecarDaemon::bind_with_bus(
        config.clone(),
        Arc::new(RecordingBus {
            principal: Arc::clone(&principal),
        }),
    )
    .await
    .expect("绑定身份转发 Sidecar");
    let token = daemon.token().clone();
    let scope = MemoryScope::new(NamespaceId::new());
    let request = MemoryRequest {
        api_version: API_VERSION_V1.into(),
        request_id: "sidecar-principal-forwarding".into(),
        access_scope: AccessScope::from(scope),
        command: MemoryCommand::Stats(StatsCommand),
    };
    let task = tokio::spawn(daemon.run());
    let response = send_command(
        &config,
        &token,
        DaemonCommand::ExecuteAs {
            principal_id: "mcp-client".into(),
            request: Box::new(request),
        },
    )
    .await
    .expect("发送身份转发请求");
    assert_eq!(response.status, "error");
    assert_eq!(principal.lock().await.as_deref(), Some("mcp-client"));
    send_command(&config, &token, DaemonCommand::Stop)
        .await
        .expect("停止身份转发 Sidecar");
    task.await.expect("等待 Sidecar").expect("Sidecar 正常退出");
}

#[tokio::test]
async fn clean_stop_explicitly_shuts_down_the_command_bus() {
    let directory = tempfile::tempdir().expect("创建关闭测试目录");
    let config = SidecarConfig::local(directory.path());
    let closed = Arc::new(AtomicBool::new(false));
    let daemon = SidecarDaemon::bind_with_bus(
        config.clone(),
        Arc::new(ShutdownBus {
            closed: Arc::clone(&closed),
        }),
    )
    .await
    .expect("绑定关闭测试 Sidecar");
    let token = daemon.token().clone();
    let task = tokio::spawn(daemon.run());
    send_command(&config, &token, DaemonCommand::Stop)
        .await
        .expect("停止关闭测试 Sidecar");
    task.await.expect("等待 Sidecar").expect("Sidecar 正常退出");
    assert!(closed.load(Ordering::SeqCst));
}

#[tokio::test]
async fn unauthenticated_idle_connection_does_not_block_clean_stop() {
    let directory = tempfile::tempdir().expect("创建空闲连接测试目录");
    let config = SidecarConfig::local(directory.path());
    let closed = Arc::new(AtomicBool::new(false));
    let daemon = SidecarDaemon::bind_with_bus(
        config.clone(),
        Arc::new(ShutdownBus {
            closed: Arc::clone(&closed),
        }),
    )
    .await
    .expect("绑定空闲连接测试 Sidecar");
    let token = daemon.token().clone();
    let mut task = tokio::spawn(daemon.run());
    let _idle = SidecarStream::connect(&config.endpoint)
        .await
        .expect("建立未认证空闲连接");

    send_command(&config, &token, DaemonCommand::Stop)
        .await
        .expect("停止空闲连接测试 Sidecar");
    let stopped = tokio::time::timeout(std::time::Duration::from_millis(200), &mut task).await;
    if stopped.is_err() {
        task.abort();
    }
    assert!(stopped.is_ok(), "未认证空闲连接阻塞了 Sidecar 关闭");
    assert!(closed.load(Ordering::SeqCst));
}

#[tokio::test]
async fn malformed_connection_does_not_skip_clean_shutdown() {
    let directory = tempfile::tempdir().expect("创建畸形连接测试目录");
    let config = SidecarConfig::local(directory.path());
    let closed = Arc::new(AtomicBool::new(false));
    let daemon = SidecarDaemon::bind_with_bus(
        config.clone(),
        Arc::new(ShutdownBus {
            closed: Arc::clone(&closed),
        }),
    )
    .await
    .expect("绑定畸形连接测试 Sidecar");
    let token = daemon.token().clone();
    let task = tokio::spawn(daemon.run());
    let malformed = SidecarStream::connect(&config.endpoint)
        .await
        .expect("建立畸形连接");
    malformed
        .into_inner()
        .write_all(b"{invalid-json}\n")
        .await
        .expect("发送畸形控制消息");
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;

    assert!(!task.is_finished(), "单个畸形连接终止了 Sidecar");
    send_command(&config, &token, DaemonCommand::Stop)
        .await
        .expect("停止畸形连接测试 Sidecar");
    task.await.expect("等待 Sidecar").expect("Sidecar 正常退出");
    assert!(closed.load(Ordering::SeqCst));
}

#[tokio::test]
async fn shutdown_failure_keeps_the_checkpoint_running() {
    let directory = tempfile::tempdir().expect("创建关闭失败测试目录");
    let config = SidecarConfig::local(directory.path());
    let daemon = SidecarDaemon::bind_with_bus(config.clone(), Arc::new(FailingShutdownBus))
        .await
        .expect("绑定关闭失败测试 Sidecar");
    let token = daemon.token().clone();
    let task = tokio::spawn(daemon.run());

    send_command(&config, &token, DaemonCommand::Stop)
        .await
        .expect("请求停止关闭失败测试 Sidecar");
    let error = task
        .await
        .expect("等待关闭失败测试 Sidecar")
        .expect_err("关闭失败必须由守护进程返回");
    assert!(matches!(error, jc_memory_sidecar::SidecarError::Command(_)));
    let checkpoint: serde_json::Value = serde_json::from_slice(
        &tokio::fs::read(&config.checkpoint_file)
            .await
            .expect("读取失败后的生命周期检查点"),
    )
    .expect("解析失败后的生命周期检查点");
    assert_eq!(checkpoint["state"], "running");
}

#[tokio::test]
async fn lifecycle_checkpoint_detects_unclean_restart_and_records_clean_stop() {
    let directory = tempfile::tempdir().expect("创建检查点测试目录");
    let config = SidecarConfig::local(directory.path());
    let first = SidecarDaemon::bind(config.clone())
        .await
        .expect("首次绑定 Sidecar");
    assert!(!first.recovered_unclean_shutdown());
    drop(first);

    let restarted = SidecarDaemon::bind(config.clone())
        .await
        .expect("崩溃后重新绑定 Sidecar");
    assert!(restarted.recovered_unclean_shutdown());
    let token = restarted.token().clone();
    let task = tokio::spawn(restarted.run());
    send_command(&config, &token, DaemonCommand::Stop)
        .await
        .expect("干净停止 Sidecar");
    task.await.expect("等待 Sidecar").expect("Sidecar 正常退出");

    let checkpoint: serde_json::Value = serde_json::from_slice(
        &tokio::fs::read(&config.checkpoint_file)
            .await
            .expect("读取生命周期检查点"),
    )
    .expect("解析生命周期检查点");
    assert_eq!(checkpoint["version"], 1);
    assert_eq!(checkpoint["state"], "stopped");
}

#[tokio::test]
async fn second_instance_checks_the_os_lock_before_reading_checkpoint() {
    let directory = tempfile::tempdir().expect("创建锁顺序测试目录");
    let config = SidecarConfig::local(directory.path());
    let first = SidecarDaemon::bind(config.clone())
        .await
        .expect("绑定首个 Sidecar");
    tokio::fs::write(&config.checkpoint_file, b"{invalid-json")
        .await
        .expect("破坏共享检查点");

    let Err(error) = SidecarDaemon::bind(config).await else {
        panic!("第二实例不应取得锁");
    };
    assert!(matches!(
        error,
        jc_memory_sidecar::SidecarError::Lock(LockError::AlreadyRunning)
    ));
    drop(first);
}

#[tokio::test]
async fn stop_is_accepted_while_an_execute_is_in_flight_and_waits_for_release() {
    let directory = tempfile::tempdir().expect("创建 Sidecar 目录");
    let config = SidecarConfig::local(directory.path());
    let started = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let bus = Arc::new(BlockingBus {
        started: Arc::clone(&started),
        release: Arc::clone(&release),
    });
    let daemon = SidecarDaemon::bind_with_bus(config.clone(), bus)
        .await
        .expect("绑定阻塞测试 Sidecar");
    let token = daemon.token().clone();
    let scope = MemoryScope::new(NamespaceId::new());
    let request = MemoryRequest {
        api_version: API_VERSION_V1.into(),
        request_id: "sidecar-in-flight".into(),
        access_scope: AccessScope::from(scope),
        command: MemoryCommand::Stats(StatsCommand),
    };
    let daemon_task = tokio::spawn(daemon.run());
    let execute_config = config.clone();
    let execute_token = token.clone();
    let execute_task = tokio::spawn(async move {
        send_command(
            &execute_config,
            &execute_token,
            DaemonCommand::Execute {
                request: Box::new(request),
            },
        )
        .await
    });
    started.notified().await;

    let stop = tokio::time::timeout(
        std::time::Duration::from_millis(100),
        send_command(&config, &token, DaemonCommand::Stop),
    )
    .await
    .expect("Stop 必须在 Execute 完成前被接收")
    .expect("发送 Stop");
    assert_eq!(stop.status, "stopping");
    assert!(!daemon_task.is_finished());

    release.notify_one();
    let execute = execute_task
        .await
        .expect("等待在途 Execute")
        .expect("Execute 控制响应");
    assert_eq!(execute.status, "error");
    daemon_task
        .await
        .expect("等待 Sidecar 优雅关闭")
        .expect("Sidecar 正常关闭");
}

async fn sidecar_execute(
    config: &SidecarConfig,
    token: &TokenStore,
    request: MemoryRequest,
) -> ResponseEnvelope {
    let response = send_command(
        config,
        token,
        DaemonCommand::Execute {
            request: Box::new(request),
        },
    )
    .await
    .expect("执行 Sidecar State 命令");
    assert!(response.ok, "Sidecar State 命令失败: {}", response.status);
    response.response.expect("Sidecar State 命令必须返回响应")
}

fn state_observe_request(
    scope: &MemoryScope,
    request_id: &str,
    fragment_id: FragmentId,
    value: &str,
    reason: &str,
    occurred_at: &str,
    evidence: Option<(EvidenceId, FragmentId)>,
) -> MemoryRequest {
    let occurred_at = occurred_at.parse().unwrap();
    let mut metadata = serde_json::Map::new();
    metadata.insert(
        "state".to_owned(),
        serde_json::json!({
            "key": "project.deployment_strategy",
            "value": value,
            "reason": reason,
        }),
    );
    let fragment = CognitiveFragment {
        id: fragment_id,
        scope: scope.clone(),
        session_id: None,
        trace_id: None,
        kind: FragmentKind::Fact,
        content: FragmentContent::Text {
            text: format!("部署策略 {value}"),
        },
        source: FragmentSource {
            source_type: SourceType::User,
            source_id: "sidecar-state-contract".to_owned(),
            actor_id: None,
            observer_id: None,
            tool_id: None,
            uri: None,
        },
        perspective: Perspective::FirstPerson,
        occurred_at,
        observed_at: occurred_at,
        confidence: 1.0,
        importance: 0.5,
        validity: ValidityStatus::Unverified,
        security_label: SecurityLabel::Internal,
        evidence: evidence
            .into_iter()
            .map(|(id, fragment_id)| EvidenceRef {
                id,
                fragment_id: Some(fragment_id),
                event_id: None,
                method: VerificationMethod::Manual,
                note: Some("人工确认部署策略变更".to_owned()),
            })
            .collect(),
        metadata,
    };
    MemoryRequest {
        api_version: API_VERSION_V1.to_owned(),
        request_id: request_id.to_owned(),
        access_scope: AccessScope::from(scope.clone()),
        command: MemoryCommand::Observe(ObserveCommand { fragment }),
    }
}

fn state_recall_request(scope: &MemoryScope, request_id: &str) -> MemoryRequest {
    MemoryRequest {
        api_version: API_VERSION_V1.to_owned(),
        request_id: request_id.to_owned(),
        access_scope: AccessScope::from(scope.clone()),
        command: MemoryCommand::Recall(RecallCommand {
            mode: RecallMode::CurrentState,
            text: "project.deployment_strategy".to_owned(),
            entities: Vec::new(),
            at_time: None,
            limits: RecallLimits {
                max_seeds: 4,
                max_neighbors: 4,
                max_hops: 2,
                max_nodes: 8,
                timeout_ms: 500,
            },
        }),
    }
}

fn observe_event_id(response: &ResponseEnvelope) -> jc_memory_protocol::EventId {
    let MemoryResponse::Observe(result) = &response.response else {
        panic!("State Observe 必须返回 ObserveResult")
    };
    result.event_id
}

fn contract_state_values(name: &str) -> serde_json::Value {
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../sdk/contracts/fixtures")
        .join(name);
    let fixture: serde_json::Value =
        serde_json::from_slice(&std::fs::read(source).unwrap()).unwrap();
    serde_json::json!([
        fixture["response"]["response"]["payload"]["current_states"][0]["value"],
        fixture["response"]["response"]["payload"]["historical_states"][0]["value"],
    ])
}
