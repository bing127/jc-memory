#![cfg(unix)]
//! Sidecar 本地认证、单实例和生命周期契约测试。

use std::os::unix::fs::PermissionsExt as _;
use std::sync::Arc;

use async_trait::async_trait;
use jc_memory_core::{CommandBus, CommandError, EmbeddedCommandBus, MemoryConfig, MemoryEngine};
use jc_memory_protocol::{
    API_VERSION_V1, AccessScope, CommitCommand, FragmentContent, FragmentKind, FragmentSource,
    MemoryCommand, MemoryRequest, MemoryScope, NamespaceId, Perspective, SecurityLabel, SourceType,
    StatsCommand, ValidityStatus,
};
use jc_memory_sidecar::{
    DaemonCommand, InstanceLock, LockError, SidecarConfig, SidecarDaemon, TokenStore, send_command,
};
use tokio::sync::Notify;

#[derive(Clone)]
struct BlockingBus {
    started: Arc<Notify>,
    release: Arc<Notify>,
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
