//! 疆诚记忆 MCP JSON-RPC Server 与结构化工具契约。

use std::sync::Arc;

use jc_memory_core::{CommandBus, CommandError};
use jc_memory_protocol::{AccessContext, MemoryRequest, Operation};
use serde::Deserialize;
use serde_json::{Value, json};

/// 当前 MCP 协议版本。
pub const MCP_PROTOCOL_VERSION: &str = "2025-06-18";

/// MCP JSON-RPC Server 错误。
#[derive(Debug, thiserror::Error)]
pub enum McpServerError {
    /// 请求不是合法 JSON-RPC 对象。
    #[error("invalid JSON-RPC request: {0}")]
    InvalidRequest(String),
    /// 请求参数无法解析。
    #[error("invalid tool arguments: {0}")]
    InvalidArguments(String),
    /// Core `CommandBus` 执行失败。
    #[error(transparent)]
    Command(#[from] CommandError),
    /// JSON 编码失败。
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Deserialize)]
struct RpcRequest {
    jsonrpc: String,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Deserialize)]
struct ToolCallParams {
    name: String,
    #[serde(default)]
    arguments: Value,
}

/// 与 MCP tools/call 对应的服务实现。
pub struct McpServer<B> {
    bus: Arc<B>,
}

impl<B> Clone for McpServer<B> {
    fn clone(&self) -> Self {
        Self {
            bus: Arc::clone(&self.bus),
        }
    }
}

impl<B> McpServer<B>
where
    B: CommandBus + Send + Sync + 'static,
{
    /// 创建绑定共享 `CommandBus` 的 MCP Server。
    #[must_use]
    pub fn new(bus: B) -> Self {
        Self { bus: Arc::new(bus) }
    }

    /// 处理一条 JSON-RPC 请求并返回 JSON-RPC 响应。
    pub async fn handle(&self, input: Value) -> Value {
        let id = input.get("id").cloned().unwrap_or(Value::Null);
        match self.handle_inner(input).await {
            Ok(result) => json!({"jsonrpc":"2.0", "id": id, "result": result}),
            Err(error) => json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": error_response(&error),
            }),
        }
    }

    async fn handle_inner(&self, input: Value) -> Result<Value, McpServerError> {
        let request: RpcRequest = serde_json::from_value(input)
            .map_err(|error| McpServerError::InvalidRequest(error.to_string()))?;
        if request.jsonrpc != "2.0" {
            return Err(McpServerError::InvalidRequest("jsonrpc must be 2.0".into()));
        }
        match request.method.as_str() {
            "initialize" => Ok(json!({
                "protocolVersion": MCP_PROTOCOL_VERSION,
                "capabilities": {"tools": {"listChanged": false}},
                "serverInfo": {"name": "jiangcheng-memory", "version": "0.1.0"}
            })),
            "notifications/initialized" => Ok(Value::Null),
            "tools/list" => Ok(json!({"tools": tool_definitions()})),
            "tools/call" => {
                let params: ToolCallParams = serde_json::from_value(request.params)
                    .map_err(|error| McpServerError::InvalidArguments(error.to_string()))?;
                self.call_tool(&params.name, params.arguments).await
            }
            _ => Err(McpServerError::InvalidRequest(format!(
                "unknown method: {}",
                request.method
            ))),
        }
    }

    async fn call_tool(&self, name: &str, arguments: Value) -> Result<Value, McpServerError> {
        if name == "memory_commit" {
            return Ok(tool_error("unsupported", "当前协议尚未定义 Commit 命令"));
        }
        let request_value = arguments
            .get("request")
            .cloned()
            .unwrap_or(arguments.clone());
        let request: MemoryRequest = serde_json::from_value(request_value)
            .map_err(|error| McpServerError::InvalidArguments(error.to_string()))?;
        let access = access_context(&request, name)?;
        let response = self.bus.execute(&access, request).await?;
        let structured = serde_json::to_value(&response)?;
        Ok(json!({
            "content": [{"type": "text", "text": response.canonical_json()?}],
            "structuredContent": structured,
            "isError": false,
        }))
    }
}

fn access_context(request: &MemoryRequest, tool: &str) -> Result<AccessContext, McpServerError> {
    let operation = match tool {
        "memory_observe" => Operation::Observe,
        "memory_recall" | "memory_stats" => Operation::Recall,
        "memory_correct" | "memory_forget" => Operation::Mutate,
        "memory_explain" => Operation::Explain,
        _ => {
            return Err(McpServerError::InvalidArguments(format!(
                "unknown memory tool: {tool}"
            )));
        }
    };
    let mut context = AccessContext::new("mcp-client", request.access_scope.clone().into());
    context.request_id.clone_from(&request.request_id);
    context.grants.insert(jc_memory_protocol::Grant::for_scope(
        operation,
        &context.scope,
    ));
    Ok(context)
}

fn error_response(error: &McpServerError) -> Value {
    match error {
        McpServerError::Command(command) => json!({
            "code": -32000,
            "message": "command failed",
            "data": {"code": format!("{:?}", command.code()), "detail": command.to_string()},
        }),
        McpServerError::InvalidRequest(detail) | McpServerError::InvalidArguments(detail) => {
            json!({"code": -32602, "message": "invalid params", "data": {"detail": detail}})
        }
        McpServerError::Json(detail) => {
            json!({"code": -32603, "message": "internal error", "data": {"detail": detail.to_string()}})
        }
    }
}

fn tool_error(code: &str, detail: &str) -> Value {
    json!({
        "content": [{"type":"text", "text": detail}],
        "structuredContent": {"code": code, "detail": detail},
        "isError": true,
    })
}

fn tool_definitions() -> Vec<Value> {
    [
        "memory_observe",
        "memory_recall",
        "memory_commit",
        "memory_correct",
        "memory_explain",
        "memory_forget",
        "memory_stats",
    ]
    .into_iter()
    .map(|name| {
        json!({
            "name": name,
            "description": "调用疆诚记忆共享 CommandBus 并返回结构化协议结果",
            "inputSchema": {
                "type": "object",
                "properties": {"request": {"type":"object"}},
                "required": ["request"],
                "additionalProperties": false,
            }
        })
    })
    .collect()
}

/// 以 stdio 行协议运行 MCP Server。
///
/// # Errors
///
/// 标准输入读取或标准输出写入失败时返回错误。
pub async fn serve_stdio<B>(server: McpServer<B>) -> Result<(), std::io::Error>
where
    B: CommandBus + Send + Sync + 'static,
{
    use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _};
    let stdin = tokio::io::stdin();
    let mut lines = tokio::io::BufReader::new(stdin).lines();
    let mut stdout = tokio::io::stdout();
    while let Some(line) = lines.next_line().await? {
        let input = serde_json::from_str(&line).unwrap_or_else(|error| {
            json!({"jsonrpc":"2.0", "id": null, "method":"invalid", "params": {"error": error.to_string()}})
        });
        let response = server.handle(input).await;
        let encoded = serde_json::to_vec(&response)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
        stdout.write_all(&encoded).await?;
        stdout.write_all(b"\n").await?;
        stdout.flush().await?;
    }
    Ok(())
}
