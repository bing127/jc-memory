import { act, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import { App } from "../src/app.js";

const instanceId = "019b8c36-e7b0-7f41-8b92-cc77e129fc2d";
const namespaceId = "00000000-0000-0000-0000-000000000001";

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
  window.history.replaceState({}, "", "/");
});

test("总览通过 overview 频道展示权威状态且不再周期请求 HTTP", async () => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  window.history.replaceState({}, "", `/instances/${instanceId}/overview`);
  const fetch = vi.fn(async (input: string | URL | Request) => {
    const url = String(input);
    if (url === "/api/v1/session") {
      return Response.json({ authenticated: true, subject: "owner" });
    }
    if (url === "/api/v1/instances") {
      return Response.json([{
        id: instanceId,
        name: "本地记忆实例",
        baseUrl: "http://127.0.0.1:8787/",
        namespaceId,
        protocolVersion: "v1",
        createdAt: "2026-08-01T00:00:00.000Z",
        updatedAt: "2026-08-01T00:00:00.000Z",
      }]);
    }
    throw new Error(`总览不应发起周期请求：${url}`);
  });
  vi.stubGlobal("fetch", fetch);
  vi.stubGlobal("WebSocket", FakeWebSocket);

  render(<App />);
  expect(await screen.findByRole("heading", { name: "运行总览" })).not.toBeNull();
  expect(FakeWebSocket.instances).toHaveLength(1);
  const socket = FakeWebSocket.instances[0];
  await act(async () => {
    socket?.emitOpen();
    await Promise.resolve();
  });
  expect(socket?.sent.map((message) => JSON.parse(message))).toContainEqual({
    version: 1,
    type: "subscribe",
    subscriptions: [{ topic: "overview" }],
  });

  act(() => socket?.emitMessage({
    version: 1,
    type: "snapshot",
    topic: "overview",
    sampledAt: "2026-08-02T00:00:00.000Z",
    data: {
      status: {
        health: { status: "ok" },
        ready: { status: "ready" },
        serverReady: true,
      },
      stats: {
        api_version: "v1",
        request_id: "overview-stats-001",
        response: {
          response: "stats",
          payload: { fragment_count: 4, event_count: 5 },
        },
      },
    },
  }));

  expect(await screen.findByText("4")).not.toBeNull();
  expect(screen.getByText("5")).not.toBeNull();
  expect(screen.getByText("overview-stats-001")).not.toBeNull();
  expect(screen.getByText("Health").className).toContain("status-ok");
  expect(screen.getByText("Ready").className).toContain("status-ok");
  expect(screen.getByText("server_ready").className).toContain("status-ok");
  expect(screen.queryByText(/P95|CPU|模型成本|云账单/)).toBeNull();

  expect(fetch).toHaveBeenCalledTimes(2);
  await act(async () => vi.advanceTimersByTimeAsync(30_000));
  expect(fetch).toHaveBeenCalledTimes(2);
});
