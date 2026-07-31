#![cfg(unix)]
//! Sidecar 本地认证、单实例和生命周期契约测试。

use std::os::unix::fs::PermissionsExt as _;

use jc_memory_core::{EmbeddedCommandBus, MemoryConfig, MemoryEngine};
use jc_memory_protocol::{
    API_VERSION_V1, AccessScope, CommitCommand, FragmentContent, FragmentKind, FragmentSource,
    MemoryCommand, MemoryRequest, MemoryScope, NamespaceId, Perspective, SecurityLabel, SourceType,
    ValidityStatus,
};
use jc_memory_sidecar::{
    DaemonCommand, InstanceLock, LockError, SidecarConfig, SidecarDaemon, TokenStore, send_command,
};

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
