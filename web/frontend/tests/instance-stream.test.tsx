import { StrictMode, useState } from "react";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

import {
  InstanceStreamProvider,
  useInstanceStreamRefresh,
  useInstanceStreamSubscription,
  useInstanceStreamTopic,
  useInstanceTransportState,
} from "../src/lib/instance-stream.js";

class FakeWebSocket {
  static readonly CONNECTING = 0;
  static readonly OPEN = 1;
  static readonly CLOSING = 2;
  static readonly CLOSED = 3;
  static readonly instances: FakeWebSocket[] = [];

  readonly url: string;
  readonly sent: string[] = [];
  readyState = FakeWebSocket.CONNECTING;
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

  removeEventListener(type: string, listener: EventListener): void {
    this.listeners.get(type)?.delete(listener);
  }

  send(data: string): void {
    this.sent.push(data);
  }

  close(): void {
    if (this.readyState === FakeWebSocket.CLOSED) return;
    this.readyState = FakeWebSocket.CLOSED;
    this.dispatch("close", new CloseEvent("close"));
  }

  emitOpen(): void {
    this.readyState = FakeWebSocket.OPEN;
    this.dispatch("open", new Event("open"));
  }

  emitMessage(message: unknown): void {
    this.dispatch("message", new MessageEvent("message", { data: JSON.stringify(message) }));
  }

  emitClose(): void {
    this.readyState = FakeWebSocket.CLOSED;
    this.dispatch("close", new CloseEvent("close"));
  }

  private dispatch(type: string, event: Event): void {
    for (const listener of this.listeners.get(type) ?? []) listener(event);
  }
}

beforeEach(() => {
  vi.stubGlobal("WebSocket", FakeWebSocket);
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
  FakeWebSocket.instances.length = 0;
});

test("StrictMode 中只创建一条连接并合并页面订阅", async () => {
  const view = render(
    <StrictMode>
      <InstanceStreamProvider instanceId="instance-1">
        <Subscription topic="overview" />
        <Subscription topic="metrics" />
      </InstanceStreamProvider>
    </StrictMode>,
  );

  await flushEffects();
  expect(FakeWebSocket.instances).toHaveLength(1);
  const socket = FakeWebSocket.instances[0];
  act(() => socket?.emitOpen());
  await flushEffects();

  expect(socket?.sent.map(parseMessage)).toEqual([{
    version: 1,
    type: "subscribe",
    subscriptions: [{ topic: "metrics" }, { topic: "overview" }],
  }]);

  view.rerender(
    <StrictMode>
      <InstanceStreamProvider instanceId="instance-1">
        <Subscription topic="metrics" />
      </InstanceStreamProvider>
    </StrictMode>,
  );
  await flushEffects();

  expect(FakeWebSocket.instances).toHaveLength(1);
  expect(socket?.sent.map(parseMessage)).not.toContainEqual({
    version: 1,
    type: "subscribe",
    subscriptions: [],
  });
  expect(socket?.sent.map(parseMessage).at(-1)).toEqual({
    version: 1,
    type: "subscribe",
    subscriptions: [{ topic: "metrics" }],
  });
});

test("频道错误保留最后快照且不改变传输连接状态", async () => {
  render(
    <InstanceStreamProvider instanceId="instance-2">
      <Subscription topic="metrics" />
      <StatusProbe topic="metrics" />
    </InstanceStreamProvider>,
  );
  await flushEffects();
  const socket = FakeWebSocket.instances[0];
  act(() => socket?.emitOpen());
  act(() => socket?.emitMessage({
    version: 1,
    type: "snapshot",
    topic: "metrics",
    sampledAt: "2026-08-02T00:00:00.000Z",
    data: { inFlight: 2 },
  }));
  act(() => socket?.emitMessage({
    version: 1,
    type: "channel_error",
    topic: "metrics",
    sampledAt: "2026-08-02T00:00:01.000Z",
    code: "upstream_unavailable",
    message: "实例指标采样失败",
  }));

  expect(screen.getByTestId("transport").textContent).toBe("connected");
  expect(screen.getByTestId("topic-data").textContent).toBe('{"inFlight":2}');
  expect(screen.getByTestId("topic-error").textContent).toBe("实例指标采样失败");
});

test("断线只安排一次重连并在新连接打开后恢复订阅", async () => {
  vi.useFakeTimers();
  render(
    <InstanceStreamProvider instanceId="instance-3">
      <Subscription topic="overview" />
    </InstanceStreamProvider>,
  );
  await flushEffects();
  const first = FakeWebSocket.instances[0];
  act(() => first?.emitOpen());
  await flushEffects();

  act(() => {
    first?.emitClose();
    first?.emitClose();
  });
  await act(async () => vi.advanceTimersByTimeAsync(2_000));

  expect(FakeWebSocket.instances).toHaveLength(2);
  const second = FakeWebSocket.instances[1];
  act(() => second?.emitOpen());
  expect(second?.sent.map(parseMessage)).toContainEqual({
    version: 1,
    type: "subscribe",
    subscriptions: [{ topic: "overview" }],
  });
});

test("refresh 只发送刷新消息且不改变当前订阅", async () => {
  render(
    <InstanceStreamProvider instanceId="instance-4">
      <Subscription topic="runtime.list" />
      <RefreshButton />
    </InstanceStreamProvider>,
  );
  await flushEffects();
  const socket = FakeWebSocket.instances[0];
  act(() => socket?.emitOpen());
  await flushEffects();
  fireEvent.click(screen.getByRole("button", { name: "刷新 Runtime" }));

  expect(socket?.sent.map(parseMessage)).toEqual([
    {
      version: 1,
      type: "subscribe",
      subscriptions: [{ topic: "runtime.list" }],
    },
    {
      version: 1,
      type: "refresh",
      topics: ["runtime.list"],
    },
  ]);
});

test("切换 Runtime Detail 的 runId 时立即清除旧详情", async () => {
  render(
    <InstanceStreamProvider instanceId="instance-5">
      <RuntimeDetailProbe />
    </InstanceStreamProvider>,
  );
  await flushEffects();
  const socket = FakeWebSocket.instances[0];
  act(() => socket?.emitOpen());
  act(() => socket?.emitMessage({
    version: 1,
    type: "snapshot",
    topic: "runtime.detail",
    sampledAt: "2026-08-02T00:00:00.000Z",
    data: { run: { run_id: "run-1" } },
  }));
  expect(screen.getByTestId("run-detail").textContent).toContain("run-1");

  fireEvent.click(screen.getByRole("button", { name: "选择第二个 Run" }));
  await flushEffects();

  expect(screen.getByTestId("run-detail").textContent).toBe("尚无详情");
  expect(socket?.sent.map(parseMessage).at(-1)).toEqual({
    version: 1,
    type: "subscribe",
    subscriptions: [{ topic: "runtime.detail", runId: "run-2" }],
  });
});

function Subscription({ topic }: { topic: "overview" | "metrics" | "runtime.list" }): null {
  useInstanceStreamSubscription({ topic });
  return null;
}

function StatusProbe({ topic }: { topic: "metrics" }) {
  const transport = useInstanceTransportState();
  const state = useInstanceStreamTopic<{ inFlight: number }>(topic);
  return (
    <>
      <span data-testid="transport">{transport}</span>
      <span data-testid="topic-data">{JSON.stringify(state.data)}</span>
      <span data-testid="topic-error">{state.error}</span>
    </>
  );
}

function RefreshButton() {
  const refresh = useInstanceStreamRefresh();
  return <button type="button" onClick={() => refresh(["runtime.list"])}>刷新 Runtime</button>;
}

function RuntimeDetailProbe() {
  const [runId, setRunId] = useState("run-1");
  useInstanceStreamSubscription({ topic: "runtime.detail", runId });
  const detail = useInstanceStreamTopic<{ run: { run_id: string } }>("runtime.detail");
  return (
    <>
      <span data-testid="run-detail">{detail.data ? JSON.stringify(detail.data) : "尚无详情"}</span>
      <button type="button" onClick={() => setRunId("run-2")}>选择第二个 Run</button>
    </>
  );
}

function parseMessage(value: string): unknown {
  return JSON.parse(value);
}

async function flushEffects(): Promise<void> {
  await act(async () => {
    await Promise.resolve();
  });
}
