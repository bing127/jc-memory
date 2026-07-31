use std::{
    io,
    path::Path,
    process,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use jc_memory_core::{CommandBus, CommandError};
use jc_memory_protocol::{
    AccessContext, Grant, MemoryCommand, MemoryRequest, Operation, ResponseEnvelope,
};
use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncBufReadExt as _, AsyncWriteExt as _},
    sync::Mutex,
    task::JoinSet,
};
use tokio_util::sync::CancellationToken;

use crate::{
    InstanceLock, SidecarConfig, SidecarListener, SidecarStream, TokenStore, TransportError,
};

/// Sidecar 控制命令。
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DaemonCommand {
    /// 查询 Sidecar 是否运行。
    Status,
    /// 请求守护进程停止接收新连接并退出。
    Stop,
    /// 诊断本地文件和传输端点。
    Doctor,
    /// 通过本地认证转发一个共享 `MemoryRequest`。
    Execute {
        /// 需要转发到共享 `CommandBus` 的业务请求。
        request: Box<MemoryRequest>,
    },
    /// 通过本地认证转发一个带调用方主体的共享请求。
    ExecuteAs {
        /// 经过本地认证的调用方主体标识。
        principal_id: String,
        /// 需要转发到共享 `CommandBus` 的业务请求。
        request: Box<MemoryRequest>,
    },
}

/// Sidecar 控制响应。
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct DaemonResponse {
    /// 是否成功处理命令。
    pub ok: bool,
    /// 稳定状态文本。
    pub status: String,
    /// 诊断细节。
    pub detail: Option<String>,
    /// 业务命令成功时返回的共享协议响应。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response: Option<ResponseEnvelope>,
}

#[derive(Debug, Deserialize)]
struct DaemonRequest {
    token: String,
    command: DaemonCommand,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum LifecycleState {
    Running,
    Stopped,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct LifecycleCheckpoint {
    version: u32,
    state: LifecycleState,
    pid: u32,
    started_at_ms: u128,
}

/// Sidecar 生命周期错误。
#[derive(Debug, thiserror::Error)]
pub enum SidecarError {
    /// 配置不合法。
    #[error("invalid sidecar configuration: {0}")]
    Config(String),
    /// 文件系统错误。
    #[error("sidecar io error: {0}")]
    Io(#[from] io::Error),
    /// Token 文件错误。
    #[error(transparent)]
    Token(#[from] crate::TokenError),
    /// 实例锁错误。
    #[error(transparent)]
    Lock(#[from] crate::LockError),
    /// 本地传输错误。
    #[error(transparent)]
    Transport(#[from] TransportError),
    /// 关闭共享 `CommandBus` 失败。
    #[error(transparent)]
    Command(#[from] CommandError),
    /// 控制消息无法解析。
    #[error("invalid control message: {0}")]
    Json(#[from] serde_json::Error),
    /// 生命周期检查点不受支持。
    #[error("invalid lifecycle checkpoint: {0}")]
    Checkpoint(String),
}

/// 已取得实例锁并绑定本地端点的 Sidecar 守护进程。
pub struct SidecarDaemon {
    config: SidecarConfig,
    token: TokenStore,
    _lock: InstanceLock,
    listener: SidecarListener,
    stop: CancellationToken,
    active_connections: Arc<Mutex<usize>>,
    bus: Option<Arc<dyn CommandBus>>,
    checkpoint: LifecycleCheckpoint,
    recovered_unclean_shutdown: bool,
}

impl SidecarDaemon {
    /// 准备数据目录、Token、实例锁和本地监听端点。
    ///
    /// # Errors
    ///
    /// 配置、Token、实例锁或本地端点无法准备时返回错误。
    pub async fn bind(config: SidecarConfig) -> Result<Self, SidecarError> {
        config.validate().map_err(SidecarError::Config)?;
        config.prepare().await?;
        let lock = InstanceLock::acquire(&config.lock_file)?;
        let previous_checkpoint = load_checkpoint(&config.checkpoint_file).await?;
        let recovered_unclean_shutdown = previous_checkpoint
            .as_ref()
            .is_some_and(|checkpoint| checkpoint.state == LifecycleState::Running);
        let token = TokenStore::load_or_create(&config.token_file).await?;
        let listener = SidecarListener::bind(&config.endpoint).await?;
        let checkpoint = LifecycleCheckpoint {
            version: 1,
            state: LifecycleState::Running,
            pid: process::id(),
            started_at_ms: now_millis(),
        };
        save_checkpoint(&config.checkpoint_file, &checkpoint).await?;
        Ok(Self {
            config,
            token,
            _lock: lock,
            listener,
            stop: CancellationToken::new(),
            active_connections: Arc::new(Mutex::new(0)),
            bus: None,
            checkpoint,
            recovered_unclean_shutdown,
        })
    }

    /// 准备绑定共享 Core `CommandBus` 的 Sidecar。
    ///
    /// # Errors
    ///
    /// 配置、Token、实例锁或本地端点无法准备时返回错误。
    pub async fn bind_with_bus(
        config: SidecarConfig,
        bus: Arc<dyn CommandBus>,
    ) -> Result<Self, SidecarError> {
        let mut daemon = Self::bind(config).await?;
        daemon.bus = Some(bus);
        Ok(daemon)
    }

    /// 运行控制行协议直到收到 Stop 或外部取消。
    ///
    /// # Errors
    ///
    /// 接收连接或读写控制消息失败时返回错误。
    pub async fn run(self) -> Result<(), SidecarError> {
        let mut connections = JoinSet::new();
        loop {
            tokio::select! {
                () = self.stop.cancelled() => {
                    while let Some(result) = connections.join_next().await {
                        report_connection_result(result);
                    }
                    if let Some(bus) = &self.bus {
                        bus.shutdown().await?;
                    }
                    let mut checkpoint = self.checkpoint.clone();
                    checkpoint.state = LifecycleState::Stopped;
                    save_checkpoint(&self.config.checkpoint_file, &checkpoint).await?;
                    return Ok(())
                },
                accepted = self.listener.accept() => {
                    let stream = accepted?;
                    let stop = self.stop.clone();
                    let token = self.token.clone();
                    let config = self.config.clone();
                    let active_connections = Arc::clone(&self.active_connections);
                    let bus = self.bus.clone();
                    {
                        let mut active = active_connections.lock().await;
                        *active += 1;
                    }
                    connections.spawn(async move {
                        handle_connection(
                            stream,
                            token,
                            config,
                            stop,
                            active_connections,
                            bus,
                        )
                        .await
                    });
                },
                joined = connections.join_next(), if !connections.is_empty() => {
                    if let Some(result) = joined {
                        report_connection_result(result);
                    }
                }
            }
        }
    }

    /// 返回当前 Token，用于同一用户进程发起控制请求。
    #[must_use]
    pub fn token(&self) -> &TokenStore {
        &self.token
    }

    /// 返回本次启动是否从未标记干净停止的检查点恢复。
    #[must_use]
    pub const fn recovered_unclean_shutdown(&self) -> bool {
        self.recovered_unclean_shutdown
    }
}

fn report_connection_result(result: Result<Result<(), SidecarError>, tokio::task::JoinError>) {
    match result {
        Ok(Ok(())) => {}
        Ok(Err(error)) => tracing::warn!(error = %error, "Sidecar 客户端连接处理失败"),
        Err(error) => tracing::warn!(error = %error, "Sidecar 客户端连接任务异常退出"),
    }
}

async fn load_checkpoint(
    path: &std::path::Path,
) -> Result<Option<LifecycleCheckpoint>, SidecarError> {
    let bytes = match tokio::fs::read(path).await {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let checkpoint: LifecycleCheckpoint = serde_json::from_slice(&bytes)?;
    if checkpoint.version != 1 {
        return Err(SidecarError::Checkpoint(format!(
            "unsupported version {}",
            checkpoint.version
        )));
    }
    Ok(Some(checkpoint))
}

async fn save_checkpoint(
    path: &Path,
    checkpoint: &LifecycleCheckpoint,
) -> Result<(), SidecarError> {
    save_checkpoint_with_before_replace(path, checkpoint, |_| Ok(())).await?;
    Ok(())
}

async fn save_checkpoint_with_before_replace<F>(
    path: &Path,
    checkpoint: &LifecycleCheckpoint,
    before_replace: F,
) -> Result<(), io::Error>
where
    F: FnOnce(&Path) -> Result<(), io::Error>,
{
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("生命周期检查点缺少父目录"))?;
    let bytes = serde_json::to_vec(checkpoint)?;
    let temporary = tempfile::NamedTempFile::new_in(parent)?;
    let mut file = tokio::fs::File::from_std(temporary.reopen()?);
    file.write_all(&bytes).await?;
    file.sync_all().await?;
    drop(file);
    before_replace(temporary.path())?;
    temporary.persist(path).map_err(|error| error.error)?;
    sync_directory(parent).await?;
    Ok(())
}

#[cfg(unix)]
async fn sync_directory(path: &Path) -> Result<(), io::Error> {
    tokio::fs::File::open(path).await?.sync_all().await
}

#[cfg(windows)]
async fn sync_directory(path: &Path) -> Result<(), io::Error> {
    use std::os::windows::fs::OpenOptionsExt as _;

    // Windows 需要该标志才能把目录作为句柄打开并刷新目录项。
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)?
        .sync_all()
}

#[cfg(all(not(unix), not(windows)))]
async fn sync_directory(_path: &Path) -> Result<(), io::Error> {
    Ok(())
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

async fn handle_connection(
    stream: SidecarStream,
    token: TokenStore,
    config: SidecarConfig,
    stop: CancellationToken,
    active_connections: Arc<Mutex<usize>>,
    bus: Option<Arc<dyn CommandBus>>,
) -> Result<(), SidecarError> {
    let result = handle_connection_inner(stream, token, config, stop, bus).await;
    let mut active = active_connections.lock().await;
    *active = active.saturating_sub(1);
    result
}

async fn handle_connection_inner(
    stream: SidecarStream,
    token: TokenStore,
    config: SidecarConfig,
    stop: CancellationToken,
    bus: Option<Arc<dyn CommandBus>>,
) -> Result<(), SidecarError> {
    let stream = stream.into_inner();
    let (reader, mut writer) = tokio::io::split(stream);
    let mut lines = tokio::io::BufReader::new(reader).lines();
    let line = tokio::select! {
        () = stop.cancelled() => return Ok(()),
        line = lines.next_line() => line?,
    };
    let Some(line) = line else {
        return Ok(());
    };
    let request: DaemonRequest = serde_json::from_str(&line)?;
    let Some(candidate) = decode_hex(&request.token) else {
        write_response(&mut writer, unauthorized()).await?;
        return Ok(());
    };
    if !token.constant_time_eq(&candidate) {
        write_response(&mut writer, unauthorized()).await?;
        return Ok(());
    }
    let response = match request.command {
        DaemonCommand::Status => DaemonResponse {
            ok: true,
            status: "ready".into(),
            detail: Some(config.endpoint.display().to_string()),
            response: None,
        },
        DaemonCommand::Doctor => DaemonResponse {
            ok: true,
            status: "healthy".into(),
            detail: Some(config.data_dir.display().to_string()),
            response: None,
        },
        DaemonCommand::Stop => {
            stop.cancel();
            DaemonResponse {
                ok: true,
                status: "stopping".into(),
                detail: None,
                response: None,
            }
        }
        DaemonCommand::Execute { request } => match bus {
            Some(bus) => match execute_request(bus.as_ref(), *request).await {
                Ok(response) => DaemonResponse {
                    ok: true,
                    status: "executed".into(),
                    detail: None,
                    response: Some(response),
                },
                Err(error) => DaemonResponse {
                    ok: false,
                    status: "error".into(),
                    detail: Some(error.to_string()),
                    response: None,
                },
            },
            None => DaemonResponse {
                ok: false,
                status: "unsupported".into(),
                detail: Some("Sidecar 未绑定 CommandBus".into()),
                response: None,
            },
        },
        DaemonCommand::ExecuteAs {
            principal_id,
            request,
        } => match bus {
            Some(bus) => match execute_request_as(bus.as_ref(), *request, &principal_id).await {
                Ok(response) => DaemonResponse {
                    ok: true,
                    status: "executed".into(),
                    detail: None,
                    response: Some(response),
                },
                Err(error) => DaemonResponse {
                    ok: false,
                    status: "error".into(),
                    detail: Some(error.to_string()),
                    response: None,
                },
            },
            None => DaemonResponse {
                ok: false,
                status: "unsupported".into(),
                detail: Some("Sidecar 未绑定 CommandBus".into()),
                response: None,
            },
        },
    };
    write_response(&mut writer, response).await
}

async fn write_response<W>(writer: &mut W, response: DaemonResponse) -> Result<(), SidecarError>
where
    W: tokio::io::AsyncWrite + Unpin,
{
    let encoded = serde_json::to_vec(&response)?;
    writer.write_all(&encoded).await?;
    writer.write_all(b"\n").await?;
    writer.shutdown().await?;
    Ok(())
}

fn unauthorized() -> DaemonResponse {
    DaemonResponse {
        ok: false,
        status: "unauthorized".into(),
        detail: None,
        response: None,
    }
}

async fn execute_request(
    bus: &dyn CommandBus,
    request: MemoryRequest,
) -> Result<ResponseEnvelope, CommandError> {
    execute_request_as(bus, request, "sidecar-local").await
}

async fn execute_request_as(
    bus: &dyn CommandBus,
    request: MemoryRequest,
    principal_id: &str,
) -> Result<ResponseEnvelope, CommandError> {
    let operation = match &request.command {
        MemoryCommand::Observe(_) => Operation::Observe,
        MemoryCommand::Commit(_) | MemoryCommand::Correct(_) | MemoryCommand::Forget(_) => {
            Operation::Mutate
        }
        MemoryCommand::Recall(_) | MemoryCommand::Stats(_) => Operation::Recall,
        MemoryCommand::Explain(_) => Operation::Explain,
    };
    let scope: jc_memory_protocol::MemoryScope = request.access_scope.clone().into();
    let principal_id = if principal_id.trim().is_empty() {
        "sidecar-local"
    } else {
        principal_id
    };
    let mut access = AccessContext::new(principal_id, scope.clone());
    access.request_id.clone_from(&request.request_id);
    access.grants.insert(Grant::for_scope(operation, &scope));
    bus.execute(&access, request).await
}

fn decode_hex(value: &str) -> Option<Vec<u8>> {
    if value.len() != 64 {
        return None;
    }
    let mut bytes = Vec::with_capacity(32);
    let mut chars = value.as_bytes().chunks_exact(2);
    for pair in &mut chars {
        let high = hex_value(pair[0])?;
        let low = hex_value(pair[1])?;
        bytes.push((high << 4) | low);
    }
    Some(bytes)
}

fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

/// 将 Token 转为本地控制客户端使用的十六进制字符串。
#[must_use]
pub fn token_hex(token: &TokenStore) -> String {
    use std::fmt::Write as _;
    let mut result = String::with_capacity(64);
    for byte in token.as_bytes() {
        write!(&mut result, "{byte:02x}").expect("写入内存字符串不会失败");
    }
    result
}

/// 向已运行的 Sidecar 发送一个经过 Token 认证的控制命令。
///
/// # Errors
///
/// 端点连接、控制消息写入或响应解析失败时返回错误。
pub async fn send_command(
    config: &SidecarConfig,
    token: &TokenStore,
    command: DaemonCommand,
) -> Result<DaemonResponse, SidecarError> {
    let stream = SidecarStream::connect(&config.endpoint).await?;
    let stream = stream.into_inner();
    let (reader, mut writer) = tokio::io::split(stream);
    let request = serde_json::json!({"token": token_hex(token), "command": command});
    writer
        .write_all(serde_json::to_string(&request)?.as_bytes())
        .await?;
    writer.write_all(b"\n").await?;
    let mut lines = tokio::io::BufReader::new(reader).lines();
    let line = lines.next_line().await?.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "sidecar closed control stream",
        )
    })?;
    Ok(serde_json::from_str(&line)?)
}

#[cfg(test)]
mod tests {
    use std::io;

    use super::{LifecycleCheckpoint, LifecycleState, save_checkpoint_with_before_replace};

    #[tokio::test]
    async fn failed_atomic_checkpoint_replace_preserves_previous_version() {
        let directory = tempfile::tempdir().expect("创建原子检查点测试目录");
        let path = directory.path().join("lifecycle.json");
        let previous = LifecycleCheckpoint {
            version: 1,
            state: LifecycleState::Stopped,
            pid: 1,
            started_at_ms: 1,
        };
        let previous_bytes = serde_json::to_vec(&previous).expect("序列化旧检查点");
        tokio::fs::write(&path, &previous_bytes)
            .await
            .expect("写入旧检查点");
        let replacement = LifecycleCheckpoint {
            version: 1,
            state: LifecycleState::Running,
            pid: 2,
            started_at_ms: 2,
        };

        let error = save_checkpoint_with_before_replace(&path, &replacement, |_| {
            Err(io::Error::other("注入替换失败"))
        })
        .await
        .expect_err("替换前故障必须返回错误");

        assert_eq!(error.kind(), io::ErrorKind::Other);
        assert_eq!(
            tokio::fs::read(&path).await.expect("读取保留的旧检查点"),
            previous_bytes
        );
    }
}
