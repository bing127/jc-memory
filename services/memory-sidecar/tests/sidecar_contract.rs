#![cfg(unix)]
//! Sidecar 本地认证、单实例和生命周期契约测试。

use std::os::unix::fs::PermissionsExt as _;

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
