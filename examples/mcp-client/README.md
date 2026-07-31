# MCP Client 示例

该目录只保存不含密钥的 MCP JSON-RPC 示例载荷，目标是本地 `jc-memory-mcp` stdio 服务。

```bash
JC_MEMORY_MCP_SIDECAR_DATA_DIR=/tmp/jc-memory-mcp-data \
  cargo run -p jc-memory-mcp
```

将 `initialize.json` 和 `tools-call-recall.json` 逐行写入服务 stdin，即可观察初始化和结构化 Recall 工具调用。真实部署时通过环境变量提供认证 Token，不要把 Token 写入示例或仓库。
