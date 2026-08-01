# 统一实例 WebSocket 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 用一条实例级 WebSocket 替换总览、Runtime 和性能页的前端周期轮询，并消除频道失败导致的性能页持续闪动。

**Architecture:** `AppShell` 内的 `InstanceStreamProvider` 按当前实例维持唯一连接，页面只声明订阅需求。BFF 在 `/api/v1/instances/:instanceId/stream` 上接收完整订阅集合，按 Topic 启停独立采样任务，并将传输错误、频道错误和协议错误分开处理。

**Tech Stack:** Node.js 22、Fastify 5、`@fastify/websocket`、`ws`、React 19、TypeScript、Zod、TanStack Query、Vitest、Testing Library、Playwright。

---

## 文件结构

- Create: `web/backend/src/instance-stream.ts`：订阅协议、频道采样器、任务生命周期和消息发送。
- Modify: `web/backend/src/routes/instances.ts`：注册统一实例 WebSocket，删除旧指标专用流。
- Modify: `web/backend/src/routes/memory.ts`：导出构造 Stats Scope 所需的纯函数，避免复制 Scope 绑定规则。
- Create: `web/backend/tests/instance-stream.test.ts`：统一连接、订阅幂等、错误去重、刷新和清理契约。
- Modify: `web/backend/tests/app.test.ts`：移除旧指标流测试，保留 HTTP 诊断接口覆盖。
- Modify: `web/frontend/src/lib/contracts.ts`：统一流协议、总览统计和频道数据类型。
- Create: `web/frontend/src/lib/instance-stream-client.ts`：实例连接注册表、原生 WebSocket、重连和协议状态。
- Create: `web/frontend/src/lib/instance-stream.tsx`：Provider、订阅合并和频道 Hooks。
- Create: `web/frontend/tests/instance-stream.test.tsx`：单连接、路由订阅和频道错误状态测试。
- Modify: `web/frontend/src/components/app-shell.tsx`：在当前实例页面外层挂载 Provider。
- Modify: `web/frontend/src/pages/overview-page.tsx`：订阅 `overview`，删除 15 秒轮询。
- Modify: `web/frontend/src/pages/runtime-page.tsx`：订阅 Runtime 频道，删除 5/10 秒轮询并接入 `refresh`。
- Modify: `web/frontend/src/pages/performance-page.tsx`：使用共享指标频道，分离传输/采样状态并稳定错误区域。
- Modify: `web/frontend/src/lib/performance.ts`：连续 gap 去重。
- Modify: `web/frontend/tests/overview-page.test.tsx`：总览频道快照与零轮询测试。
- Modify: `web/frontend/tests/runtime-page.test.tsx`：Runtime 订阅、Run 切换和写后刷新测试。
- Modify: `web/frontend/tests/performance-page.test.tsx`：频道错误、恢复和无闪动测试。
- Modify: `web/frontend/src/index.css`：实时错误区域稳定尺寸。
- Modify: `web/e2e/control-plane.spec.ts`：单连接、跨页订阅、零周期 GET 和图表稳定性验收。
- Modify: `web/e2e/fixture-memory-server.mjs`：为各频道提供递增 Fixture，并支持指标失败/恢复场景。
- Modify: `docs/superpowers/specs/2026-08-02-unified-instance-websocket-design.md`：记录已确认状态和 `refresh` 协议。
- Modify: `docs/superpowers/plans/2026-08-02-unified-instance-websocket-implementation.md`：逐项勾选实际完成步骤。

### Task 1: BFF 统一订阅协议与频道调度

**Files:**
- Create: `web/backend/src/instance-stream.ts`
- Modify: `web/backend/src/routes/instances.ts`
- Modify: `web/backend/src/routes/memory.ts`
- Create: `web/backend/tests/instance-stream.test.ts`
- Modify: `web/backend/tests/app.test.ts`

- [ ] **Step 1: 写多 Topic 单连接红灯测试**

在 `instance-stream.test.ts` 启动真实 Fastify TCP 服务，登录并创建实例，使用 `ws` 连接统一地址后发送：

```ts
socket.send(JSON.stringify({
  version: 1,
  type: "subscribe",
  subscriptions: [
    { topic: "overview" },
    { topic: "metrics" },
    { topic: "runtime.list" },
    { topic: "runtime.detail", runId },
  ],
}));
```

收集首轮消息并断言 Topic 集合等于：

```ts
expect(new Set(messages.map((message) => message.topic))).toEqual(new Set([
  "overview",
  "metrics",
  "runtime.list",
  "runtime.detail",
]));
expect(messages.every((message) => message.version === 1)).toBe(true);
```

- [ ] **Step 2: 运行后端红灯测试**

Run: `cd web/backend && corepack pnpm test -- instance-stream.test.ts`

Expected: FAIL，统一 `/stream` 路由返回 `404`。

- [ ] **Step 3: 实现版本化协议解析和频道采样器**

在 `instance-stream.ts` 定义严格 Zod Schema：

```ts
const subscriptionSchema = z.discriminatedUnion("topic", [
  z.strictObject({ topic: z.literal("overview") }),
  z.strictObject({ topic: z.literal("metrics") }),
  z.strictObject({ topic: z.literal("runtime.list") }),
  z.strictObject({ topic: z.literal("runtime.detail"), runId: uuidStringSchema }),
]);

const clientMessageSchema = z.discriminatedUnion("type", [
  z.strictObject({
    version: z.literal(1),
    type: z.literal("subscribe"),
    subscriptions: z.array(subscriptionSchema).max(4),
  }),
  z.strictObject({
    version: z.literal(1),
    type: z.literal("refresh"),
    topics: z.array(z.enum(["overview", "metrics", "runtime.list", "runtime.detail"])).min(1).max(4),
  }),
]);
```

每个任务使用稳定 key：普通 Topic 直接使用名称，详情使用 `runtime.detail:${runId}`。任务保存 `timer`、`sampling`、`closed` 和 `failed`；采样锁阻止重叠请求，停止任务时设置 `closed` 并清理定时器。

频道采样返回使用现有后端类型：

```ts
import type { PerformanceSnapshot } from "./metrics.js";
import type { MemoryResponseEnvelope } from "./protocol.js";

interface OverviewStatus {
  health: unknown;
  ready: unknown;
  serverReady: boolean;
}

type StreamData =
  | { topic: "overview"; data: { status: OverviewStatus; stats: MemoryResponseEnvelope } }
  | { topic: "metrics"; data: PerformanceSnapshot }
  | { topic: "runtime.list"; data: unknown }
  | { topic: "runtime.detail"; data: unknown };
```

Stats 调用必须复用 `toAccessScope`，并通过以下输入获取权威响应：

```ts
await upstream.execute(connection, {
  operation: "stats",
  requestId: randomUUID(),
  accessScope: toAccessScope(connection.namespaceId, {
    tenantId: null,
    userId: null,
    agentId: null,
    projectId: null,
  }),
  payload: {},
});
```

- [ ] **Step 4: 注册统一路由并删除旧指标流**

在 `instances.ts` 注册：

```ts
app.get(
  "/api/v1/instances/:id/stream",
  { websocket: true },
  (socket, request) => {
    if (!auth.sessionFromRequest(request) || !auth.isAllowedOrigin(request.headers.origin)) {
      socket.close(1008, "unauthorized");
      return;
    }
    const parsed = instanceParamsSchema.safeParse(request.params);
    const connection = parsed.success ? store.getConnection(parsed.data.id) : null;
    if (!connection) {
      socket.close(1008, "instance_not_found");
      return;
    }
    attachInstanceStream(socket, connection, upstream);
  },
);
```

删除 `/api/v1/instances/:id/metrics/stream` 和 `METRICS_STREAM_INTERVAL_MS`，HTTP `/metrics` 诊断接口保留。

- [ ] **Step 5: 补订阅幂等、刷新、错误去重和清理红灯测试**

增加以下独立用例和核心断言：

```ts
expect(metricsCallsAfterDuplicateSubscribe).toBe(metricsCallsBeforeDuplicateSubscribe);
expect(requestedRuntimeDetailUrls).toEqual([oldRunUrl, newRunUrl]);
expect(received.filter((message) => message.type === "channel_error")).toHaveLength(1);
expect(received.at(-1)).toMatchObject({ type: "snapshot", topic: "metrics" });
expect(received).toContainEqual(expect.objectContaining({
  type: "protocol_error",
  code: "topic_not_subscribed",
}));
expect(upstreamCallsAfterClose).toBe(upstreamCallsBeforeClose);
expect(receivedAfterInvalidSubscription).toContainEqual(expect.objectContaining({
  type: "snapshot",
  topic: "overview",
}));
```

- [ ] **Step 6: 运行后端聚焦测试并重构**

Run: `cd web/backend && corepack pnpm test -- instance-stream.test.ts app.test.ts`

Expected: PASS，所有 WebSocket 在 `afterEach` 中关闭，测试服务器无悬挂句柄。

- [ ] **Step 7: 提交后端统一流**

```bash
git add web/backend/src/instance-stream.ts web/backend/src/routes/instances.ts web/backend/src/routes/memory.ts web/backend/tests/instance-stream.test.ts web/backend/tests/app.test.ts
git commit -m "feat(web): 增加统一实例订阅流" -m "BFF 通过单条 WebSocket 按 Topic 调度总览、指标和 Runtime 采样，支持幂等订阅、即时刷新、错误去重与任务清理。"
```

### Task 2: 前端实例连接 Provider

**Files:**
- Modify: `web/frontend/src/lib/contracts.ts`
- Create: `web/frontend/src/lib/instance-stream-client.ts`
- Create: `web/frontend/src/lib/instance-stream.tsx`
- Create: `web/frontend/tests/instance-stream.test.tsx`
- Modify: `web/frontend/src/components/app-shell.tsx`

- [ ] **Step 1: 写单连接和订阅合并红灯测试**

在 React `StrictMode` 中使用可控 `FakeWebSocket` 渲染 Provider 和两个测试消费者：

```tsx
<StrictMode>
  <InstanceStreamProvider instanceId="instance-1">
    <TopicConsumer subscription={{ topic: "overview" }} />
    <TopicConsumer subscription={{ topic: "metrics" }} />
  </InstanceStreamProvider>
</StrictMode>
```

断言只创建一个连接，打开后只发送一条完整订阅消息：

```ts
expect(FakeWebSocket.instances).toHaveLength(1);
socket.emitOpen();
expect(socket.sent.map(JSON.parse)).toEqual([{
  version: 1,
  type: "subscribe",
  subscriptions: [{ topic: "metrics" }, { topic: "overview" }],
}]);
```

卸载一个消费者后断言仍复用原连接并发送缩减后的完整集合。

- [ ] **Step 2: 运行 Provider 红灯测试**

Run: `cd web/frontend && corepack pnpm test -- instance-stream.test.tsx`

Expected: FAIL，`InstanceStreamProvider` 和订阅 Hook 尚不存在。

- [ ] **Step 3: 实现协议类型、Provider 和 Hooks**

在 `contracts.ts` 增加：

```ts
export type InstanceStreamSubscription =
  | { topic: "overview" }
  | { topic: "metrics" }
  | { topic: "runtime.list" }
  | { topic: "runtime.detail"; runId: string };

export type InstanceStreamTopic = InstanceStreamSubscription["topic"];
export type InstanceTransportState = "connecting" | "connected" | "disconnected";
```

`instance-stream.tsx` 提供：

```ts
export function InstanceStreamProvider(props: { instanceId: string | null; children: ReactNode }): JSX.Element;
export function useInstanceStreamSubscription(subscription: InstanceStreamSubscription | null): void;
export function useInstanceStreamTopic<T>(topic: InstanceStreamTopic): { data: T | null; sampledAt: string | null; error: string | null };
export function useInstanceTransportState(): InstanceTransportState;
export function useInstanceStreamRefresh(): (topics: InstanceStreamTopic[]) => void;
```

`instance-stream-client.ts` 使用模块级注册表，以 `instanceId` 为 key 保存客户端、引用计数和待释放定时器。`acquire` 取消同实例的待释放任务；`release` 在 `setTimeout(disposeIfUnused, 0)` 到期且引用仍为零时关闭客户端。WebSocket URL 使用当前页面协议映射 `ws/wss`，路径为统一 `/stream`。

Provider 用消费者 token 到订阅的 Map 合并需求；在微任务中统一计算规范化排序后的订阅集合，签名未变化时不得重复发送。这样路由 effect 的清理和挂载只产生最终集合，不发送中间空订阅。

`useInstanceStreamSubscription` 使用 Topic 与 `runId` 组成的稳定字符串作为 effect 依赖，不依赖调用方每次渲染产生的新对象身份。

当 `runtime.detail` 的订阅 key 从旧 `runId` 切换为新 `runId` 时，Provider 立即清除该 Topic 的旧 `data`、`sampledAt` 和 `error`，避免新快照到达前展示上一个 Run。

- [ ] **Step 4: 补频道状态和重连红灯测试**

增加用例并使用以下核心断言证明：

```ts
expect(result.current.transportState).toBe("connected");
expect(result.current.metrics.data).toEqual(lastSnapshot);
expect(result.current.metrics.error).toBe("实例指标采样失败");
expect(FakeWebSocket.instances).toHaveLength(2);
expect(FakeWebSocket.instances[1]?.sent.map(JSON.parse)).toContainEqual(expectedSubscribe);
expect(socket.sent.map(JSON.parse)).toContainEqual({
  version: 1,
  type: "refresh",
  topics: ["runtime.list"],
});
```

另增加 StrictMode 和同轮订阅切换断言：

```ts
expect(FakeWebSocket.instances).toHaveLength(1);
expect(socket.sent.map(JSON.parse)).not.toContainEqual({
  version: 1,
  type: "subscribe",
  subscriptions: [],
});
expect(socket.sent.map(JSON.parse).at(-1)).toEqual(expectedRouteSubscriptions);
```

- [ ] **Step 5: 在 AppShell 挂载 Provider**

将当前实例 ID 传给 Provider，并让整个实例内容区共享连接。保留现有顶部栏结构：

```tsx
<InstanceStreamProvider instanceId={selected?.id ?? null}>
  <div className="console-main">
    <header className="console-topbar">
      <div>
        <Server aria-hidden="true" />
        <span>
          <small>当前实例</small>
          <strong>{selected?.name ?? "尚未连接实例"}</strong>
        </span>
      </div>
      {selected ? (
        <div className="console-connection">
          <span aria-hidden="true" />REST {selected.protocolVersion}
        </div>
      ) : null}
    </header>
    {children}
  </div>
</InstanceStreamProvider>
```

Provider 不因 `children` 路由变化重建；只有实例 ID 改变或退出实例区域时关闭旧连接。

- [ ] **Step 6: 运行前端聚焦测试并提交**

Run: `cd web/frontend && corepack pnpm test -- instance-stream.test.tsx`

Run: `cd web/frontend && corepack pnpm typecheck`

Expected: PASS。

```bash
git add web/frontend/src/lib/contracts.ts web/frontend/src/lib/instance-stream-client.ts web/frontend/src/lib/instance-stream.tsx web/frontend/src/components/app-shell.tsx web/frontend/tests/instance-stream.test.tsx
git commit -m "feat(web): 共享实例 WebSocket 连接" -m "AppShell 为当前实例维持唯一连接，合并页面订阅并分离传输状态与频道错误，重连后自动恢复订阅。"
```

### Task 3: 总览页迁移

**Files:**
- Modify: `web/frontend/src/pages/overview-page.tsx`
- Modify: `web/frontend/tests/overview-page.test.tsx`

- [ ] **Step 1: 写总览零轮询红灯测试**

在 Provider 测试壳中渲染 `OverviewPage`，打开 WebSocket 后推送：

```ts
socket.emitMessage({
  version: 1,
  type: "snapshot",
  topic: "overview",
  sampledAt: "2026-08-02T00:00:00.000Z",
  data: {
    status: { health: { status: "ok" }, ready: { status: "ready" }, serverReady: true },
    stats: { request_id: "request-1", response: { payload: { fragment_count: 3, event_count: 5 } } },
  },
});
```

断言显示 `3`、`5`、三个正常状态，并在推进 30 秒假时钟后 `fetch` 调用数仍为零。

- [ ] **Step 2: 运行红灯测试**

Run: `cd web/frontend && corepack pnpm test -- overview-page.test.tsx`

Expected: FAIL，页面仍调用 HTTP Query。

- [ ] **Step 3: 替换为 overview 频道**

删除 `useQuery`、`api.instanceStatus`、`api.memory(stats)` 和 `emptyScope`。页面调用：

```ts
useInstanceStreamSubscription({ topic: "overview" });
const overview = useInstanceStreamTopic<OverviewSnapshot>("overview");
```

使用 `overview.data.status` 与 `overview.data.stats` 渲染原 UI。频道错误时保留最后数据，并在固定错误区域显示“无法刷新实例状态”。

- [ ] **Step 4: 运行测试并提交**

Run: `cd web/frontend && corepack pnpm test -- overview-page.test.tsx instance-stream.test.tsx`

Expected: PASS。

```bash
git add web/frontend/src/pages/overview-page.tsx web/frontend/tests/overview-page.test.tsx
git commit -m "refactor(web): 总览改用实例订阅流" -m "总览状态和权威统计通过 overview 频道更新，移除两个十五秒前端轮询并保留最后成功数据。"
```

### Task 4: Runtime 页迁移与写后刷新

**Files:**
- Modify: `web/frontend/src/pages/runtime-page.tsx`
- Modify: `web/frontend/tests/runtime-page.test.tsx`

- [ ] **Step 1: 写 Runtime 订阅红灯测试**

渲染页面后断言订阅集合先包含 `runtime.list`；选择 Run 后包含：

```ts
[
  { topic: "runtime.detail", runId },
  { topic: "runtime.list" },
]
```

推进 20 秒假时钟后断言没有 Runtime GET `fetch`。推送列表和详情快照，断言原列表、状态、Transition 和 JSON 均正常显示。

- [ ] **Step 2: 写 Runtime Start 刷新红灯测试**

HTTP POST 成功返回 `newRunId` 后断言：

```ts
expect(location.search).toContain(`run=${newRunId}`);
expect(socket.sent.map(JSON.parse)).toContainEqual({
  version: 1,
  type: "refresh",
  topics: ["runtime.list"],
});
```

并断言随后订阅切换到 `runtime.detail:newRunId`。

- [ ] **Step 3: 运行红灯测试**

Run: `cd web/frontend && corepack pnpm test -- runtime-page.test.tsx`

Expected: FAIL，页面仍包含 `refetchInterval`，且没有订阅或刷新消息。

- [ ] **Step 4: 替换 Runtime 读取链路**

保留 `useMutation` 处理 Runtime Start，删除两个 `useQuery` 和 QueryClient invalidation。增加：

```ts
useInstanceStreamSubscription({ topic: "runtime.list" });
useInstanceStreamSubscription(selectedRunId
  ? { topic: "runtime.detail", runId: selectedRunId }
  : null);
const runs = useInstanceStreamTopic<RuntimeRunPage>("runtime.list");
const detail = useInstanceStreamTopic<RuntimeRunDetail>("runtime.detail");
const refresh = useInstanceStreamRefresh();
```

mutation 成功时先设置 `run` 查询参数，再调用 `refresh(["runtime.list"])`。删除“每 10 秒刷新”文案，替换为“实时订阅”。

- [ ] **Step 5: 运行测试并提交**

Run: `cd web/frontend && corepack pnpm test -- runtime-page.test.tsx instance-stream.test.tsx`

Expected: PASS。

```bash
git add web/frontend/src/pages/runtime-page.tsx web/frontend/tests/runtime-page.test.tsx
git commit -m "refactor(web): Runtime 改用实例订阅流" -m "Run 列表和详情通过共享 WebSocket 更新，移除五秒与十秒轮询，并在 Runtime Start 后即时刷新列表和详情订阅。"
```

### Task 5: 性能页无闪动迁移

**Files:**
- Modify: `web/frontend/src/pages/performance-page.tsx`
- Modify: `web/frontend/src/lib/performance.ts`
- Modify: `web/frontend/tests/performance-page.test.tsx`
- Modify: `web/frontend/src/index.css`

- [ ] **Step 1: 写频道错误不触发重连红灯测试**

在共享 Provider 下打开实时 Tab，推送一次指标快照后连续推送两次 `channel_error`。断言：

```ts
expect(screen.getByText("实时流已连接")).not.toBeNull();
expect(screen.getByText("指标采样失败")).not.toBeNull();
expect(screen.getByText("采样缺口 1")).not.toBeNull();
expect(FakeWebSocket.instances).toHaveLength(1);
```

再推送恢复快照，断言错误隐藏、历史继续且连接数不变。

- [ ] **Step 2: 写布局稳定红灯测试**

错误槽始终存在并带稳定测试标识：

```ts
const slot = screen.getByTestId("metrics-channel-status");
expect(slot.classList.contains("is-empty")).toBe(true);
socket.emitChannelError("metrics");
expect(slot.classList.contains("is-empty")).toBe(false);
```

测试不比较 JSDOM 几何尺寸；Playwright 再比较错误前后 `.live-monitor` 顶部与图表顶部坐标保持不变。

- [ ] **Step 3: 运行红灯测试**

Run: `cd web/frontend && corepack pnpm test -- performance-page.test.tsx`

Expected: FAIL，性能页仍自己创建 WebSocket，并把 gap 设置为传输错误。

- [ ] **Step 4: 使用共享 metrics 频道并去重 gap**

删除性能页中的 `new WebSocket`、重连定时器和 `MetricsStreamMessage`。实时 Tab 订阅：

```ts
useInstanceStreamSubscription(tab === "live" ? { topic: "metrics" } : null);
const transportState = useInstanceTransportState();
const metrics = useInstanceStreamTopic<PerformanceSnapshot>("metrics");
```

使用 `sampledAt` Ref 保证同一快照只追加一次；使用错误状态 Ref 保证同一连续失败区间只追加一个 gap。收到下一次快照后清除失败区间标记。

错误槽始终渲染：

```tsx
<div
  className={`live-channel-status ${metrics.error ? "" : "is-empty"}`}
  data-testid="metrics-channel-status"
  aria-live="polite"
>
  {metrics.error ? <div className="error-band">指标采样失败，图表保留最后成功数据且缺口不会补零。</div> : null}
</div>
```

CSS 为 `.live-channel-status` 设置稳定 `min-height`，空状态使用同等占位，不让后续指标卡和图表上下移动。

- [ ] **Step 5: 运行聚焦测试并提交**

Run: `cd web/frontend && corepack pnpm test -- performance-page.test.tsx instance-stream.test.tsx`

Expected: PASS。

```bash
git add web/frontend/src/pages/performance-page.tsx web/frontend/src/lib/performance.ts web/frontend/tests/performance-page.test.tsx web/frontend/src/index.css
git commit -m "fix(web): 消除实时性能页面闪动" -m "性能页复用实例连接并分离传输与指标错误，连续失败只记录一个缺口，固定状态区域避免内容反复位移。"
```

### Task 6: 端到端单连接与零轮询验收

**Files:**
- Modify: `web/e2e/fixture-memory-server.mjs`
- Modify: `web/e2e/control-plane.spec.ts`

- [ ] **Step 1: 扩展 Fixture 可观测信号**

Fixture 为 status、Stats、Runtime 和 metrics 维护独立请求计数。增加仅测试环境使用的读取端点：

```text
GET /__fixture/requests
```

返回各上游路径的调用次数，不包含 API Key、请求体或用户输入。该端点只存在于 `fixture-memory-server.mjs`，不进入生产 BFF。

- [ ] **Step 2: 写 Playwright 单连接与页面切换断言**

在页面初始化脚本中包装原生 WebSocket，只记录构造次数和 URL，不读取消息内容：

```ts
await page.addInitScript(() => {
  const NativeWebSocket = window.WebSocket;
  const urls: string[] = [];
  class CountingWebSocket extends NativeWebSocket {
    constructor(url: string | URL, protocols?: string | string[]) {
      super(url, protocols);
      urls.push(String(url));
    }
  }
  Object.defineProperty(window, "WebSocket", { value: CountingWebSocket });
  Object.defineProperty(window, "__instanceStreamUrls", { value: urls });
});
```

登录后依次切换总览、Runtime 和性能页，断言 `/stream` URL 只有一次，并且旧 `/metrics/stream` 从未创建。

- [ ] **Step 3: 写零周期 GET 和布局稳定断言**

记录浏览器请求，等待覆盖 15 秒旧总览周期后断言不存在重复的 status、memory/stats、runtime list 和 runtime detail GET。通过 Fixture 计数确认 BFF 仅按当前订阅 Topic 调用上游。

性能错误/恢复 Fixture 场景中记录 `.live-monitor` 和 `.performance-chart-grid` 的边界坐标，断言错误出现及恢复后图表顶部坐标不变；等待至少两个成功样本并检查 SVG 折线非空且从左向右推进。

- [ ] **Step 4: 运行桌面和移动端 E2E**

Run: `cd web && corepack pnpm test:e2e`

Expected: `desktop-chromium` 与 `mobile-chromium` 全部 PASS，并生成实时性能截图。

- [ ] **Step 5: 提交 E2E 验收**

```bash
git add web/e2e/fixture-memory-server.mjs web/e2e/control-plane.spec.ts
git commit -m "test(web): 验证实例单连接与无轮询" -m "Playwright 覆盖跨页面复用 WebSocket、按 Topic 采样、零周期 GET、指标失败恢复和桌面移动端布局稳定。"
```

### Task 7: 文档、完整门禁与推送

**Files:**
- Modify: `docs/superpowers/specs/2026-08-02-unified-instance-websocket-design.md`
- Modify: `docs/superpowers/plans/2026-08-02-unified-instance-websocket-implementation.md`

- [ ] **Step 1: 检索并移除控制台轮询残留**

Run: `rg -n "refetchInterval|metrics/stream|每 10 秒刷新" web/frontend/src web/backend/src`

Expected: 无总览、Runtime 或性能实时读取残留；`json-viewer` 的复制提示 `setTimeout` 和 Provider 的断线退避不属于周期轮询。

- [ ] **Step 2: 运行 Web 全量测试**

Run: `cd web/backend && corepack pnpm exec vitest run --maxWorkers=1 --no-file-parallelism`

Run: `cd web/frontend && corepack pnpm exec vitest run --maxWorkers=1 --no-file-parallelism`

Expected: 全部测试 PASS。单 worker 避免当前高负载开发机让无关 5 秒用例产生资源竞争超时。

- [ ] **Step 3: 运行类型检查和生产构建**

Run: `cd web && corepack pnpm typecheck`

Run: `cd web && corepack pnpm build`

Expected: 退出码 0；如仍存在既有 Vite 大 chunk 提示，记录但不得描述为构建失败。

- [ ] **Step 4: 运行最终 E2E 与差异检查**

Run: `cd web && corepack pnpm test:e2e`

Run: `git diff --check`

Expected: 桌面和移动端 E2E 通过，差异检查无输出。

- [ ] **Step 5: 勾选完成项并提交文档**

只勾选有实际命令证据的步骤，记录 Fixture 与真实部署边界：

```bash
git add docs/superpowers/specs/2026-08-02-unified-instance-websocket-design.md docs/superpowers/plans/2026-08-02-unified-instance-websocket-implementation.md
git commit -m "docs(web): 记录统一实例流验收" -m "勾选已完成步骤并记录单连接、零轮询、频道错误恢复、桌面与移动端验证结果和实机边界。"
```

- [ ] **Step 6: 推送当前分支**

Run: `git push origin feat/performance-observability`

Expected: 本地 `HEAD` 与 `origin/feat/performance-observability` 一致，工作树干净。
