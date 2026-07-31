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
    /// MCP 调用方 Token 缺失或无效。
    #[error("mcp authentication failed")]
    Unauthorized,
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
    #[serde(rename = "authToken", default)]
    auth_token: Option<String>,
}

/// MCP stdio 调用方的可选 Token 校验器。
#[derive(Clone)]
struct McpAuth {
    expected: Arc<[u8]>,
}

impl McpAuth {
    fn new(token: &str) -> Result<Self, McpServerError> {
        if token.is_empty() {
            return Err(McpServerError::InvalidArguments(
                "MCP 身份 Token 不能为空".into(),
            ));
        }
        Ok(Self {
            expected: Arc::from(token.as_bytes()),
        })
    }

    fn verify(&self, candidate: Option<&str>) -> bool {
        let candidate = candidate.map_or(&[][..], str::as_bytes);
        let mut difference = (self.expected.len() as u64) ^ (candidate.len() as u64);
        let length = self.expected.len().max(candidate.len());
        for index in 0..length {
            let expected = self.expected.get(index).copied().unwrap_or_default();
            let actual = candidate.get(index).copied().unwrap_or_default();
            difference |= u64::from(expected ^ actual);
        }
        difference == 0
    }
}

/// 与 MCP tools/call 对应的服务实现。
pub struct McpServer<B> {
    bus: Arc<B>,
    auth: Option<McpAuth>,
}

impl<B> Clone for McpServer<B> {
    fn clone(&self) -> Self {
        Self {
            bus: Arc::clone(&self.bus),
            auth: self.auth.clone(),
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
        Self {
            bus: Arc::new(bus),
            auth: None,
        }
    }

    /// 创建带调用方 Token 校验的 MCP Server。
    ///
    /// # Errors
    ///
    /// Token 为空时返回参数错误。
    pub fn with_auth_token(bus: B, token: impl AsRef<str>) -> Result<Self, McpServerError> {
        Ok(Self {
            bus: Arc::new(bus),
            auth: Some(McpAuth::new(token.as_ref())?),
        })
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
                self.call_tool(&params.name, params.arguments, params.auth_token.as_deref())
                    .await
            }
            _ => Err(McpServerError::InvalidRequest(format!(
                "unknown method: {}",
                request.method
            ))),
        }
    }

    async fn call_tool(
        &self,
        name: &str,
        arguments: Value,
        auth_token: Option<&str>,
    ) -> Result<Value, McpServerError> {
        if let Some(auth) = &self.auth
            && !auth.verify(auth_token)
        {
            return Err(McpServerError::Unauthorized);
        }
        let request_value = arguments
            .get("request")
            .cloned()
            .unwrap_or(arguments.clone());
        let request: MemoryRequest = serde_json::from_value(request_value)
            .map_err(|error| McpServerError::InvalidArguments(error.to_string()))?;
        let expected_command = name.strip_prefix("memory_").ok_or_else(|| {
            McpServerError::InvalidArguments(format!("unknown memory tool: {name}"))
        })?;
        if command_name(&request.command) != expected_command {
            return Err(McpServerError::InvalidArguments(format!(
                "tool {name} cannot execute command {}",
                command_name(&request.command)
            )));
        }
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
        "memory_commit" | "memory_correct" | "memory_forget" => Operation::Mutate,
        "memory_recall" | "memory_stats" => Operation::Recall,
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

fn command_name(command: &jc_memory_protocol::MemoryCommand) -> &'static str {
    match command {
        jc_memory_protocol::MemoryCommand::Observe(_) => "observe",
        jc_memory_protocol::MemoryCommand::Commit(_) => "commit",
        jc_memory_protocol::MemoryCommand::Recall(_) => "recall",
        jc_memory_protocol::MemoryCommand::Stats(_) => "stats",
        jc_memory_protocol::MemoryCommand::Correct(_) => "correct",
        jc_memory_protocol::MemoryCommand::Forget(_) => "forget",
        jc_memory_protocol::MemoryCommand::Explain(_) => "explain",
    }
}

fn error_response(error: &McpServerError) -> Value {
    match error {
        McpServerError::Command(command) => json!({
            "code": -32000,
            "message": "command failed",
            "data": {"code": format!("{:?}", command.code()), "detail": command.to_string()},
        }),
        McpServerError::Unauthorized => json!({
            "code": -32001,
            "message": "mcp authentication failed",
            "data": {"code": "unauthorized"},
        }),
        McpServerError::InvalidRequest(detail) | McpServerError::InvalidArguments(detail) => {
            json!({"code": -32602, "message": "invalid params", "data": {"detail": detail}})
        }
        McpServerError::Json(detail) => {
            json!({"code": -32603, "message": "internal error", "data": {"detail": detail.to_string()}})
        }
    }
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
        let command = name
            .strip_prefix("memory_")
            .expect("稳定工具名带 memory_ 前缀");
        json!({
            "name": name,
            "description": "调用疆诚记忆共享 CommandBus 并返回结构化协议结果",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "request": {
                        "type": "object",
                        "properties": {
                            "api_version": {"type":"string", "const":"v1"},
                            "request_id": {"type":"string", "minLength":1},
                            "access_scope": {
                                "type":"object",
                                "properties": {
                                    "namespace_id": {"type":"string"},
                                    "tenant_id": {"type":["string","null"]},
                                    "user_id": {"type":["string","null"]},
                                    "agent_id": {"type":["string","null"]},
                                    "project_id": {"type":["string","null"]}
                                },
                                "required":["namespace_id"],
                                "additionalProperties":false
                            },
                            "command": {
                                "type":"object",
                                "properties": {
                                    "command": {"type":"string", "const":command},
                                    "payload": {}
                                },
                                "required":["command","payload"],
                                "additionalProperties":false
                            }
                        },
                        "required":["api_version","request_id","access_scope","command"],
                        "additionalProperties":false
                    }
                },
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
