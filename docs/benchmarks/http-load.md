# HTTP 压测与性能报告

`http-load.mjs` 为本地 Web/BFF 或 Rust Server 提供统一的并发 HTTP 压测，输出版本化 `jcmem.load-test.v1` JSON。报告可直接导入 Web 控制台的“性能 -> 压测报告”页面。

## 运行方式

准备一个不包含凭据的配置文件：

```json
{
  "base_url": "http://127.0.0.1:8987",
  "warmup_requests": 20,
  "timeout_ms": 30000,
  "real_rust_sqlite": true,
  "scenarios": [
    {
      "name": "healthz-c50",
      "method": "GET",
      "path": "/healthz",
      "requests": 1000,
      "concurrency": 50
    },
    {
      "name": "recall-c32",
      "method": "POST",
      "path": "/v1/memory/recall",
      "body_file": "recall.json",
      "requests": 800,
      "concurrency": 32
    }
  ]
}
```

API Key 只从环境变量读取，不写入配置文件或报告：

```bash
export JC_MEMORY_API_KEY='sk-jcmemory-...'
node scripts/benchmark/http-load.mjs \
  --config /tmp/jcmem-load/config.json \
  --output target/benchmarks/load-test.json
```

默认只允许 loopback 地址。对外部地址压测必须使用 HTTPS 并显式添加 `--allow-external`。脚本不把请求体、查询文本、API Key、Scope 或请求 ID写入报告；请求体中的 `request_id` 会为每次请求追加序号。

## 报告字段

每个场景记录请求总数、并发数、成功/失败数、状态码、传输错误、吞吐、P50/P95/P99/最大延迟和是否为真实 Rust/SQLite 链路。百分位使用 nearest-rank，样本不足时保留实际可计算字段，空样本显示 `null`。

报告必须保持 `schema_version: jcmem.load-test.v1`。压测脚本只生成采集数据，不能把生成器、Fixture 或静态推测描述成真实数据库规模验证。

## 对比规则

优化前后必须固定数据库规模、配置、预热、请求体、请求数量和并发等级。至少重复 Recall 并发 8、32、64；同时记录最终 Fragment/Event 计数，确认压力测试没有丢写或多写。
