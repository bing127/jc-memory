use std::{io, sync::Arc};

use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncBufReadExt as _, AsyncWriteExt as _},
    sync::Mutex,
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
}

/// Sidecar 控制响应。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonResponse {
    /// 是否成功处理命令。
    pub ok: bool,
    /// 稳定状态文本。
    pub status: String,
    /// 诊断细节。
    pub detail: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DaemonRequest {
    token: String,
    command: DaemonCommand,
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
    /// 控制消息无法解析。
    #[error("invalid control message: {0}")]
    Json(#[from] serde_json::Error),
}

/// 已取得实例锁并绑定本地端点的 Sidecar 守护进程。
pub struct SidecarDaemon {
    config: SidecarConfig,
    token: TokenStore,
    _lock: InstanceLock,
    listener: SidecarListener,
    stop: CancellationToken,
    active_connections: Arc<Mutex<usize>>,
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
        let token = TokenStore::load_or_create(&config.token_file).await?;
        let lock = InstanceLock::acquire(&config.lock_file)?;
        let listener = SidecarListener::bind(&config.endpoint).await?;
        Ok(Self {
            config,
            token,
            _lock: lock,
            listener,
            stop: CancellationToken::new(),
            active_connections: Arc::new(Mutex::new(0)),
        })
    }

    /// 运行控制行协议直到收到 Stop 或外部取消。
    ///
    /// # Errors
    ///
    /// 接收连接或读写控制消息失败时返回错误。
    pub async fn run(self) -> Result<(), SidecarError> {
        loop {
            tokio::select! {
                () = self.stop.cancelled() => return Ok(()),
                accepted = self.listener.accept() => {
                    let stream = accepted?;
                    let stop = self.stop.clone();
                    let token = self.token.clone();
                    let config = self.config.clone();
                    let active_connections = Arc::clone(&self.active_connections);
                    {
                        let mut active = active_connections.lock().await;
                        *active += 1;
                    }
                    handle_connection(stream, token, config, stop.clone(), active_connections).await?;
                }
            }
        }
    }

    /// 返回当前 Token，用于同一用户进程发起控制请求。
    #[must_use]
    pub fn token(&self) -> &TokenStore {
        &self.token
    }
}

async fn handle_connection(
    stream: SidecarStream,
    token: TokenStore,
    config: SidecarConfig,
    stop: CancellationToken,
    active_connections: Arc<Mutex<usize>>,
) -> Result<(), SidecarError> {
    let result = handle_connection_inner(stream, token, config, stop).await;
    let mut active = active_connections.lock().await;
    *active = active.saturating_sub(1);
    result
}

async fn handle_connection_inner(
    stream: SidecarStream,
    token: TokenStore,
    config: SidecarConfig,
    stop: CancellationToken,
) -> Result<(), SidecarError> {
    let stream = stream.into_inner();
    let (reader, mut writer) = tokio::io::split(stream);
    let mut lines = tokio::io::BufReader::new(reader).lines();
    let Some(line) = lines.next_line().await? else {
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
        },
        DaemonCommand::Doctor => DaemonResponse {
            ok: true,
            status: "healthy".into(),
            detail: Some(config.data_dir.display().to_string()),
        },
        DaemonCommand::Stop => {
            stop.cancel();
            DaemonResponse {
                ok: true,
                status: "stopping".into(),
                detail: None,
            }
        }
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
    }
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
