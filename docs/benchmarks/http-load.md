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

压测受保护的 Web/BFF 接口时，会话 Cookie 和允许的 Origin 也只从环境变量读取：

```bash
export JC_WEB_SESSION_COOKIE='jc_memory_session=<当前会话值>'
export JC_WEB_ORIGIN='http://127.0.0.1:5173'
node scripts/benchmark/http-load.mjs \
  --config target/benchmarks/recall-after-config.json \
  --output target/benchmarks/recall-after.json
```

`JC_MEMORY_API_KEY` 用于直接压测 Rust Server；`JC_WEB_SESSION_COOKIE` 与 `JC_WEB_ORIGIN` 用于压测 BFF。三者均不会写入 `jcmem.load-test.v1` 报告。

默认只允许 loopback 地址。对外部地址压测必须使用 HTTPS 并显式添加 `--allow-external`。脚本不把请求体、查询文本、API Key、Scope 或请求 ID写入报告；请求体中的 `request_id` 会为每次请求追加序号。

## 报告字段

每个场景记录请求总数、并发数、成功/失败数、状态码、传输错误、吞吐、P50/P95/P99/最大延迟和是否为真实 Rust/SQLite 链路。百分位使用 nearest-rank，样本不足时保留实际可计算字段，空样本显示 `null`。

报告必须保持 `schema_version: jcmem.load-test.v1`。压测脚本只生成采集数据，不能把生成器、Fixture 或静态推测描述成真实数据库规模验证。

## 对比规则

优化前后必须固定数据库规模、配置、预热、请求体、请求数量和并发等级。至少重复 Recall 并发 8、32、64；同时记录最终 Fragment/Event 计数，确认压力测试没有丢写或多写。

## 2026-08-01 Recall 复测

本次正式报告位于 gitignored 的 `target/benchmarks/recall-after.json`，报告 provenance 为提交 `384ee96`、`dirty=false`、`real_rust_sqlite=true`。链路为本机受保护 BFF `127.0.0.1:8990` 到 Rust Server `127.0.0.1:8987` 再到 SQLite。

固定条件如下：

- `max_in_flight=64`、`max_requests_per_second=100000`、`request_timeout_ms=30000`；
- 预热为每个场景 20 次；正式请求为 c8 400 次、c32 800 次、c64 800 次；
- 数据库包含 2032 个 Fragment 和 2032 个 Event，压测前后计数一致；
- Fragment 正文按 `deployment migration benchmark fragment <n> records the PostgreSQL migration pool and recall evidence` 生成；
- Recall 使用 `semantic`，查询为 `deployment migration benchmark fragment 17`，Limits 为 `4/8/2/32/30000ms`。

正式结果：

| 场景 | 成功/请求 | 状态码 | 吞吐 | P50 | P95 | P99 | 最大延迟 |
| --- | ---: | --- | ---: | ---: | ---: | ---: | ---: |
| Recall c8 | 400/400 | 200: 400 | 125.94/s | 61.58ms | 85.59ms | 113.54ms | 122.20ms |
| Recall c32 | 800/800 | 200: 800 | 123.40/s | 252.58ms | 305.02ms | 313.25ms | 318.51ms |
| Recall c64 | 800/800 | 200: 800 | 118.52/s | 523.56ms | 599.77ms | 604.42ms | 608.85ms |

三个场景均没有 429、timeout 或传输错误。压测期间 Fragment/Event 数量保持 2032/2032；Recall 只新增会话和激活记录，不覆盖权威 Fragment、Event 或 EvidenceRef。

历史报告记录的基线为 c8 `13.06/s, 608.22/777.67/863.10ms`，c32 `13.48/s, 2320.82/2738.33/3169.88ms`，c64 `13.00/s, 4863.74/5605.15/6439.92ms`。按已知相同的数据库规模、服务限额、请求数和并发数计算，当前吞吐参考增幅为 864.30%/815.44%/811.73%，P95 参考降幅为 88.99%/88.86%/89.30%。旧请求体没有保存在仓库或版本化报告中，因此这些百分比只能作为同已知条件参考，不能描述为严格同请求体 A/B 结论。

实测定位到旧 `recent_fragments` 查询按 `fragment_id` 联接 FTS5 虚表，在 2032 行规模下单次曾出现约 1.6–1.85 秒慢查询。提交 `b5b14b3` 改为通过 Scope/时间索引读取权威 Fragment、Content 和 Event；全文匹配仍由 FTS 承担。回归测试同时证明派生 FTS 行缺失时，最近候选仍来自权威内容。
