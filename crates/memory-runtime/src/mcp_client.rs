//! MCP JSON-RPC Client 契约。

use std::{future::Future, time::Duration};

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

/// MCP 传输抽象，便于使用版本化本地 Fixture。
#[async_trait]
pub trait McpTransport: Send + Sync {
    /// 发送一次 JSON-RPC 请求。
    async fn request(&self, method: &str, params: Value) -> Result<Value, McpError>;
}

/// MCP 调用错误。
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum McpError {
    /// 请求超过超时。
    #[error("MCP request timed out")]
    Timeout,
    /// 请求被取消。
    #[error("MCP request cancelled")]
    Cancelled,
    /// MCP 协议返回错误。
    #[error("MCP protocol error: {0}")]
    Protocol(String),
    /// 传输层失败。
    #[error("MCP transport error: {0}")]
    Transport(String),
}

/// 带超时和取消控制的 MCP Client。
pub struct McpClient<T> {
    transport: T,
    timeout: Duration,
}

impl<T: McpTransport> McpClient<T> {
    /// 创建 MCP Client。
    #[must_use]
    pub const fn new(transport: T, timeout: Duration) -> Self {
        Self { transport, timeout }
    }

    /// 执行 initialize 握手。
    ///
    /// # Errors
    ///
    /// 传输失败、超时或调用取消时返回错误。
    pub async fn initialize(&self) -> Result<Value, McpError> {
        self.request(
            "initialize",
            json!({"protocolVersion":"2025-06-18"}),
            CancellationToken::new(),
        )
        .await
    }

    /// 获取工具列表。
    ///
    /// # Errors
    ///
    /// 传输失败、超时或调用取消时返回错误。
    pub async fn tools_list(&self, cancellation: CancellationToken) -> Result<Value, McpError> {
        self.request("tools/list", json!({}), cancellation).await
    }

    /// 调用一个 MCP 工具。
    ///
    /// # Errors
    ///
    /// 传输失败、超时或调用取消时返回错误。
    pub async fn tools_call(
        &self,
        name: &str,
        arguments: Value,
        cancellation: CancellationToken,
    ) -> Result<Value, McpError> {
        self.request(
            "tools/call",
            json!({"name": name, "arguments": arguments}),
            cancellation,
        )
        .await
    }

    async fn request(
        &self,
        method: &str,
        params: Value,
        cancellation: CancellationToken,
    ) -> Result<Value, McpError> {
        controlled(
            self.transport.request(method, params),
            cancellation,
            self.timeout,
        )
        .await
    }
}

async fn controlled<F>(
    future: F,
    cancellation: CancellationToken,
    timeout: Duration,
) -> Result<Value, McpError>
where
    F: Future<Output = Result<Value, McpError>>,
{
    tokio::select! {
        biased;
        () = cancellation.cancelled() => Err(McpError::Cancelled),
        result = tokio::time::timeout(timeout, future) => result.map_err(|_| McpError::Timeout)?,
    }
}
