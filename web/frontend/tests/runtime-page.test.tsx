import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, test, vi } from "vitest";
import { MemoryRouter, useLocation } from "react-router-dom";

import { InstanceStreamProvider } from "../src/lib/instance-stream.js";
import { RuntimePage } from "../src/pages/runtime-page.js";

const baseInstance = {
  id: "00000000-0000-0000-0000-000000000010",
  name: "本地实例",
  baseUrl: "http://127.0.0.1:8787",
  namespaceId: "00000000-0000-0000-0000-000000000001",
  protocolVersion: "v1" as const,
  createdAt: "2026-08-01T00:00:00.000Z",
  updatedAt: "2026-08-01T00:00:00.000Z",
};

class FakeWebSocket {
  static readonly CONNECTING = 0;
  static readonly OPEN = 1;
  static readonly CLOSED = 3;
  static readonly instances: FakeWebSocket[] = [];
  readonly sent: string[] = [];
  readyState = FakeWebSocket.CONNECTING;
  private readonly listeners = new Map<string, Set<EventListener>>();

  constructor(readonly url: string | URL) {
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

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
  FakeWebSocket.instances.length = 0;
});

test("Runtime 通过列表与详情 Topic 展示真实 Transition 且不再 GET 轮询", async () => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  const fetch = vi.fn();
  vi.stubGlobal("fetch", fetch);
  vi.stubGlobal("WebSocket", FakeWebSocket);
  const runId = "00000000-0000-7000-8000-000000000701";
  const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
  renderRuntime({ ...baseInstance, id: "00000000-0000-0000-0000-000000000011" });

  const socket = FakeWebSocket.instances[0];
  await act(async () => {
    socket?.emitOpen();
    await Promise.resolve();
  });
  expect(socket?.sent.map(parseMessage).at(-1)).toEqual({
    version: 1,
    type: "subscribe",
    subscriptions: [{ topic: "runtime.list" }],
  });
  act(() => socket?.emitMessage({
    version: 1,
    type: "snapshot",
    topic: "runtime.list",
    sampledAt: "2026-08-02T00:00:00.000Z",
    data: runtimeList(runId),
  }));

  expect(await screen.findByText(runId)).not.toBeNull();
  expect(screen.getByText("completed")).not.toBeNull();
  await user.click(screen.getByRole("button", { name: "查看 Run" }));
  await flushEffects();
  expect(screen.getByTestId("runtime-location").textContent).toBe(`?run=${runId}`);
  expect(socket?.sent.map(parseMessage).at(-1)).toEqual({
    version: 1,
    type: "subscribe",
    subscriptions: [
      { topic: "runtime.detail", runId },
      { topic: "runtime.list" },
    ],
  });

  act(() => socket?.emitMessage({
    version: 1,
    type: "snapshot",
    topic: "runtime.detail",
    sampledAt: "2026-08-02T00:00:01.000Z",
    data: runtimeDetail(runId),
  }));
  expect(await screen.findByText("observing -> recalling")).not.toBeNull();
  expect(screen.getAllByText("paused").length).toBeGreaterThan(0);
  expect(screen.getAllByText(/"phase": "observe"/).length).toBeGreaterThan(0);
  expect(screen.queryByText("Approval approved")).toBeNull();
  expect(screen.queryByText("Tool executed")).toBeNull();

  await act(async () => vi.advanceTimersByTimeAsync(20_000));
  expect(fetch).not.toHaveBeenCalled();
});

test("Runtime 列表频道失败时显示真实错误且不构造演示 Run", async () => {
  vi.stubGlobal("fetch", vi.fn());
  vi.stubGlobal("WebSocket", FakeWebSocket);
  renderRuntime({ ...baseInstance, id: "00000000-0000-0000-0000-000000000012" });
  const socket = FakeWebSocket.instances[0];
  act(() => {
    socket?.emitOpen();
    socket?.emitMessage({
      version: 1,
      type: "channel_error",
      topic: "runtime.list",
      sampledAt: "2026-08-02T00:00:00.000Z",
      code: "upstream_unavailable",
      message: "Runtime 列表采样失败",
    });
  });

  expect(await screen.findByText("Runtime 查询失败，请检查实例版本、API Key 与 Scope。")).not.toBeNull();
  expect(screen.queryByRole("button", { name: "查看 Run" })).toBeNull();
});

test("Runtime Start 成功后选择新 Run 并即时刷新列表", async () => {
  const runId = "00000000-0000-7000-8000-000000000711";
  const fetch = vi.fn(async (_input: string | URL | Request, init?: RequestInit) => {
    expect(init?.method).toBe("POST");
    expect(JSON.parse(String(init?.body))).toEqual({ prompt: "执行真实 Runtime" });
    return Response.json({
      outcome: "completed",
      run_id: runId,
      state: "completed",
      response: { text: "完成" },
      approval_id: null,
    }, { status: 201 });
  });
  vi.stubGlobal("fetch", fetch);
  vi.stubGlobal("WebSocket", FakeWebSocket);
  const user = userEvent.setup();
  renderRuntime({ ...baseInstance, id: "00000000-0000-0000-0000-000000000013" });
  const socket = FakeWebSocket.instances[0];
  await act(async () => {
    socket?.emitOpen();
    await Promise.resolve();
  });

  await user.type(screen.getByLabelText("Runtime Prompt"), "执行真实 Runtime");
  await user.click(screen.getByRole("button", { name: "启动 Run" }));
  expect(await screen.findByText("完成")).not.toBeNull();
  expect(screen.getByTestId("runtime-location").textContent).toContain(`run=${runId}`);
  await flushEffects();

  expect(socket?.sent.map(parseMessage)).toContainEqual({
    version: 1,
    type: "refresh",
    topics: ["runtime.list"],
  });
  expect(socket?.sent.map(parseMessage).at(-1)).toEqual({
    version: 1,
    type: "subscribe",
    subscriptions: [
      { topic: "runtime.detail", runId },
      { topic: "runtime.list" },
    ],
  });
});

function renderRuntime(instance = baseInstance) {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={[`/instances/${instance.id}/runtime`]}>
        <InstanceStreamProvider instanceId={instance.id}>
          <RuntimePage instance={instance} />
          <LocationProbe />
        </InstanceStreamProvider>
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

function runtimeList(runId: string) {
  return {
    items: [{
      run_id: runId,
      scope: {
        namespace_id: baseInstance.namespaceId,
        tenant_id: null,
        user_id: null,
        agent_id: null,
        project_id: null,
      },
      state: "completed",
      next_step: 4,
      version: 4,
      created_at_us: 1_785_556_800_000_000,
      updated_at_us: 1_785_556_801_000_000,
      control: null,
    }],
    total: 1,
    limit: 25,
    offset: 0,
  };
}

function runtimeDetail(runId: string) {
  return {
    run: {
      ...runtimeList(runId).items[0],
      control: {
        state: "paused",
        resume_state: "reasoning",
        user_input: null,
        next_attempt_at_us: null,
        attempt: 2,
        updated_at_us: 1_785_556_800_500_000,
      },
    },
    transitions: [{
      id: "transition-1",
      run_id: runId,
      step: 0,
      trace_id: "00000000-0000-7000-8000-000000000702",
      from_state: "observing",
      to_state: "recalling",
      input_refs: [],
      output_refs: [],
      attempt: 1,
      deadline_at_us: 1_785_556_860_000_000,
      payload: { phase: "observe" },
      created_at_us: 1_785_556_800_100_000,
    }],
  };
}

function LocationProbe() {
  const location = useLocation();
  return <output data-testid="runtime-location">{location.search}</output>;
}

function parseMessage(value: string): unknown {
  return JSON.parse(value);
}

async function flushEffects(): Promise<void> {
  await act(async () => {
    await Promise.resolve();
  });
}
