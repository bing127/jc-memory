# 疆诚记忆 Web 控制面 Phase 1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在项目根目录 `web/` 中交付可登录、可安全连接真实 `memory-server`、可执行 Stats/Observe/Recall/Explain 的 Phase 1 Web 控制面。

**Architecture:** `web/frontend` 是 React/Vite 单页应用，只消费同源 BFF API；`web/backend` 是 Fastify/TypeScript 服务，使用独立 SQLite WAL 保存控制面元数据、AES-256-GCM 加密实例 API Key，并把经过白名单和 Zod 校验的请求代理到现有 `memory-server`。浏览器、日志和错误响应都不能获得实例 API Key，Memory 正文与响应不写入控制面数据库。

**Tech Stack:** Node.js 22.22+、pnpm 9.15.9、TypeScript 7、Fastify 5、Node SQLite、Zod 4、React 19、Vite 8、TanStack Query、React Hook Form、Radix UI、Tailwind CSS 4、Vitest、Testing Library、Playwright。

---

## 文件结构

```text
web/
  package.json                  # Web 工作区统一命令
  pnpm-workspace.yaml           # frontend/backend 工作区
  .env.example                  # 非敏感配置名和生成说明
  README.zh-CN.md               # 本地启动、边界和真实能力说明
  backend/
    package.json
    tsconfig.json
    src/
      app.ts                    # Fastify 组装、日志脱敏、错误映射
      server.ts                 # 进程入口和优雅关闭
      config.ts                 # 环境变量解析
      auth.ts                   # 密码验证、HMAC Session、Origin 门禁
      crypto.ts                 # AES-256-GCM 凭据加解密
      database.ts               # SQLite WAL 和 schema
      instances.ts              # Instance Registry repository
      upstream.ts               # URL 策略和 memory-server 客户端
      protocol.ts               # 稳定请求/响应 Zod 契约
      routes/session.ts         # 登录、登出、会话
      routes/instances.ts       # 实例 CRUD、Health/Ready
      routes/memory.ts          # Stats/Observe/Recall/Explain 代理
    tests/
      auth.test.ts
      instances.test.ts
      upstream.test.ts
      app.test.ts
  frontend/
    package.json
    tsconfig.json
    vite.config.ts
    index.html
    src/
      main.tsx
      app.tsx                   # 路由和 QueryClient
      index.css                 # 设计 tokens 和响应式布局
      lib/api.ts                # 同源 BFF client
      lib/contracts.ts          # 浏览器可见 DTO，不含 apiKey
      components/ui/*           # shadcn 风格基础控件
      components/app-shell.tsx
      components/status-pill.tsx
      pages/login-page.tsx
      pages/get-started-page.tsx
      pages/overview-page.tsx
      pages/memory-lab-page.tsx
      pages/capabilities-page.tsx
    tests/
      login-page.test.tsx
      overview-page.test.tsx
      memory-lab-page.test.tsx
  e2e/
    control-plane.spec.ts
  playwright.config.ts
```

## Task 1: 建立独立 Web 工作区

**Files:**
- Create: `web/package.json`
- Create: `web/pnpm-workspace.yaml`
- Create: `web/backend/package.json`
- Create: `web/backend/tsconfig.json`
- Create: `web/frontend/package.json`
- Create: `web/frontend/tsconfig.json`
- Create: `web/frontend/vite.config.ts`
- Create: `web/frontend/index.html`

- [x] **Step 1: 写工作区契约失败测试**

创建 `web/tests/workspace.test.mjs`，读取两个 package，断言 Node 版本、`packageManager`、`test/typecheck/build` 命令和 frontend/backend 边界存在。

```js
assert.equal(root.packageManager, "pnpm@9.15.9");
  assert.equal(backend.engines.node, ">=22.22");
assert.equal(frontend.scripts.test, "vitest run");
```

- [x] **Step 2: 运行测试并确认 RED**

Run: `node --test web/tests/workspace.test.mjs`

Expected: FAIL，原因是 `web/package.json` 尚不存在。

- [x] **Step 3: 写最小工作区配置**

根脚本固定为：

```json
{
  "scripts": {
    "dev": "pnpm --parallel --filter './*' dev",
    "test": "pnpm -r test",
    "typecheck": "pnpm -r typecheck",
    "build": "pnpm -r build"
  }
}
```

- [x] **Step 4: 安装并确认 GREEN**

Run: `cd web && corepack pnpm install && node --test tests/workspace.test.mjs`

Expected: PASS，生成 `web/pnpm-lock.yaml`。

## Task 2: 后端配置、凭据加密与 SQLite 边界

**Files:**
- Create: `web/backend/src/config.ts`
- Create: `web/backend/src/crypto.ts`
- Create: `web/backend/src/database.ts`
- Create: `web/backend/src/instances.ts`
- Create: `web/backend/tests/instances.test.ts`

- [x] **Step 1: 写凭据和持久化失败测试**

```ts
test("数据库只保存密文且删除连接不调用实例删除", () => {
  const store = createInstanceStore(tempDatabase(), key);
  const created = store.create({ name: "local", baseUrl: "http://127.0.0.1:8787", namespaceId, apiKey: "secret" });
  expect(store.list()).toEqual([{ ...created, apiKey: undefined }]);
  expect(readDatabaseBytes()).not.toContain("secret");
  store.remove(created.id);
  expect(store.list()).toEqual([]);
});
```

- [x] **Step 2: 运行测试并确认 RED**

Run: `cd web && pnpm --filter @jiangcheng-memory/control-plane-backend test -- instances.test.ts`

Expected: FAIL，原因是 `createInstanceStore` 尚不存在。

- [x] **Step 3: 实现最小加密 Registry**

`crypto.ts` 使用随机 12 字节 nonce 和 AES-256-GCM，数据库只保存 `nonce/ciphertext/tag`；`database.ts` 使用 Node SQLite 创建 WAL 数据库和 `instances` 表；repository 的公开 DTO 固定为：

```ts
export interface InstanceSummary {
  id: string;
  name: string;
  baseUrl: string;
  namespaceId: string;
  protocolVersion: "v1";
  createdAt: string;
  updatedAt: string;
}
```

- [x] **Step 4: 确认 GREEN**

Run: `cd web && pnpm --filter @jiangcheng-memory/control-plane-backend test -- instances.test.ts`

Expected: PASS，且测试数据库中不存在 API Key 明文。

## Task 3: 登录 Session 与请求来源门禁

**Files:**
- Create: `web/backend/src/auth.ts`
- Create: `web/backend/src/routes/session.ts`
- Create: `web/backend/tests/auth.test.ts`

- [x] **Step 1: 写认证失败测试**

```ts
test("无效密码不建立会话，Cookie 不包含密码", async () => {
  const app = await buildTestApp();
  const denied = await app.inject({ method: "POST", url: "/api/v1/session", payload: { password: "wrong" } });
  expect(denied.statusCode).toBe(401);
  const accepted = await login(app, "correct-password");
  expect(accepted.headers["set-cookie"]).toContain("HttpOnly");
  expect(accepted.headers["set-cookie"]).not.toContain("correct-password");
});
```

- [x] **Step 2: 运行测试并确认 RED**

Run: `cd web && pnpm --filter @jiangcheng-memory/control-plane-backend test -- auth.test.ts`

Expected: FAIL，原因是 Session route 尚不存在。

- [x] **Step 3: 实现 HMAC Session**

Session payload 只包含 `subject/issuedAt/expiresAt/nonce`，使用 SHA-256 HMAC 签名；密码通过 `timingSafeEqual` 比较派生的 scrypt 结果；写请求必须是允许的 Origin，Cookie 使用 `HttpOnly`、`SameSite=Strict`，生产环境使用 `Secure`。

- [x] **Step 4: 确认 GREEN**

Run: `cd web && pnpm --filter @jiangcheng-memory/control-plane-backend test -- auth.test.ts`

Expected: PASS。

## Task 4: memory-server 上游策略与错误映射

**Files:**
- Create: `web/backend/src/protocol.ts`
- Create: `web/backend/src/upstream.ts`
- Create: `web/backend/tests/upstream.test.ts`

- [x] **Step 1: 写 URL、凭据和重试失败测试**

```ts
test("仅允许 HTTPS 或 loopback HTTP，写请求不自动重试", async () => {
  expect(() => validateBaseUrl("http://memory.example.com")).toThrow("HTTPS");
  expect(validateBaseUrl("http://127.0.0.1:8787").href).toBe("http://127.0.0.1:8787/");
  await client.execute(instance, observeEnvelope);
  expect(fetchCalls).toHaveLength(1);
  expect(fetchCalls[0].headers.get("x-api-key")).toBe("secret");
  expect(JSON.stringify(publicError)).not.toContain("secret");
});
```

- [x] **Step 2: 运行测试并确认 RED**

Run: `cd web && pnpm --filter @jiangcheng-memory/control-plane-backend test -- upstream.test.ts`

Expected: FAIL，原因是 `validateBaseUrl` 和 upstream client 尚不存在。

- [x] **Step 3: 实现白名单代理**

只允许 `stats/observe/recall/explain/correct/forget/commit` 映射到固定 `/v1/memory/<operation>`；Health、Ready 和 Metrics 使用固定 GET 路径；30 秒 AbortSignal timeout；错误只返回稳定 `code/message/requestId/status`。

- [x] **Step 4: 确认 GREEN**

Run: `cd web && pnpm --filter @jiangcheng-memory/control-plane-backend test -- upstream.test.ts`

Expected: PASS。

## Task 5: 组装 BFF API

**Files:**
- Create: `web/backend/src/app.ts`
- Create: `web/backend/src/server.ts`
- Create: `web/backend/src/routes/instances.ts`
- Create: `web/backend/src/routes/memory.ts`
- Create: `web/backend/tests/app.test.ts`

- [x] **Step 1: 写端到端 API 失败测试**

```ts
test("登录后可连接真实协议 Fixture，浏览器响应没有 apiKey", async () => {
  const cookie = await login(app);
  const created = await app.inject({ method: "POST", url: "/api/v1/instances", headers: { cookie }, payload: instanceInput });
  const stats = await app.inject({ method: "POST", url: `/api/v1/instances/${created.json().id}/memory/stats`, headers: { cookie }, payload: { accessScope } });
  expect(stats.statusCode).toBe(200);
  expect(stats.json().response.payload.fragment_count).toBe(1);
  expect(JSON.stringify(stats.json())).not.toContain(instanceInput.apiKey);
});
```

- [x] **Step 2: 运行测试并确认 RED**

Run: `cd web && pnpm --filter @jiangcheng-memory/control-plane-backend test -- app.test.ts`

Expected: FAIL，原因是 routes 尚未注册。

- [x] **Step 3: 实现 Route 边界**

公开路由只有 `/healthz` 和 Session 登录；其他 `/api/v1/*` 必须认证。实例创建使用 Zod 严格校验；命令由后端补齐 `api_version/request_id/access_scope/command`，浏览器不提交任意上游 URL、Header 或 API Key。

- [x] **Step 4: 确认 GREEN**

Run: `cd web && pnpm --filter @jiangcheng-memory/control-plane-backend test`

Expected: 全部 backend tests PASS。

## Task 6: 前端设计系统、登录与路由门禁

**Files:**
- Create: `web/frontend/src/index.css`
- Create: `web/frontend/src/lib/api.ts`
- Create: `web/frontend/src/lib/contracts.ts`
- Create: `web/frontend/src/components/ui/button.tsx`
- Create: `web/frontend/src/components/ui/input.tsx`
- Create: `web/frontend/src/components/ui/select.tsx`
- Create: `web/frontend/src/components/ui/tabs.tsx`
- Create: `web/frontend/src/app.tsx`
- Create: `web/frontend/src/pages/login-page.tsx`
- Create: `web/frontend/tests/login-page.test.tsx`

- [x] **Step 1: 写登录页面失败测试**

```tsx
test("未登录时只展示登录入口，密码不会写入 URL", async () => {
  render(<App />);
  await user.type(screen.getByLabelText("管理员密码"), "secret");
  await user.click(screen.getByRole("button", { name: "进入控制台" }));
  expect(location.search).not.toContain("secret");
});
```

- [x] **Step 2: 运行测试并确认 RED**

Run: `cd web && pnpm --filter @jiangcheng-memory/control-plane-frontend test -- login-page.test.tsx`

Expected: FAIL，原因是 App/LoginPage 尚不存在。

- [x] **Step 3: 实现登录和稳定布局**

配色使用黑白、中性灰、信号绿、风险红和少量暖黄；无渐变装饰；面板圆角不超过 6px；桌面侧栏和移动端顶部导航不能覆盖正文。

- [x] **Step 4: 确认 GREEN**

Run: `cd web && pnpm --filter @jiangcheng-memory/control-plane-frontend test -- login-page.test.tsx`

Expected: PASS。

## Task 7: Get Started、Instance Registry 与 Overview

**Files:**
- Create: `web/frontend/src/components/app-shell.tsx`
- Create: `web/frontend/src/components/status-pill.tsx`
- Create: `web/frontend/src/pages/get-started-page.tsx`
- Create: `web/frontend/src/pages/overview-page.tsx`
- Create: `web/frontend/tests/overview-page.test.tsx`

- [x] **Step 1: 写真实状态渲染失败测试**

```tsx
test("Overview 只显示后端实际返回的 Ready 和 Stats", async () => {
  renderOverview({ health: "ok", ready: "ready", fragmentCount: 4, eventCount: 5 });
  expect(await screen.findByText("4")).toBeVisible();
  expect(screen.queryByText(/P95|CPU|成本/)).not.toBeInTheDocument();
});
```

- [x] **Step 2: 运行测试并确认 RED**

Run: `cd web && pnpm --filter @jiangcheng-memory/control-plane-frontend test -- overview-page.test.tsx`

Expected: FAIL，原因是 Overview 尚不存在。

- [x] **Step 3: 实现实例向导和 Overview**

Get Started 只开放“连接现有实例”；Managed Sandbox 显示 `PLATFORM` 且不提供伪按钮。Overview 显示 Health、Ready、`server_ready`、Fragment/Event 数、Namespace、协议版本和最后 Request ID。

- [x] **Step 4: 确认 GREEN**

Run: `cd web && pnpm --filter @jiangcheng-memory/control-plane-frontend test -- overview-page.test.tsx`

Expected: PASS。

## Task 8: Memory Lab 与 Canonical JSON

**Files:**
- Create: `web/frontend/src/pages/memory-lab-page.tsx`
- Create: `web/frontend/src/components/json-viewer.tsx`
- Create: `web/frontend/src/components/memory-packet.tsx`
- Create: `web/frontend/tests/memory-lab-page.test.tsx`

- [x] **Step 1: 写 Packet 语义失败测试**

```tsx
test("Core 未填充字段显示当前调用链未提供", async () => {
  render(<MemoryPacket packet={fixturePacket} />);
  expect(screen.getByText("当前调用链未提供")).toBeVisible();
  expect(screen.queryByText("没有此类记忆")).not.toBeInTheDocument();
  expect(screen.getByText("truncated: false")).toBeVisible();
});
```

- [x] **Step 2: 运行测试并确认 RED**

Run: `cd web && pnpm --filter @jiangcheng-memory/control-plane-frontend test -- memory-lab-page.test.tsx`

Expected: FAIL，原因是 MemoryPacket 尚不存在。

- [x] **Step 3: 实现 Observe/Recall/Explain**

Observe 表单生成 UUIDv7、完整 Fragment 和当前时间；Recall 暴露十种 mode 与五个硬预算；结果提供 Packet、Explain 表格和完整 JSON 三种视图；写请求失败后不自动重试。

- [x] **Step 4: 确认 GREEN**

Run: `cd web && pnpm --filter @jiangcheng-memory/control-plane-frontend test`

Expected: 全部 frontend tests PASS。

## Task 9: 能力状态矩阵与中文运行文档

**Files:**
- Create: `web/frontend/src/pages/capabilities-page.tsx`
- Create: `web/.env.example`
- Create: `web/README.zh-CN.md`

- [x] **Step 1: 写能力状态失败测试**

断言生产导航只有 `LIVE` 能力，Runtime、Governance、Managed Sandbox 分别显示真实 `CORE/ROADMAP/PLATFORM` 状态且没有可执行入口。

- [x] **Step 2: 运行并确认 RED**

Run: `cd web && pnpm --filter @jiangcheng-memory/control-plane-frontend test -- capabilities-page.test.tsx`

Expected: FAIL，能力页尚不存在。

- [x] **Step 3: 实现状态注册表和文档**

状态来源逐项链接到 `services/memory-server/src/lib.rs`、`docs/server.zh-CN.md`、`docs/runtime.zh-CN.md` 与 Web 设计规格；README 写明 Node 22.22+、密钥生成命令、两个进程启动方式和 `http://` 仅允许 loopback。

- [x] **Step 4: 确认 GREEN**

Run: `cd web && pnpm test && pnpm typecheck && pnpm build`

Expected: 全部 PASS。

## Task 10: 浏览器 E2E 与交付门禁

**Files:**
- Create: `web/playwright.config.ts`
- Create: `web/e2e/control-plane.spec.ts`

- [x] **Step 1: 写主路径 E2E**

使用本地版本化 Fixture server，覆盖登录、连接实例、Stats、Observe、Recall、Explain、登出；断言浏览器网络响应和 DOM 均不含 API Key。

- [x] **Step 2: 运行 E2E 并确认 RED**

Run: `cd web && pnpm exec playwright test`

Expected: 初次在缺失页面或行为处 FAIL。

- [x] **Step 3: 完成最小集成与响应式修正**

只修复测试暴露的行为、可访问性和布局问题；不增加未进入能力注册表的后台模块。

- [x] **Step 4: 运行完整门禁**

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

Expected: 全部命令退出码为 0；桌面 1440x900 与移动端 390x844 截图无溢出、遮挡或空白画布。

## 规格覆盖复核

- Phase 1 的登录、Instance Registry、Health/Ready、Stats、Observe、Recall、Explain 和 Canonical JSON 均有对应 Task。
- Commit、Correct、Forget 已由后续写操作闭环计划补齐确认、审计原因和结果展示，见 `2026-08-01-web-control-plane-phase1-mutations.md`。
- Runtime Chain、Governance、Recovery 和 Managed Sandbox 只进入能力矩阵，不进入生产导航。
- 控制面不持久化 Fragment、Event、Evidence、Memory Packet 或 ToolResult 正文。
- 本计划不声称完成外部平台实机验证；浏览器 E2E 使用本地版本化 Fixture server。
