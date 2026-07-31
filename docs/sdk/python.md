# Python SDK

疆诚记忆 Python SDK 提供 `RemoteMemoryClient`、`AsyncRemoteMemoryClient`、`EmbeddedMemoryClient` 和 `AsyncEmbeddedMemoryClient`。远程与嵌入式模式都使用同一套 JSON Request/Response Envelope，原始 Fragment、Event 和 EvidenceRef 以字典原样传递。

## 开发验证

```bash
python3 -m venv .venv
.venv/bin/python -m pip install -e 'sdk/python[dev]'
.venv/bin/python -m pytest sdk/python/tests -q
.venv/bin/python -m mypy sdk/python/src sdk/python/tests
(cd sdk/python && ../../.venv/bin/maturin develop --features extension-module)
```

`maturin develop` 是当前主机的真实 PyO3 构建；未安装原生模块时，Embedded Fixture 测试会明确跳过，不把跳过结果描述为实机通过。静态 Fixture 的 canonical SHA 用于跨语言远程协议校验；真实 Core Embedded Smoke 会生成运行时 `query_id` 和上下文，因此只比较稳定协议字段，不要求动态值逐字等于 Fixture。

## 远程同步与异步

```python
from jiangcheng_memory import AsyncRemoteMemoryClient, RemoteMemoryClient

sync_client = RemoteMemoryClient(
    base_url="http://127.0.0.1:8080",
    api_key=env_api_key,
)
response = sync_client.recall(request_envelope)
sync_client.close()

async with AsyncRemoteMemoryClient(base_url=server_url) as async_client:
    response = await async_client.recall(request_envelope)
```

示例中的 `env_api_key`、`server_url` 和 `request_envelope` 都应由应用配置提供。SDK 不在代码、Fixture 或日志中保存真实 Token。异步取消保留 Python `asyncio.CancelledError`；HTTP 错误映射为包含 `code`、`request_id`、`details` 的 `JiangchengMemoryError`。

## PyO3 Embedded

`crates/memory-python` 使用 `pyo3-async-runtimes` 把 Rust Tokio Future 转为 Python awaitable。绑定层只解析 JSON、构造精确 Scope Grant 并调用 `EmbeddedCommandBus`，不会复制业务逻辑，也不会把 `Prediction`、`Inference` 或 `Plan` 升级为事实。
