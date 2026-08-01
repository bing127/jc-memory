# Recall Performance And Observability Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 消除 Recall 候选权威记录的串行 N+1 读取，让查询预算覆盖完整链路，并交付可复现压测 JSON、静态报告图表和实时性能监控。

**Architecture:** Storage 以精确 Scope 批量读取 Fragment 和 Evidence，Core 用一个 Deadline 包裹完整 Recall。Server 输出低基数 Prometheus 指标，BFF 将其归一化为稳定 JSON；前端性能页本地解析 `jcmem.load-test.v1` 报告并轮询实时快照。

**Tech Stack:** Rust 2024、Tokio、SQLx/SQLite、Axum、Node.js 22、Fastify、React 19、TypeScript、Zod、Recharts、Vitest、Playwright。

---

## 文件结构

- `crates/memory-storage/src/repository.rs`：Scope 批量 Fragment/Evidence 查询与权威投影校验。
- `crates/memory-storage/tests/repository.rs`：批量读取顺序、缺失和跨 Scope 契约。
- `crates/memory-core/src/recall.rs`：候选批量读取与端到端 Deadline。
- `crates/memory-core/tests/recall_bounds.rs`：Recall 超时和结果一致性。
- `services/memory-server/src/metrics.rs`：固定 operation、固定 Bucket 的低基数指标聚合与 Prometheus 渲染。
- `services/memory-server/src/lib.rs`：HTTP/gRPC 请求观测和 `/metrics` 状态接线。
- `services/memory-server/tests/protocol_parity.rs`：成功、429、timeout 指标契约。
- `scripts/benchmark/http-load.mjs`：本机优先的可复现 HTTP 压测执行器。
- `scripts/benchmark/tests/http-load.test.mjs`：参数、百分位、计数和输出脱敏测试。
- `benchmarks/schema/load-test.schema.json`：`jcmem.load-test.v1` JSON Schema。
- `docs/benchmarks/http-load.md`：中文运行、对比和边界说明。
- `web/backend/src/metrics.ts`：Prometheus 文本白名单解析与分位数归一化。
- `web/backend/src/upstream.ts`：独立读取实例 `/metrics`。
- `web/backend/src/routes/instances.ts`：受会话保护的实例性能快照接口。
- `web/backend/tests/metrics.test.ts`、`web/backend/tests/app.test.ts`：解析器和路由契约。
- `web/frontend/src/lib/performance.ts`：压测报告 Zod Schema、汇总和实时差分计算。
- `web/frontend/src/pages/performance-page.tsx`：静态报告与实时监控两个 Tab。
- `web/frontend/tests/performance-page.test.tsx`：导入、缺失值、图例和实时错误状态。
- `web/frontend/src/app.tsx`、`web/frontend/src/components/app-shell.tsx`：性能页面路由和导航。
- `web/frontend/src/index.css`：稳定图表尺寸、指标布局和响应式样式。
- `web/e2e/control-plane.spec.ts`、`web/e2e/fixture-memory-server.mjs`：性能主路径 E2E。

### Task 1: Scope 批量读取 Fragment

**Files:**
- Modify: `crates/memory-storage/src/repository.rs`
- Modify: `crates/memory-storage/tests/repository.rs`

- [x] **Step 1: 写批量读取红灯测试**

新增测试，写入两个同 Scope Fragment 和一个外部 Scope Fragment，要求输入逆序时仍按输入 ID 返回，并拒绝混入外部 ID：

```rust
let ids = [second.fragment_id, first.fragment_id];
let fragments = harness.repository
    .get_fragments_in_scope(&scope, &ids)
    .await
    .expect("批量读取同 Scope Fragment");
assert_eq!(fragments.iter().map(|item| item.id).collect::<Vec<_>>(), ids);

let error = harness.repository
    .get_fragments_in_scope(&scope, &[first.fragment_id, foreign.fragment_id])
    .await
    .unwrap_err();
assert!(matches!(error, StorageError::NotFound { entity: "fragment", .. }));
```

- [x] **Step 2: 运行测试并确认因方法缺失失败**

Run: `CARGO_INCREMENTAL=0 cargo test -p jc-memory-storage --test repository batch_fragment_lookup --offline`

Expected: FAIL，提示 `get_fragments_in_scope` 不存在。

- [x] **Step 3: 实现两次查询的批量读取**

在 Trait 增加：

```rust
async fn get_fragments_in_scope(
    &self,
    scope: &MemoryScope,
    ids: &[FragmentId],
) -> Result<Vec<CognitiveFragment>, StorageError>;
```

实现使用 `sqlx::QueryBuilder<Sqlite>` 参数化生成 `f.id IN (?1, ?2)` 形式的查询，主查询一次读取 Fragment/Content/Event 投影；第二次按 owner ID 批量读取 Evidence。把 `decode_fragment` 拆为接收已解码 Evidence 的纯投影校验函数，单条读取继续复用同一函数。结果先放入 `BTreeMap<FragmentId, CognitiveFragment>`，再按输入顺序取出；任何 ID 缺失均返回现有 `NotFound`。

- [x] **Step 4: 运行存储测试和静态门禁**

Run: `CARGO_INCREMENTAL=0 cargo test -p jc-memory-storage --test repository --offline`

Run: `cargo fmt --all -- --check && CARGO_INCREMENTAL=0 cargo clippy -p jc-memory-storage --all-targets --all-features --offline -- -D warnings`

Expected: 全部退出码 0。

- [x] **Step 5: 提交批量读取**

```bash
git add crates/memory-storage/src/repository.rs crates/memory-storage/tests/repository.rs
git commit -m "perf(storage): 批量读取 Recall 权威记录" -m "以精确 Scope 批量加载 Fragment 与 Evidence，消除候选逐条读取并保持权威投影校验、输入顺序和缺失记录语义。"
```

### Task 2: Recall 使用批量读取与完整 Deadline

**Files:**
- Modify: `crates/memory-core/src/recall.rs`
- Modify: `crates/memory-core/tests/recall_bounds.rs`

- [x] **Step 1: 写结果一致性与完整超时红灯测试**

在 `recall_bounds.rs` 新增多个候选 Recall 的 Packet 与 Evidence 一致性测试；在 `recall.rs` 的内部测试模块中，把完整执行包装函数传入超过预算的 Future，要求返回 `RecallEngineError::TimedOut`。

```rust
let error = run_with_recall_timeout(1, async {
    tokio::time::sleep(Duration::from_millis(20)).await;
    Ok::<_, MemoryError>(())
})
.await
.unwrap_err();
assert!(matches!(error, MemoryError::Recall(RecallEngineError::TimedOut)));
```

- [x] **Step 2: 运行测试并确认包装函数缺失**

Run: `CARGO_INCREMENTAL=0 cargo test -p jc-memory-core --test recall_bounds --offline`

Expected: FAIL，提示完整 Deadline 行为缺失。

- [x] **Step 3: 实现最小 Core 优化**

`execute_recall` 在授权后保存 `timeout_ms`，通过 `tokio::time::timeout` 包裹 `execute_recall_inner`。Inner 合并候选后收集 ID，一次调用 `get_fragments_in_scope`，按 ID Map 与候选 Hit 组合，不再循环调用 `get_fragment_in_scope`。激活引擎获得剩余毫秒预算，避免重新获得完整预算。

- [x] **Step 4: 运行 Core Recall、隔离和污染测试**

Run: `CARGO_INCREMENTAL=0 cargo test -p jc-memory-core --test recall_bounds --test tenant_isolation --test poisoning --test quarantine_security --offline`

Expected: 全部退出码 0。

- [x] **Step 5: 提交 Core 优化**

```bash
git add crates/memory-core/src/recall.rs crates/memory-core/tests/recall_bounds.rs
git commit -m "perf(recall): 消除候选串行读取并统一超时" -m "Recall 合并候选后按 Scope 批量读取权威记录，并让数据库、激活和 Packet 渲染共享同一端到端 Deadline。"
```

### Task 3: 版本化 HTTP 压测报告

**Files:**
- Create: `scripts/benchmark/http-load.mjs`
- Create: `scripts/benchmark/tests/http-load.test.mjs`
- Create: `benchmarks/schema/load-test.schema.json`
- Create: `docs/benchmarks/http-load.md`

- [x] **Step 1: 写百分位与安全边界红灯测试**

测试 `nearestRank`、计数一致性、本机 URL 默认允许、外部 URL 默认拒绝、报告 JSON 不包含 `JC_MEMORY_API_KEY`：

```javascript
assert.equal(nearestRank([1, 2, 9, 10], 0.95), 10);
assert.throws(() => assertLoadTarget(new URL("https://memory.example.com"), false));
assert.equal(JSON.stringify(report).includes(process.env.JC_MEMORY_API_KEY ?? "never"), false);
```

- [x] **Step 2: 运行 Node 测试并确认模块缺失**

Run: `node --test scripts/benchmark/tests/http-load.test.mjs`

Expected: FAIL，提示 `http-load.mjs` 不存在。

- [x] **Step 3: 实现执行器与 Schema**

配置 JSON 包含 `base_url`、`warmup_requests` 和场景数组；场景包含 `name`、`method`、`path`、`requests`、`concurrency`、可选 `body_file`。API Key 只从 `JC_MEMORY_API_KEY` 读取。输出固定字段：

```json
{
  "schema_version": "jcmem.load-test.v1",
  "provenance": {"git_commit": "a8f0b46", "dirty": false, "platform": "darwin-arm64"},
  "config": {"target": "loopback", "real_rust_sqlite": true},
  "scenarios": [{"name": "recall-c32", "requests": 800, "concurrency": 32,
    "success": 800, "failure": 0, "status_counts": {"200": 800},
    "throughput_rps": 13.48, "latency_ms": {"p50": 2320.82, "p95": 2738.33, "p99": 3169.88, "max": 3200.0}}]
}
```

- [x] **Step 4: 验证测试、Schema 和中文文档**

Run: `node --test scripts/benchmark/tests/http-load.test.mjs`

Run: `jq empty benchmarks/schema/load-test.schema.json`

Run: `git diff --check`

Expected: 全部退出码 0。

- [x] **Step 5: 提交压测工具**

```bash
git add scripts/benchmark/http-load.mjs scripts/benchmark/tests/http-load.test.mjs benchmarks/schema/load-test.schema.json docs/benchmarks/http-load.md
git commit -m "feat(benchmarks): 增加可视化 HTTP 压测报告" -m "新增本机优先的并发压测执行器和 jcmem.load-test.v1 Schema，记录吞吐、错误率与 P50/P95/P99，并确保报告不包含 API Key。"
```

### Task 4: Rust Server 实时指标

**Files:**
- Create: `services/memory-server/src/metrics.rs`
- Modify: `services/memory-server/src/lib.rs`
- Modify: `services/memory-server/tests/protocol_parity.rs`

- [ ] **Step 1: 写指标红灯测试**

对成功 Observe、SlowBus timeout 和超过 `max_in_flight` 的请求执行后读取 `/metrics`，断言包含固定 operation、累计 Bucket、429 和 timeout，且不包含 API Key、request ID 或 Scope ID。

- [ ] **Step 2: 运行测试并确认现有 metrics 只有 ready**

Run: `CARGO_INCREMENTAL=0 cargo test -p jc-memory-server --test protocol_parity metrics --offline`

Expected: FAIL，缺少 `jc_memory_requests_total`。

- [ ] **Step 3: 实现无高基数指标聚合**

`ServerMetrics` 使用固定 operation 枚举、`AtomicU64` 计数器和固定秒级 Histogram Bucket。`command_inner` 与 `GrpcAdapter::execute` 记录整个请求耗时和稳定结果类别；`/metrics` 从 `State` 渲染 Prometheus 0.0.4 文本，in-flight 直接读取现有原子计数。

- [ ] **Step 4: 运行 Server 测试和 Clippy**

Run: `CARGO_INCREMENTAL=0 cargo test -p jc-memory-server --all-targets --all-features --offline`

Run: `cargo fmt --all -- --check && CARGO_INCREMENTAL=0 cargo clippy -p jc-memory-server --all-targets --all-features --offline -- -D warnings`

Expected: 全部退出码 0。

- [ ] **Step 5: 提交 Server 指标**

```bash
git add services/memory-server/src/metrics.rs services/memory-server/src/lib.rs services/memory-server/tests/protocol_parity.rs
git commit -m "feat(server): 暴露请求延迟与背压指标" -m "按固定 operation 记录请求、耗时 Histogram、in-flight、429 与 timeout，不把 Scope、凭据或请求标识写入指标标签。"
```

### Task 5: BFF 指标归一化接口

**Files:**
- Create: `web/backend/src/metrics.ts`
- Create: `web/backend/tests/metrics.test.ts`
- Modify: `web/backend/src/upstream.ts`
- Modify: `web/backend/src/routes/instances.ts`
- Modify: `web/backend/tests/app.test.ts`

- [ ] **Step 1: 写 Prometheus 解析红灯测试**

使用固定 metrics Fixture，断言只接受白名单指标，并把累计 Bucket 转为 operation 的 total、success、error、rateLimited、timeouts、inFlight、P50/P95/P99；未知标签和 `NaN` 必须拒绝。

- [ ] **Step 2: 运行测试并确认解析器缺失**

Run: `cd web && corepack pnpm --filter ./backend test -- metrics.test.ts`

Expected: FAIL，提示 `metrics.ts` 不存在。

- [ ] **Step 3: 实现解析器、Upstream 和受保护路由**

增加 `upstream.metrics(connection)`，只 GET `/metrics`。增加 `GET /api/v1/instances/:id/metrics`，要求有效 Session，从 Store 取服务端凭据并返回归一化 `PerformanceSnapshot`；响应不得包含原始 Prometheus 文本或 API Key。

- [ ] **Step 4: 运行 BFF 测试与 Typecheck**

Run: `cd web && corepack pnpm --filter ./backend test`

Run: `cd web && corepack pnpm --filter ./backend typecheck`

Expected: 全部退出码 0。

- [ ] **Step 5: 提交 BFF 指标接口**

```bash
git add web/backend/src/metrics.ts web/backend/tests/metrics.test.ts web/backend/src/upstream.ts web/backend/src/routes/instances.ts web/backend/tests/app.test.ts
git commit -m "feat(web): 增加实例性能指标接口" -m "BFF 白名单解析 Prometheus 指标并返回稳定性能快照，实例凭据和原始标签继续留在服务端边界。"
```

### Task 6: 静态报告图表页面

**Files:**
- Create: `web/frontend/src/lib/performance.ts`
- Create: `web/frontend/src/pages/performance-page.tsx`
- Create: `web/frontend/tests/performance-page.test.tsx`
- Modify: `web/frontend/package.json`
- Modify: `web/pnpm-lock.yaml`
- Modify: `web/frontend/src/app.tsx`
- Modify: `web/frontend/src/components/app-shell.tsx`
- Modify: `web/frontend/src/index.css`

- [ ] **Step 1: 写报告解析和页面红灯测试**

测试合法 `jcmem.load-test.v1` 显示总请求、成功率、P50/P95/P99、吞吐和 429；未知版本显示中文错误；`null` 指标显示“未采集”；文件内容只在浏览器解析。

- [ ] **Step 2: 运行测试并确认页面缺失**

Run: `cd web && corepack pnpm --filter ./frontend test -- performance-page.test.tsx`

Expected: FAIL，提示 `PerformancePage` 不存在。

- [ ] **Step 3: 安装 Recharts 并实现静态报告 Tab**

Run: `cd web && corepack pnpm --filter ./frontend add recharts@latest`

实现 Zod Schema、汇总函数和文件选择器。使用 Recharts 的 `BarChart` 展示 P50/P95/P99、`LineChart` 展示并发与吞吐、`BarChart` 展示状态码；所有图表包含文字标题、Legend、Tooltip、固定高度和表格替代信息。

- [ ] **Step 4: 接入路由、导航和响应式样式**

新增 `/instances/:id/performance` 路由；导航使用 Lucide `Gauge` 图标。图表容器固定 `min-height`，移动端表格允许横向滚动，按钮和文本不得溢出。

- [ ] **Step 5: 运行前端测试、Typecheck 和 Build**

Run: `cd web && corepack pnpm --filter ./frontend test`

Run: `cd web && corepack pnpm --filter ./frontend typecheck`

Run: `cd web && corepack pnpm --filter ./frontend build`

Expected: 全部退出码 0。

- [ ] **Step 6: 提交静态图表**

```bash
git add web/frontend web/pnpm-lock.yaml
git commit -m "feat(web): 增加压测报告可视化页面" -m "本地解析版本化压测 JSON，以延迟分位数、并发吞吐、状态码和明细表展示真实结果，缺失指标明确标记为未采集。"
```

### Task 7: 实时监控 Tab 与 E2E

**Files:**
- Modify: `web/frontend/src/lib/contracts.ts`
- Modify: `web/frontend/src/lib/api.ts`
- Modify: `web/frontend/src/lib/performance.ts`
- Modify: `web/frontend/src/pages/performance-page.tsx`
- Modify: `web/frontend/tests/performance-page.test.tsx`
- Modify: `web/e2e/fixture-memory-server.mjs`
- Modify: `web/e2e/control-plane.spec.ts`

- [ ] **Step 1: 写实时采样红灯测试**

模拟两个累计快照，断言差分产生每秒请求数；采样失败后图表插入 gap 并显示最后成功时间，不把失败样本写成零。

- [ ] **Step 2: 运行测试并确认实时行为缺失**

Run: `cd web && corepack pnpm --filter ./frontend test -- performance-page.test.tsx`

Expected: FAIL，缺少实时 Tab 和 `api.instanceMetrics`。

- [ ] **Step 3: 实现轮询与环形样本**

React Query 每 5 秒读取 `GET /api/v1/instances/:id/metrics`，只保留最近 60 个样本。总计数差分计算 RPS 和错误率；累计 Histogram 分位数直接显示。离线时保留最后成功数据并停止连线。

- [ ] **Step 4: 增加 Fixture 与 Playwright 主路径**

Fixture `/metrics` 返回两次递增的固定 Prometheus 文本。Playwright 登录、打开“性能”、验证实时指标，再导入压测 Fixture 并验证 P95 和 429 图表标题。

- [ ] **Step 5: 运行 Web 完整门禁**

Run: `cd web && corepack pnpm test`

Run: `cd web && corepack pnpm typecheck`

Run: `cd web && corepack pnpm build`

Run: `cd web && corepack pnpm test:e2e`

Expected: 全部退出码 0。

- [ ] **Step 6: 提交实时监控**

```bash
git add web/frontend web/e2e
git commit -m "feat(web): 增加实时请求与延迟监控" -m "性能页面轮询受保护的实例指标，展示 RPS、成功率、in-flight、P50/P95/P99、429 和 timeout，并对采样缺口保持诚实。"
```

### Task 8: 同条件复测、文档和最终门禁

**Files:**
- Modify: `docs/benchmarks/http-load.md`
- Modify: `docs/release/verification-report.md`
- Modify: `docs/superpowers/plans/2026-08-01-recall-performance-observability-implementation.md`
- Create: `target/benchmarks/recall-after.json`（gitignored，不提交）

- [ ] **Step 1: 使用相同配置运行 Recall c8/c32/c64**

以隔离端口、相同数据库规模、相同预热和相同请求体运行新压测脚本，输出 `target/benchmarks/recall-after.json`。不得覆盖之前报告。

- [ ] **Step 2: 对比前后数据**

记录吞吐、P50/P95/P99、错误率、状态码和最终 Fragment/Event 计数。只有实测改善才写入结论；若未改善，保留报告并回到根因分析。

- [ ] **Step 3: 运行最终质量门禁**

Run: `cargo fmt --all -- --check`

Run: `CARGO_INCREMENTAL=0 cargo clippy -p jc-memory-storage -p jc-memory-core -p jc-memory-server --all-targets --all-features --offline -- -D warnings`

Run: `CARGO_INCREMENTAL=0 cargo test -p jc-memory-storage -p jc-memory-core -p jc-memory-server --all-targets --all-features --offline`

Run: `RUSTDOCFLAGS='-D warnings' cargo doc -p jc-memory-storage -p jc-memory-core -p jc-memory-server --no-deps --all-features --offline`

Run: `cd web && corepack pnpm test && corepack pnpm typecheck && corepack pnpm build && corepack pnpm test:e2e`

Run: `git diff --check`

Expected: 全部退出码 0；任何失败单独记录，不能把静态通过描述为实机通过。

- [ ] **Step 4: 更新完成勾选与中文验证记录**

在本计划中逐项勾选实际完成步骤；在发布验证报告中写入命令、结果、压测前后数据和未验证边界。

- [ ] **Step 5: 提交验证记录**

```bash
git add docs/benchmarks/http-load.md docs/release/verification-report.md docs/superpowers/plans/2026-08-01-recall-performance-observability-implementation.md
git commit -m "docs(perf): 记录 Recall 优化与可视化验收" -m "记录聚焦测试、Web E2E、同条件压测对比和未验证边界，并勾选实际完成的实施步骤。"
```

- [ ] **Step 6: 推送功能分支**

Run: `git push origin feat/performance-observability`

Expected: 远端分支更新成功，工作树保持干净。
