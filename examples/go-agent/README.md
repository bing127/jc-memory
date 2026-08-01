# Go Agent 示例

在 `sdk/go` module 中运行远程 Fixture 示例：

```bash
export JC_MEMORY_URL=http://127.0.0.1:8080
export JC_MEMORY_API_KEY="sk-jcmemory-$(openssl rand -hex 17)"
cd sdk/go && go run ../../examples/go-agent
```

示例不内置 API Key，也不提交本地数据库或 C ABI 动态库。
