use std::{io, path::Path};

/// 本地传输错误。
#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    /// 监听或连接失败。
    #[error("local transport error: {0}")]
    Io(#[from] io::Error),
}

#[cfg(unix)]
mod platform {
    use super::{Path, TransportError};
    use std::os::unix::fs::PermissionsExt as _;
    use tokio::net::{UnixListener, UnixStream};

    pub type Listener = UnixListener;
    pub type Stream = UnixStream;

    pub async fn bind(path: &Path) -> Result<Listener, TransportError> {
        if path.exists() {
            tokio::fs::remove_file(path).await?;
        }
        let listener = UnixListener::bind(path)?;
        tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).await?;
        Ok(listener)
    }
}

#[cfg(not(unix))]
mod platform {
    use super::*;
    use tokio::net::{TcpListener, TcpStream};

    pub type Listener = TcpListener;
    pub type Stream = TcpStream;

    pub async fn bind(_path: &Path) -> Result<Listener, TransportError> {
        Ok(TcpListener::bind("127.0.0.1:0").await?)
    }
}

/// 平台本地监听器。
pub struct SidecarListener {
    inner: platform::Listener,
}

impl SidecarListener {
    /// 绑定配置端点。
    ///
    /// # Errors
    ///
    /// 端点已损坏或无法绑定时返回错误。
    pub async fn bind(path: &Path) -> Result<Self, TransportError> {
        Ok(Self {
            inner: platform::bind(path).await?,
        })
    }

    /// 接受一个本地连接。
    ///
    /// # Errors
    ///
    /// 操作系统接受连接失败时返回错误。
    pub async fn accept(&self) -> Result<SidecarStream, TransportError> {
        let stream = self.inner.accept().await?;
        #[cfg(unix)]
        let stream = stream.0;
        #[cfg(not(unix))]
        let stream = stream.0;
        Ok(SidecarStream { inner: stream })
    }
}

/// 平台本地双向流。
pub struct SidecarStream {
    pub(crate) inner: platform::Stream,
}

impl SidecarStream {
    /// 连接配置端点。
    ///
    /// # Errors
    ///
    /// 端点不存在或连接失败时返回错误。
    pub async fn connect(path: &Path) -> Result<Self, TransportError> {
        #[cfg(unix)]
        let inner = tokio::net::UnixStream::connect(path).await?;
        #[cfg(not(unix))]
        let inner = tokio::net::TcpStream::connect(path.to_string_lossy().as_ref()).await?;
        Ok(Self { inner })
    }

    /// 拆出底层 Tokio 流以便读写行协议。
    pub fn into_inner(self) -> platform::Stream {
        self.inner
    }
}
