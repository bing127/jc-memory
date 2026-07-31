# MCP Server

`jc-memory-mcp` 提供基于 stdio 行协议的 JSON-RPC MCP Server。请求先转换为共享 `MemoryRequest`，再通过 `CommandBus` 执行；响应同时包含 MCP `content` 和结构化 `structuredContent`。

## 已实现

- `initialize`，协议版本固定为 `2025-06-18`。
- `tools/list`，公开七个稳定工具名：`memory_observe`、`memory_recall`、`memory_commit`、`memory_correct`、`memory_explain`、`memory_forget`、`memory_stats`。
- Observe、Recall、Commit、Correct、Forget、Explain、Stats 通过共享 Core ACL 和 `AccessContext` 执行。
- Commit 接收同一访问 Scope 内的完整 Fragment 批次，按输入顺序返回每条权威追加结果；Fragment Kind、Validity 和证据字段保持原样，不会把 Prediction 或 ToolResult 提升为 Fact。
- 工具调用失败返回稳定 JSON-RPC 错误分类，并在工具结果中保留结构化错误对象。
- 设置 `JC_MEMORY_MCP_TOKEN` 后，`tools/call` 必须在参数中携带 `authToken`；Token 使用固定长度循环比较，错误返回 `-32001`，不会把 Token 写入响应。
- `tools/list` 为每个工具声明 `v1`、`request_id`、`access_scope` 和对应 `command` 的必填结构；服务端会拒绝工具名与请求命令不一致的调用。

## 尚未实现

当前 Commit 已接入领域协议和共享 `CommandBus`，权威 Fragment 批次在 SQLite 单事务中追加；自动 Quarantine 治理事件仍沿用既有治理路径。Sidecar 转发和完整 Embedded/REST/gRPC/MCP 跨协议 E2E 仍待后续阶段完成。未配置 `JC_MEMORY_MCP_TOKEN` 时仅适合由外层已认证进程托管的本地 stdio 模式；远程部署必须配置该变量。

## 本地运行

```bash
JC_MEMORY_DATA_DIR=/var/lib/jc-memory \
JC_MEMORY_MCP_TOKEN='替换为部署平台注入的随机 Token' \
  cargo run --locked -p jc-memory-mcp
```

配置 Token 后，MCP 客户端的 `tools/call` 参数示例为 `{"name":"memory_stats","authToken":"<同一 Token>","arguments":{"request":...}}`。生产环境应由上层进程管理 stdin/stdout 生命周期，并通过 Secret 注入 Token；MCP Server 不把 API Key 或秘密写入日志和结构化结果。
