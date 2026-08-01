# 疆诚记忆 Web 控制面 Runtime Start Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 交付一条配置化 `OpenAI Compatible Provider` 驱动的真实 Runtime FinalResponse 链路，从 Web 发起 Run，并经 `CommandBus` 执行 Observe、Recall、非事实化 Commit 与持久化 Transition。

**Architecture:** `RuntimeService` 先补齐 Recall Context 进入模型请求与 Transition Payload 的真实数据流；`memory-server` 新增 `CommandBusRuntimeBackend`，只通过现有稳定 `MemoryCommand` 访问 Core；Server 启动配置可选 Provider，未配置时 Start 返回 503。BFF 只代理固定 Start 路由，浏览器不提交 Scope、Grant、Provider Secret 或 API Key。当前批次只发布 `FinalResponse` 路径，不伪造 Approval、Tool Execute 或恢复能力。

**Tech Stack:** Rust 1.97、Axum、SQLx/SQLite、reqwest OpenAI Compatible Provider、TypeScript 7、Fastify 5、React 19、TanStack Query、Vitest、Playwright。

---

### Task 1: RuntimeService 消费真实 Recall Context

**Files:**
- Modify: `crates/memory-runtime/src/service.rs`
- Modify: `crates/memory-runtime/src/lib.rs`
- Modify: `crates/memory-runtime/tests/service.rs`

- [x] **Step 1: 写失败测试**

把 `RecordingBackend::recall` 改为返回包含 `recommended_context` 和结构化 `transition_payload` 的 `RuntimeRecall`；Provider 记录收到的消息。断言模型请求同时包含用户原始 Prompt 与单独的 Memory Context，Recall Transition Payload 保存 Packet 摘要但不覆盖权威 Fragment。

- [x] **Step 2: 运行测试并确认 RED**

Run: `CARGO_INCREMENTAL=0 cargo test -p jc-memory-runtime --test service --offline`

Expected: FAIL，`RuntimeRecall` 尚不存在，Provider 当前只收到原始 Prompt。

- [x] **Step 3: 实现最小领域契约**

新增 `RuntimeRecall { recommended_context: String, transition_payload: serde_json::Value, evidence_refs: Vec<EvidenceRef> }`。`RuntimeBackend::recall` 返回该类型；`RuntimeService` 把 `recommended_context` 作为 System Message，把原始 Prompt 保留为 User Message，并把 `transition_payload` 写入 recall Transition Payload。空 EvidenceRef 不得由 Server 臆造验证方法。

- [x] **Step 4: 运行聚焦测试并确认 GREEN**

Run: `CARGO_INCREMENTAL=0 cargo test -p jc-memory-runtime --test service --offline`

Expected: PASS。

### Task 2: CommandBusRuntimeBackend 权威映射

**Files:**
- Create: `services/memory-server/src/runtime_backend.rs`
- Modify: `services/memory-server/src/lib.rs`
- Create: `services/memory-server/tests/runtime_backend.rs`

- [x] **Step 1: 写失败契约测试**

使用 Recording `CommandBus` 断言 Observe、Recall、Commit 的 `request_id`、精确 Scope 和命令顺序。Observe 写 `Observation + User + Unverified`；Recall 使用有界 `ContextCompletion`；FinalResponse 只写 `Inference + Model + Unverified`。断言任何路径都不写 `Fact + Verified`，Core 错误被脱敏为 Runtime Backend 错误。

- [x] **Step 2: 运行并确认 RED**

Run: `CARGO_INCREMENTAL=0 cargo test -p jc-memory-server --test runtime_backend --offline`

Expected: FAIL，`CommandBusRuntimeBackend` 尚不存在。

- [x] **Step 3: 实现 CommandBus 适配器**

适配器只构造 `MemoryRequest` 并调用共享 `CommandBus`。Recall 返回 `MemoryPacket.recommended_context`，并把 `query_id`、`coverage`、`confidence`、`missing_evidence` 与 EvidenceView 标识写入派生 Transition Payload；不合成不存在的 `VerificationMethod`。Tool Commit 当前返回明确 unsupported，直到 Tool Fragment 映射和审批恢复计划完成。

- [x] **Step 4: 运行聚焦测试并确认 GREEN**

Run: `CARGO_INCREMENTAL=0 cargo test -p jc-memory-server --test runtime_backend --offline`

Expected: PASS。

### Task 3: 配置化 Provider 与 Runtime Start HTTP 契约

**Files:**
- Modify: `services/memory-server/Cargo.toml`
- Modify: `services/memory-server/src/config.rs`
- Modify: `services/memory-server/src/lib.rs`
- Modify: `services/memory-server/src/main.rs`
- Create: `services/memory-server/tests/runtime_start_api.rs`
- Modify: `docs/configuration.zh-CN.md`
- Modify: `docs/server.zh-CN.md`
- Modify: `docs/runtime.zh-CN.md`

- [x] **Step 1: 写失败 API 与配置测试**

断言 `POST /v1/runtime/runs` 不接受客户端 Scope/Grant，Prompt 必须为 1..32768 字符；API Key Scope 缺少 Mutate 时返回 403；未配置 Runtime Service 返回 503；配置 Fixture Provider 时返回 Completed Run ID 并能由 GET Detail 读取完整 Transition。配置要求 endpoint/model 成对出现，Token 只从环境变量读取，Debug 输出不得包含 Server API Key 或 Provider Token。

- [x] **Step 2: 运行并确认 RED**

Run: `CARGO_INCREMENTAL=0 cargo test -p jc-memory-server --test runtime_start_api --offline`

Expected: FAIL，Start 路由和 Runtime Service 注入尚不存在。

- [x] **Step 3: 实现有界 Start 路由与启动装配**

`ServerState` 增加可选 `Arc<RuntimeService>`；`POST /v1/runtime/runs` 从 API Key 构造精确 `AccessContext`，使用 Server 请求超时、CancellationToken 和统一并发/速率限制。`main` 仅在 Provider 配置完整时装配 `OpenAiCompatibleProvider`、`CommandBusRuntimeBackend`、空 Tool Registry 与共享 Runtime Store；未配置时查询仍可用，Start 明确 503。响应只返回 outcome、run_id、state 与 FinalResponse 文本，不返回 Secret。

- [x] **Step 4: 运行聚焦测试并确认 GREEN**

Run: `CARGO_INCREMENTAL=0 cargo test -p jc-memory-server --test runtime_start_api --offline`

Expected: PASS。

### Task 4: BFF 与 Web Start Run

**Files:**
- Modify: `web/backend/src/upstream.ts`
- Modify: `web/backend/src/routes/runtime.ts`
- Modify: `web/backend/tests/app.test.ts`
- Modify: `web/frontend/src/lib/contracts.ts`
- Modify: `web/frontend/src/lib/api.ts`
- Modify: `web/frontend/src/pages/runtime-page.tsx`
- Modify: `web/frontend/tests/runtime-page.test.tsx`
- Modify: `web/frontend/src/index.css`

- [x] **Step 1: 写失败 BFF 与页面测试**

断言 BFF 只把 `{ prompt }` 发送到固定 Start 路由，API Key 仅写上游 Header，不代理 Scope、Grant 或 Provider 配置；页面提交后选择服务端返回的 Run ID、刷新列表和详情，并展示 FinalResponse。503 显示“当前实例未配置 Runtime Provider”，不使用模拟响应。

- [x] **Step 2: 运行并确认 RED**

Run: `cd web && pnpm --filter @jiangcheng-memory/control-plane-backend test -- app.test.ts && pnpm --filter @jiangcheng-memory/control-plane-frontend test -- runtime-page.test.tsx`

Expected: FAIL，Start 代理与表单尚不存在。

- [x] **Step 3: 实现最小 Start 工作流**

页面使用紧凑 Prompt 表单和明确的启动命令按钮；请求期间禁用重复提交；成功后将 `?run=<run_id>` 写入 URL 并刷新真实查询。页面不显示 Approval、Tool、Pause、Resume 或 Provider Secret 控件。

- [x] **Step 4: 运行聚焦测试并确认 GREEN**

Run: `cd web && pnpm test && pnpm typecheck`

Expected: PASS。

### Task 5: Fixture E2E、能力矩阵与完整门禁

**Files:**
- Modify: `web/e2e/fixture-memory-server.mjs`
- Modify: `web/e2e/control-plane.spec.ts`
- Modify: `web/frontend/src/pages/capabilities-page.tsx`
- Modify: `web/README.zh-CN.md`

- [x] **Step 1: 扩展版本化契约 Fixture 与 E2E**

Fixture 接受一次 Start 并返回 Completed Run；desktop/mobile 从 Runtime 页面输入 Prompt、启动、自动打开 Timeline，断言响应和 DOM 不包含 API Key、Provider Token、Approval 或 Tool 伪记录。Fixture 只证明契约，不描述为外部模型实机验证。

- [x] **Step 2: 更新真实能力边界**

把 `Runtime Start (FinalResponse)` 标为 LIVE；Approval、Tool Execute、Pause/Resume、Provider 管理页面继续为 CORE。文档写明生产 Start 依赖 Server 进程配置的 Provider，浏览器和控制面数据库不保存 Provider Secret。

- [x] **Step 3: 运行完整门禁**

Run:

```bash
cargo fmt --all -- --check
CARGO_INCREMENTAL=0 cargo clippy -p jc-memory-runtime -p jc-memory-server --all-targets --offline -- -D warnings
CARGO_INCREMENTAL=0 cargo test -p jc-memory-runtime -p jc-memory-server --offline
cd web
pnpm test
pnpm typecheck
pnpm build
pnpm exec playwright test
cd ..
git diff --check
```

Expected: 全部退出码为 0；未使用真实外部 Provider 时只声明本地 Fixture/契约验证。

## 规格覆盖复核

- Browser 不持有实例 API Key、Provider Token、Scope Grant 或 SQLite 访问能力。
- Prompt 作为原始 Observation 追加；Recall Context 进入模型但不覆盖权威记录。
- FinalResponse 作为 `Inference + Unverified` 追加，绝不写为 `Verified Fact`。
- Runtime Start 使用真实 Provider、CommandBus、Runtime Store 和 Transition，不在生产页面使用 Fixture。
- Approval、Tool Execute、Pause/Resume 和 Provider 管理仍无完整持久化/恢复契约，因此不提供操作入口。
