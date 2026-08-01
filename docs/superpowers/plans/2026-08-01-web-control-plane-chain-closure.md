# Web 控制面链路闭环实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**目标：** 修复真实 Server 暴露的 Stats、Runtime Start 来源校验和默认 Observe -> Recall 断点，并让 Playwright 在隔离端口上验证写入内容可被召回。

**架构：** BFF 按 Rust `MemoryCommand` 的实际 Serde 形状构造请求；所有带副作用的控制面路由统一执行 Session 与 Origin 校验。Memory Lab 默认写入 `Unverified Fact` 以形成可演示的 Recall 闭环，同时保留 Observation 选项，不改变 Rust 权威记录与事实验证边界。Fixture E2E 保存真实写入内容并按查询返回，端口全部由环境变量注入。

**技术栈：** TypeScript、Fastify、React、Vitest、Playwright、Rust memory-server

---

### Task 1：修复 Stats 协议形状

**文件：**
- 修改：`web/backend/tests/upstream.test.ts`
- 修改：`web/backend/tests/app.test.ts`
- 修改：`web/backend/src/protocol.ts`
- 修改：`sdk/contracts/fixtures/stats-basic.json`
- 修改：`sdk/node/src/types.ts`
- 修改：`sdk/node/src/__tests__/remote.test.ts`
- 修改：`sdk/python/tests/test_remote_client.py`

- [x] 先将测试期望改为 `payload: null`，运行 Backend 聚焦测试并确认因当前 `{}` 实现失败。
- [x] 让 Stats Envelope 按 operation 输出正确 payload。
- [x] 更新并重新生成 SDK Fixture 的 canonical SHA-256。
- [x] 运行 Backend 测试与 Rust SDK Fixture 契约测试。
- [x] 修正 Node Stats 公开类型和 Node/Python 固定请求，并运行六种 SDK 的本机门禁。

### Task 2：关闭 Runtime Start 的跨来源写入

**文件：**
- 修改：`web/backend/tests/app.test.ts`
- 修改：`web/backend/src/routes/runtime.ts`

- [x] 增加携带有效 Session、恶意 Origin 的 Runtime Start 测试，并确认当前没有在路由边界返回 `403`。
- [x] 对 Runtime Start 同时调用 `requireSession` 与 `requireAllowedOrigin`。
- [x] 运行 Runtime BFF 聚焦测试，确认恶意 Origin 返回 `403` 且没有请求上游。

### Task 3：闭合默认 Observe -> Recall

**文件：**
- 修改：`web/frontend/tests/memory-lab-page.test.tsx`
- 修改：`web/frontend/src/pages/memory-lab-page.tsx`

- [x] 增加默认 Fragment Kind 为 `fact`、Validity 仍为 `unverified` 的组件测试，并确认当前默认值导致失败。
- [x] 将 Observe 与 Commit 的默认 Kind 改为 `fact`，保留 `observation` 选项。
- [x] 运行 Memory Lab 聚焦测试与前端完整测试。

### Task 4：增强可重复 E2E

**文件：**
- 修改：`web/playwright.config.ts`
- 修改：`web/e2e/start-backend.mjs`
- 修改：`web/e2e/fixture-memory-server.mjs`
- 修改：`web/e2e/control-plane.spec.ts`
- 修改：`web/package.json`
- 修改：`web/frontend/vite.config.ts`
- 新增：`web/e2e/run-playwright.mjs`
- 新增：`web/e2e/start-real-memory-server.mjs`

- [x] 让测试先断言 Memory Packet 包含刚 Observe 的文本，并确认现有固定空 Fixture 失败。
- [x] 让 Fixture 保存 Observe/Commit Fragment，并在 Recall 返回匹配的 Point、Episode 和 Evidence。
- [x] 使用 `JC_WEB_E2E_*_PORT` 参数化 Browser、BFF 和 Memory Server 端口。
- [x] 新增真实 Rust Server 启动包装器和 `test:e2e:real`，只使用临时数据目录与本地 Fixture Provider。
- [x] 在隔离端口运行 Fixture Playwright 和真实 Rust Server Playwright。

### Task 5：门禁与文档状态

**文件：**
- 修改：`docs/superpowers/specs/2026-07-31-jiangcheng-memory-web-control-plane-design.md`
- 修改：`docs/superpowers/plans/2026-08-01-web-control-plane-chain-closure.md`

- [x] 更新已经被当前仓库证据推翻的 M4/M5/M6 状态描述。
- [x] 勾选本计划完成项。
- [x] 运行 `pnpm test`、`pnpm typecheck`、`pnpm build`、两类 E2E、适用 Rust 测试、fmt、Clippy、doc 和 `git diff --check`。
