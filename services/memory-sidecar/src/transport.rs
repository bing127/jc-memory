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

    pub async fn connect(path: &Path) -> Result<Stream, TransportError> {
        Ok(UnixStream::connect(path).await?)
    }

    pub async fn accept(listener: &Listener) -> Result<Stream, TransportError> {
        Ok(listener.accept().await?.0)
    }
}

#[cfg(windows)]
mod platform {
    use super::{Path, TransportError};
    use std::{
        io,
        pin::Pin,
        task::{Context, Poll},
    };
    use tokio::{
        io::{AsyncRead, AsyncWrite, ReadBuf},
        net::windows::named_pipe::{
            ClientOptions, NamedPipeClient, NamedPipeServer, ServerOptions,
        },
        sync::Mutex,
    };

    const PIPE_PREFIX: &str = r"\\.\pipe\jc-memory-";

    /// 将配置端点稳定映射为固定长度的合法管道名称。
    pub(crate) fn pipe_name(path: &Path) -> String {
        // FNV-1a 只用于生成名称，不承担认证职责；认证仍由控制协议完成。
        let mut hash = 0xcbf2_9ce4_8422_2325_u64;
        for byte in path.to_string_lossy().as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        format!("{PIPE_PREFIX}{hash:016x}")
    }

    /// Windows Named Pipe 监听器，预先保留一个未连接实例以接受下一个客户端。
    pub struct Listener {
        name: String,
        server: Mutex<Option<NamedPipeServer>>,
    }

    /// Named Pipe 服务端和客户端流的统一抽象。
    pub enum Stream {
        Server(Pin<Box<NamedPipeServer>>),
        Client(Pin<Box<NamedPipeClient>>),
    }

    impl AsyncRead for Stream {
        fn poll_read(
            self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            read_buf: &mut ReadBuf<'_>,
        ) -> Poll<io::Result<()>> {
            match self.get_mut() {
                Self::Server(stream) => stream.as_mut().poll_read(cx, read_buf),
                Self::Client(stream) => stream.as_mut().poll_read(cx, read_buf),
            }
        }
    }

    impl AsyncWrite for Stream {
        fn poll_write(
            self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            bytes: &[u8],
        ) -> Poll<io::Result<usize>> {
            match self.get_mut() {
                Self::Server(stream) => stream.as_mut().poll_write(cx, bytes),
                Self::Client(stream) => stream.as_mut().poll_write(cx, bytes),
            }
        }

        fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            match self.get_mut() {
                Self::Server(stream) => stream.as_mut().poll_flush(cx),
                Self::Client(stream) => stream.as_mut().poll_flush(cx),
            }
        }

        fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            match self.get_mut() {
                Self::Server(stream) => stream.as_mut().poll_shutdown(cx),
                Self::Client(stream) => stream.as_mut().poll_shutdown(cx),
            }
        }
    }

    #[expect(
        clippy::unused_async,
        reason = "The cross-platform listener binding contract is asynchronous"
    )]
    pub async fn bind(path: &Path) -> Result<Listener, TransportError> {
        let name = pipe_name(path);
        let server = ServerOptions::new()
            .first_pipe_instance(true)
            .create(&name)?;
        Ok(Listener {
            name,
            server: Mutex::new(Some(server)),
        })
    }

    pub async fn accept(listener: &Listener) -> Result<Stream, TransportError> {
        let mut current = listener.server.lock().await;
        let server = current.take().ok_or_else(|| {
            TransportError::Io(io::Error::other(
                "named pipe listener instance is unavailable",
            ))
        })?;
        if let Err(error) = server.connect().await {
            *current = Some(server);
            return Err(error.into());
        }
        let next = match ServerOptions::new().create(&listener.name) {
            Ok(next) => next,
            Err(error) => {
                *current = None;
                return Err(error.into());
            }
        };
        *current = Some(next);
        Ok(Stream::Server(Box::pin(server)))
    }

    #[expect(
        clippy::unused_async,
        reason = "The cross-platform stream connection contract is asynchronous"
    )]
    pub async fn connect(path: &Path) -> Result<Stream, TransportError> {
        Ok(Stream::Client(Box::pin(
            ClientOptions::new().open(pipe_name(path))?,
        )))
    }
}

#[cfg(all(not(unix), not(windows)))]
mod platform {
    use super::*;
    use tokio::net::{TcpListener, TcpStream};

    pub type Listener = TcpListener;
    pub type Stream = TcpStream;

    pub async fn bind(path: &Path) -> Result<Listener, TransportError> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?.to_string();
        tokio::fs::write(path, address).await?;
        Ok(listener)
    }

    pub async fn connect(path: &Path) -> Result<Stream, TransportError> {
        let address = tokio::fs::read_to_string(path).await?;
        Ok(TcpStream::connect(address.trim()).await?)
    }

    pub async fn accept(listener: &Listener) -> Result<Stream, TransportError> {
        Ok(listener.accept().await?.0)
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
        let stream = platform::accept(&self.inner).await?;
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
        let inner = platform::connect(path).await?;
        Ok(Self { inner })
    }

    /// 拆出底层 Tokio 流以便读写行协议。
    #[must_use]
    pub fn into_inner(self) -> platform::Stream {
        self.inner
    }
}

#[cfg(test)]
mod tests {
    use super::{SidecarListener, SidecarStream};

    #[cfg(unix)]
    #[tokio::test]
    async fn unix_endpoint_accepts_a_local_connection() {
        let directory = tempfile::tempdir().expect("创建传输测试目录");
        let endpoint = directory.path().join("jcmemd.sock");
        let listener = SidecarListener::bind(&endpoint).await.expect("绑定 UDS");
        let connecting = tokio::spawn({
            let endpoint = endpoint.clone();
            async move { SidecarStream::connect(&endpoint).await }
        });
        let accepted = listener.accept().await.expect("接受 UDS 连接");
        connecting
            .await
            .expect("等待 UDS 客户端")
            .expect("连接 UDS");
        drop(accepted);
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn windows_endpoint_uses_named_pipe_without_endpoint_file() {
        let directory = tempfile::tempdir().expect("创建传输测试目录");
        let endpoint = directory.path().join("jcmemd.pipe");
        let listener = SidecarListener::bind(&endpoint)
            .await
            .expect("绑定 Named Pipe");
        assert!(!endpoint.exists());
        let connecting = tokio::spawn({
            let endpoint = endpoint.clone();
            async move { SidecarStream::connect(&endpoint).await }
        });
        let accepted = listener.accept().await.expect("接受 Named Pipe 连接");
        connecting
            .await
            .expect("等待 Named Pipe 客户端")
            .expect("连接 Named Pipe");
        drop(accepted);
    }

    #[cfg(windows)]
    #[test]
    fn windows_endpoint_mapping_is_stable_and_bounded() {
        let endpoint = std::path::Path::new(r"C:\Users\example\AppData\Local\JiaCheng\jcmemd.pipe");
        let same = super::platform::pipe_name(endpoint);
        let other = super::platform::pipe_name(std::path::Path::new(
            r"C:\Users\example\AppData\Local\JiaCheng\other.pipe",
        ));
        assert_eq!(same, super::platform::pipe_name(endpoint));
        assert_ne!(same, other);
        assert!(same.starts_with(r"\\.\pipe\jc-memory-"));
        assert!(same.len() <= 256);
        assert!(
            same.rsplit_once('\\')
                .is_some_and(|(_, suffix)| suffix.chars().all(|value| value.is_ascii_hexdigit()))
        );
    }

    #[cfg(all(not(unix), not(windows)))]
    #[tokio::test]
    async fn loopback_endpoint_persists_the_random_address_for_clients() {
        let directory = tempfile::tempdir().expect("创建传输测试目录");
        let endpoint = directory.path().join("jcmemd.endpoint");
        let listener = SidecarListener::bind(&endpoint)
            .await
            .expect("绑定 Loopback");
        let address = tokio::fs::read_to_string(&endpoint)
            .await
            .expect("读取 Loopback 地址");
        assert!(address.starts_with("127.0.0.1:"));
        let connecting = tokio::spawn({
            let endpoint = endpoint.clone();
            async move { SidecarStream::connect(&endpoint).await }
        });
        let accepted = listener.accept().await.expect("接受 Loopback 连接");
        connecting
            .await
            .expect("等待 Loopback 客户端")
            .expect("连接 Loopback");
        drop(accepted);
    }
}
