import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

import { InstanceStreamProvider } from "../src/lib/instance-stream.js";
import { PerformancePage } from "../src/pages/performance-page.js";
import {
  appendPerformanceHistory,
  deriveOperationSeries,
  liveTimelineDomain,
  parseLoadTestReport,
  summarizeLoadTestReport,
  type PerformanceHistoryEntry,
} from "../src/lib/performance.js";
import type { PerformanceSnapshot } from "../src/lib/contracts.js";

const instance = {
  id: "instance-1",
  name: "本地记忆实例",
  baseUrl: "http://127.0.0.1:8787/",
  namespaceId: "namespace-1",
  protocolVersion: "v1" as const,
  createdAt: "2026-08-01T00:00:00.000Z",
  updatedAt: "2026-08-01T00:00:00.000Z",
};

function report(overrides: Record<string, unknown> = {}) {
  return {
    schema_version: "jcmem.load-test.v1",
    provenance: { git_commit: "abc123", dirty: false, platform: "darwin-arm64" },
    config: {
      target: "http://127.0.0.1:8787",
      warmup_requests: 10,
      timeout_ms: 30000,
      real_rust_sqlite: true,
    },
    scenarios: [
      {
        name: "recall-c32",
        requests: 100,
        concurrency: 32,
        success: 98,
        failure: 2,
        status_counts: { "200": 98, "429": 2 },
        errors: {},
        throughput_rps: 13.48,
        latency_ms: { p50: 2320.82, p95: 2738.33, p99: null, max: 3200 },
        real_rust_sqlite: true,
      },
    ],
    ...overrides,
  };
}

let instanceSequence = 0;

function renderPage() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const selectedInstance = { ...instance, id: `performance-instance-${++instanceSequence}` };
  return {
    client,
    instance: selectedInstance,
    ...render(
      <QueryClientProvider client={client}>
        <InstanceStreamProvider instanceId={selectedInstance.id}>
          <PerformancePage instance={selectedInstance} />
        </InstanceStreamProvider>
      </QueryClientProvider>,
    ),
  };
}

function snapshot(sampledAt: string, total: number, success: number): PerformanceSnapshot {
  return {
    sampledAt,
    inFlight: 3,
    operations: [{
      operation: "recall",
      total,
      success,
      error: 1,
      rateLimited: 2,
      timeouts: 1,
      latencyMs: { p50: 500, p95: 1000, p99: 2500 },
    }],
  };
}

class FakeWebSocket {
  static readonly CONNECTING = 0;
  static readonly OPEN = 1;
  static readonly CLOSED = 3;
  static readonly instances: FakeWebSocket[] = [];
  readonly sent: string[] = [];
  readyState = FakeWebSocket.CONNECTING;
  readonly url: string;
  private readonly listeners = new Map<string, Set<EventListener>>();

  constructor(url: string | URL) {
    this.url = String(url);
    FakeWebSocket.instances.push(this);
  }

  addEventListener(type: string, listener: EventListener): void {
    const listeners = this.listeners.get(type) ?? new Set<EventListener>();
    listeners.add(listener);
    this.listeners.set(type, listeners);
  }

  send(data: string): void {
    this.sent.push(data);
  }

  close(): void {
    this.readyState = FakeWebSocket.CLOSED;
  }

  emitOpen(): void {
    this.readyState = FakeWebSocket.OPEN;
    this.dispatch("open", new Event("open"));
  }

  emitMessage(message: unknown): void {
    this.dispatch("message", new MessageEvent("message", { data: JSON.stringify(message) }));
  }

  private dispatch(type: string, event: Event): void {
    for (const listener of this.listeners.get(type) ?? []) listener(event);
  }
}

beforeEach(() => {
  vi.stubGlobal("WebSocket", FakeWebSocket);
});

afterEach(() => {
  FakeWebSocket.instances.length = 0;
  vi.unstubAllGlobals();
});

test("导入合法压测报告后显示核心指标和429", async () => {
  const user = userEvent.setup();
  renderPage();

  const input = screen.getByLabelText("导入压测报告");
  const file = new File([JSON.stringify(report())], "load-report.json", {
    type: "application/json",
  });
  await user.upload(input, file);

  expect(await screen.findByText("recall-c32")).not.toBeNull();
  expect(screen.getByText("100")).not.toBeNull();
  expect(screen.getAllByText("98.0%").length).toBeGreaterThan(0);
  expect(screen.getAllByText("13.48 req/s").length).toBeGreaterThan(0);
  expect(screen.getAllByText("2,320.82 ms").length).toBeGreaterThan(0);
  expect(screen.getAllByText("2").length).toBeGreaterThan(0);
  expect(screen.getAllByText("未采集").length).toBeGreaterThan(0);
  expect(screen.getByText("abc123")).not.toBeNull();
});

test("未知报告版本显示中文错误且不渲染报告数据", async () => {
  const user = userEvent.setup();
  renderPage();
  const input = screen.getByLabelText("导入压测报告");
  const file = new File(
    [JSON.stringify({ ...report(), schema_version: "unknown.v2" })],
    "unknown.json",
    { type: "application/json" },
  );
  await user.upload(input, file);

  expect((await screen.findByRole("alert")).textContent).toContain("不支持的压测报告版本");
  expect(screen.queryByText("recall-c32")).toBeNull();
});

test("多场景摘要使用已采集延迟的最高值", () => {
  const first = report().scenarios[0];
  const parsed = parseLoadTestReport(report({
    scenarios: [
      first,
      {
        ...first,
        name: "recall-c64",
        latency_ms: { p50: 4863.74, p95: 5605.15, p99: null, max: 6439.92 },
      },
    ],
  }));

  expect(summarizeLoadTestReport(parsed).latencyMax).toEqual({
    p50: 4863.74,
    p95: 5605.15,
    p99: null,
  });
});

test("报告解析拒绝版本契约之外的字段", () => {
  expect(() => parseLoadTestReport({ ...report(), api_key: "不应进入报告" })).toThrow(
    "压测报告格式不完整或包含无效指标",
  );
});

test("累计快照差分生成RPS且失败样本保留为缺口", () => {
  let history = appendPerformanceHistory([], snapshot("2026-08-01T00:00:00.000Z", 100, 95));
  history = appendPerformanceHistory(history, snapshot("2026-08-01T00:00:10.000Z", 150, 145));
  history = appendPerformanceHistory(history, null, "2026-08-01T00:00:15.000Z");
  history = appendPerformanceHistory(history, null, "2026-08-01T00:00:16.000Z");

  expect(deriveOperationSeries(history, "recall").map((point) => point.rps)).toEqual([
    null,
    5,
    null,
  ]);
  expect(history[2]?.snapshot).toBeNull();
  expect(history).toHaveLength(3);
  expect(liveTimelineDomain(deriveOperationSeries(history, "recall"))).toEqual([
    Date.parse("2026-08-01T00:00:00.000Z"),
    Date.parse("2026-08-01T00:00:59.000Z"),
  ]);
});

test("实时序列使用递增时间戳并在六十秒后滚动窗口", () => {
  const history = [
    snapshot("2026-08-01T00:00:00.000Z", 100, 95),
    snapshot("2026-08-01T00:00:01.000Z", 105, 100),
    snapshot("2026-08-01T00:01:10.000Z", 150, 145),
  ].reduce<PerformanceHistoryEntry[]>(
    (current, item) => appendPerformanceHistory(current, item),
    [],
  );
  const series = deriveOperationSeries(history, "recall");

  expect(series.map((point) => point.timestamp)).toEqual([
    Date.parse("2026-08-01T00:00:00.000Z"),
    Date.parse("2026-08-01T00:00:01.000Z"),
    Date.parse("2026-08-01T00:01:10.000Z"),
  ]);
  expect(liveTimelineDomain(series)).toEqual([
    Date.parse("2026-08-01T00:00:11.000Z"),
    Date.parse("2026-08-01T00:01:10.000Z"),
  ]);
});

test("静态报告不订阅 metrics Topic", async () => {
  renderPage();
  const socket = FakeWebSocket.instances[0];
  await act(async () => {
    socket?.emitOpen();
    await Promise.resolve();
  });

  expect(socket?.sent.map(parseMessage)).toEqual([{
    version: 1,
    type: "subscribe",
    subscriptions: [],
  }]);
});

test("连续指标错误只记录一个缺口且恢复前后保持单连接和稳定状态槽", async () => {
  const user = userEvent.setup();
  renderPage();
  const socket = FakeWebSocket.instances[0];
  act(() => socket?.emitOpen());

  await user.click(screen.getByRole("tab", { name: "实时监控" }));
  await flushEffects();
  expect(socket?.sent.map(parseMessage).at(-1)).toEqual({
    version: 1,
    type: "subscribe",
    subscriptions: [{ topic: "metrics" }],
  });
  const statusSlot = screen.getByTestId("metrics-channel-status");
  expect(statusSlot.classList.contains("is-empty")).toBe(true);

  act(() => socket?.emitMessage(streamSnapshot(snapshot("2026-08-01T00:00:00.000Z", 100, 95))));
  expect(await screen.findByText("95.0%")).not.toBeNull();
  act(() => socket?.emitMessage(streamSnapshot(snapshot("2026-08-01T00:00:01.000Z", 105, 100))));
  expect(await screen.findByText("5 req/s")).not.toBeNull();
  expect(screen.getByLabelText("样本 2 / 60")).not.toBeNull();

  act(() => {
    socket?.emitMessage(channelError("2026-08-01T00:00:02.000Z"));
    socket?.emitMessage(channelError("2026-08-01T00:00:03.000Z"));
  });

  expect(screen.getByText("实时流已连接")).not.toBeNull();
  expect(screen.getByText("指标采样失败，图表保留最后成功数据且缺口不会补零。")).not.toBeNull();
  expect(screen.getByText("采样缺口 1")).not.toBeNull();
  expect(statusSlot.classList.contains("is-empty")).toBe(false);
  expect(FakeWebSocket.instances).toHaveLength(1);

  act(() => socket?.emitMessage(streamSnapshot(snapshot("2026-08-01T00:00:04.000Z", 120, 115))));
  expect(statusSlot.classList.contains("is-empty")).toBe(true);
  expect(screen.getByText("采样缺口 1")).not.toBeNull();
  expect(screen.getByLabelText("样本 4 / 60")).not.toBeNull();
  expect(FakeWebSocket.instances).toHaveLength(1);
});

function streamSnapshot(data: PerformanceSnapshot) {
  return {
    version: 1,
    type: "snapshot",
    topic: "metrics",
    sampledAt: data.sampledAt,
    data,
  };
}

function channelError(sampledAt: string) {
  return {
    version: 1,
    type: "channel_error",
    topic: "metrics",
    sampledAt,
    code: "upstream_unavailable",
    message: "实例指标采样失败",
  };
}

function parseMessage(value: string): unknown {
  return JSON.parse(value);
}

async function flushEffects(): Promise<void> {
  await act(async () => {
    await Promise.resolve();
  });
}
