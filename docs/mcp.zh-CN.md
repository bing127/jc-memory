# MCP Server

`jc-memory-mcp` 提供基于 stdio 行协议的 JSON-RPC MCP Server。请求先转换为共享 `MemoryRequest`，再通过 `CommandBus` 执行；响应同时包含 MCP `content` 和结构化 `structuredContent`。

## 已实现

- `initialize`，协议版本固定为 `2025-06-18`。
- `tools/list`，公开七个稳定工具名：`memory_observe`、`memory_recall`、`memory_commit`、`memory_correct`、`memory_explain`、`memory_forget`、`memory_stats`。
- Observe、Recall、Correct、Forget、Explain、Stats 通过共享 Core ACL 和 `AccessContext` 执行。
- 工具调用失败返回稳定 JSON-RPC 错误分类，并在工具结果中保留结构化错误对象。

## 尚未实现

当前领域协议没有 Commit 命令，因此 `memory_commit` 只返回 `structuredContent.code=unsupported`，不会把 Commit 错误地映射为 Observe。Sidecar 转发、远程 MCP 身份认证和完整 Embedded/REST/gRPC/MCP 跨协议 E2E 仍待后续阶段完成。

## 本地运行

```bash
JC_MEMORY_DATA_DIR=/var/lib/jc-memory \
  cargo run --locked -p jc-memory-mcp
```

生产环境应由上层进程管理 stdin/stdout 生命周期，并通过外部边界提供身份和数据目录权限；MCP Server 本身不把 API Key 或秘密写入协议消息。
