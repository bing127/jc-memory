# 疆诚记忆 Web 控制面 Runtime Explorer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 基于 `runtime_runs`、不可变 Runtime Transition 和 Control Checkpoint 的真实持久化数据，交付按精确 Scope 授权的 Run 列表、详情时间线和 Web Runtime Explorer。

**Architecture:** `jc-memory-runtime` 新增只读查询仓储，所有查询先用 `AccessContext` 和精确 Scope 执行默认拒绝 ACL；`memory-server` 在同一数据目录打开共享 SQLite pool，只通过固定 `/v1/runtime/runs` 路由返回分页 DTO；Web BFF 继续只持有加密实例凭据并代理固定路径；浏览器不直接读取数据库，也不构造 Grant。当前批次不伪造 Approval、ToolResult 或模型输出，缺少持久化查询契约的字段明确标为未提供。

**Tech Stack:** Rust 1.97、SQLx/SQLite、Axum、TypeScript 7、Fastify 5、React 19、TanStack Query、Vitest、Playwright。

---

### Task 1: Runtime 只读查询仓储

**Files:**
- Create: `crates/memory-runtime/src/query.rs`
- Modify: `crates/memory-runtime/src/lib.rs`
- Modify: `crates/memory-runtime/src/runner.rs`
- Create: `crates/memory-runtime/tests/query.rs`

- [x] **Step 1: 写失败测试**

创建两个 Scope 的 Run 与 Transition，断言列表只返回授权 Scope，支持 state/limit/offset，详情按 step 返回 Transition 和 Control Checkpoint；跨 Scope、无 Recall Grant、损坏 JSON 均拒绝。

- [x] **Step 2: 运行测试并确认 RED**

Run: `CARGO_INCREMENTAL=0 cargo test -p jc-memory-runtime --test query --offline`

Expected: FAIL，`RuntimeReadStore` 尚不存在。

- [x] **Step 3: 实现有界查询 DTO**

列表 `limit` 为 1..100，`offset` 有上限；按 `updated_at_us DESC, run_id DESC` 排序。详情只查询 `run_id + scope_key`，Transition 的 `input_refs_json`、`output_refs_json` 和 `payload_json` 使用结构化反序列化，不能以未校验字符串返回。

- [x] **Step 4: 运行聚焦测试并确认 GREEN**

Run: `CARGO_INCREMENTAL=0 cargo test -p jc-memory-runtime --test query --offline`

Expected: PASS。

### Task 2: memory-server Runtime 查询契约

**Files:**
- Modify: `services/memory-server/Cargo.toml`
- Modify: `services/memory-server/src/lib.rs`
- Modify: `services/memory-server/src/main.rs`
- Create: `services/memory-server/tests/runtime_api.rs`
- Modify: `docs/server.zh-CN.md`
- Modify: `docs/runtime.zh-CN.md`

- [x] **Step 1: 写失败契约测试**

测试 API Key Scope 绑定、分页参数、状态过滤、详情 Transition，以及未配置 Runtime Store 时的稳定 unavailable 错误；响应不得包含 API Key 或其他 Scope 的 Run ID。

- [x] **Step 2: 运行并确认 RED**

Run: `CARGO_INCREMENTAL=0 cargo test -p jc-memory-server --test runtime_api --offline`

Expected: FAIL，Runtime 路由尚不存在。

- [x] **Step 3: 实现固定只读路由**

注册 `GET /v1/runtime/runs` 和 `GET /v1/runtime/runs/{run_id}`；从服务端 API Key 构造 AccessContext，客户端不能提交 Scope 或 Grant；复用并发、速率、超时与稳定错误边界。

- [x] **Step 4: 运行聚焦测试并确认 GREEN**

Run: `CARGO_INCREMENTAL=0 cargo test -p jc-memory-server --test runtime_api --offline`

Expected: PASS。

### Task 3: BFF 与 Runtime Explorer 页面

**Files:**
- Modify: `web/backend/src/upstream.ts`
- Create: `web/backend/src/routes/runtime.ts`
- Modify: `web/backend/src/app.ts`
- Modify: `web/backend/tests/app.test.ts`
- Modify: `web/frontend/src/lib/contracts.ts`
- Modify: `web/frontend/src/lib/api.ts`
- Create: `web/frontend/src/pages/runtime-page.tsx`
- Modify: `web/frontend/src/components/app-shell.tsx`
- Modify: `web/frontend/src/app.tsx`
- Create: `web/frontend/tests/runtime-page.test.tsx`

- [x] **Step 1: 写 BFF 与页面失败测试**

断言 BFF 只代理固定 Runtime 路由并隐藏 API Key；页面渲染真实状态、Run ID、更新时间、Checkpoint 和按 step 排序的 Transition，不渲染虚构 Approval/Model/Tool 数据。

- [x] **Step 2: 运行并确认 RED**

Run: `cd web && pnpm test`

Expected: FAIL，Runtime 路由与页面尚不存在。

- [x] **Step 3: 实现页面和降级状态**

Runtime 导航只在查询契约可用时展示真实数据；实例返回 404/503 时显示 `CORE` 边界，不使用 Fixture 数据填充生产页面。详情使用 URL 选择 Run，不把正文写入控制面 SQLite。

- [x] **Step 4: 运行聚焦测试并确认 GREEN**

Run: `cd web && pnpm test && pnpm typecheck`

Expected: PASS。

### Task 4: Fixture E2E、文档与门禁

**Files:**
- Modify: `web/e2e/fixture-memory-server.mjs`
- Modify: `web/e2e/control-plane.spec.ts`
- Modify: `web/frontend/src/pages/capabilities-page.tsx`
- Modify: `web/README.zh-CN.md`

- [x] **Step 1: 增加版本化 Runtime 查询 Fixture**

Fixture 返回一条 Completed Run、完整 Transition 时间线和 Checkpoint 空状态；E2E 在桌面与移动视口展开详情，并断言页面没有 Approval/Tool 的伪记录。

- [x] **Step 2: 更新能力矩阵与边界文档**

Run/Transition/Checkpoint 标记为 LIVE；Runtime Start、Approval Decision、Tool Execute 和模型配置继续保留 CORE，直到独立服务契约及真实 E2E 完成。

- [x] **Step 3: 运行完整门禁**

Run:

```bash
CARGO_INCREMENTAL=0 cargo fmt --all -- --check
CARGO_INCREMENTAL=0 cargo clippy -p jc-memory-runtime -p jc-memory-server --all-targets -- -D warnings
CARGO_INCREMENTAL=0 cargo test -p jc-memory-runtime -p jc-memory-server --offline
cd web
pnpm test
pnpm typecheck
pnpm build
pnpm exec playwright test
cd ..
git diff --check
```

Expected: 全部退出码为 0；Fixture 结果只描述为契约 E2E，不描述为外部模型或平台实机验证。

## 规格覆盖复核

- Run 列表具备分页、状态和精确 Scope 过滤。
- Run Detail 展示真实 Transition、EvidenceRef、Payload、Deadline、Trace 与 Checkpoint。
- 所有查询默认拒绝，客户端不能自行扩大 Scope 或 Operation。
- Prediction、Plan、Approval、ToolResult 未有真实持久化查询契约时不会出现在页面。
- Runtime 执行与审批控制留给后续独立计划，不在只读查询中伪造。
