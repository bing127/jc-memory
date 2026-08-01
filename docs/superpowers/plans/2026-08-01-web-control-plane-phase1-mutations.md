# 疆诚记忆 Web 控制面 Phase 1 写操作闭环 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在不覆盖原始 Fragment/Event/EvidenceRef 的前提下，为现有 `memory-server` 的 Commit、Correct 和 Forget 契约提供严格校验、显式确认、可审计原因和结构化结果展示。

**Architecture:** 浏览器只提交受控表单 DTO，Fastify BFF 将实例 Namespace 绑定到完整协议 Envelope 并代理到固定上游路径。Commit 只接受一到二十条完整 Fragment；Correct/Forget 只追加带 EvidenceRef 的 Mutation，不删除或改写目标 Fragment。所有写操作关闭自动重试，危险操作需要用户输入目标 Fragment ID 进行二次确认。

**Tech Stack:** TypeScript 7、Fastify 5、Zod 4、React 19、TanStack Query、Radix Tabs、Vitest、Testing Library、Playwright。

---

## 文件结构

```text
web/backend/src/routes/memory.ts       # 三种写操作 DTO 校验与 Scope 绑定
web/backend/tests/app.test.ts          # BFF 契约、Scope 防篡改和拒绝路径
web/frontend/src/lib/api.ts            # 浏览器写操作白名单
web/frontend/src/pages/memory-lab-page.tsx
                                         # Commit/Correct/Forget 表单和确认状态
web/frontend/tests/memory-lab-page.test.tsx
                                         # 防误操作交互测试
web/e2e/control-plane.spec.ts          # Fixture 主路径
web/README.zh-CN.md                    # 当前真实能力与安全边界
```

### Task 1: BFF 写操作契约

**Files:**
- Modify: `web/backend/src/routes/memory.ts`
- Modify: `web/backend/tests/app.test.ts`

- [x] **Step 1: 写失败测试**

测试 `commit` 把每个 Fragment 的 `scope` 覆盖为已选择实例 Scope；测试 `correct`/`forget` 只接受 UUID 目标、非空原因、明确验证方法和完整来源，并按协议 snake_case 转发。

- [x] **Step 2: 运行测试并确认 RED**

Run: `cd web && pnpm --filter @jiangcheng-memory/control-plane-backend test -- app.test.ts`

Expected: FAIL，当前路由白名单拒绝三个操作。

- [x] **Step 3: 实现最小严格校验与映射**

`paramsSchema` 开放三个现有 Server 操作；Commit 限制 1..20 条 Fragment；Mutation 生成 `content/source/perspective/occurred_at/verification_method/reason`，BFF 不接受客户端自带 Scope 或任意上游路径。

- [x] **Step 4: 运行聚焦测试并确认 GREEN**

Run: `cd web && pnpm --filter @jiangcheng-memory/control-plane-backend test -- app.test.ts`

Expected: PASS。

### Task 2: 防误操作 Memory Lab

**Files:**
- Modify: `web/frontend/src/lib/api.ts`
- Modify: `web/frontend/src/pages/memory-lab-page.tsx`
- Modify: `web/frontend/tests/memory-lab-page.test.tsx`

- [x] **Step 1: 写失败测试**

测试页面出现 Commit、Correct、Forget；Correct 必须填写目标、原因和新内容；Forget 只有确认文本与目标 UUID 完全一致时才可提交，且页面明确说明原始权威记录不会被覆盖或物理删除。

- [x] **Step 2: 运行测试并确认 RED**

Run: `cd web && pnpm --filter @jiangcheng-memory/control-plane-frontend test -- memory-lab-page.test.tsx`

Expected: FAIL，当前页面只有 Observe/Recall/Explain。

- [x] **Step 3: 实现最小表单与结果展示**

复用 Fragment 构造逻辑；Commit 接受一个原始文本 Fragment；Correct/Forget 提供 verification method、source ID、reason 和目标确认。Mutation 成功后继续使用 Canonical JSON 展示服务端真实回执。

- [x] **Step 4: 运行聚焦测试并确认 GREEN**

Run: `cd web && pnpm --filter @jiangcheng-memory/control-plane-frontend test -- memory-lab-page.test.tsx`

Expected: PASS。

### Task 3: E2E、文档与门禁

**Files:**
- Modify: `web/e2e/control-plane.spec.ts`
- Modify: `web/README.zh-CN.md`
- Modify: `docs/superpowers/plans/2026-08-01-web-control-plane-phase1-implementation.md`

- [x] **Step 1: 扩展 Fixture 与主路径断言**

Fixture 只按已有稳定协议回显 Commit/Mutation 回执；E2E 验证确认门禁、无写重试和 API Key 不进入 DOM/响应。

- [x] **Step 2: 更新真实能力文档**

把 Commit、Correct、Forget 标记为 Web LIVE，并写明 Forget 是追加失效记录，不是 Hard Delete；Runtime、Governance、Recovery 和 Managed Sandbox 保持真实状态。

- [x] **Step 3: 运行完整门禁**

Run:

```bash
cd web
pnpm test
pnpm typecheck
pnpm build
pnpm exec playwright test
cd ..
git diff --check
```

Expected: 全部退出码为 0；外部 Server/Sidecar/MCP 仍明确为未实机验证。

## 规格覆盖复核

- Commit、Correct、Forget 均映射现有稳定 HTTP 契约，不新增 Memory 语义。
- Correct/Forget 追加新 Fragment/Event/EvidenceRef，不覆盖原始记录。
- LLM Prediction、Inference、Plan 没有任何自动提升为 Verified Fact 的路径。
- 页面不提供 Hard Delete、Snapshot、Runtime 或 Managed Sandbox 的伪操作。
- Fixture 只证明版本化契约，不描述为外部平台实机验证。
