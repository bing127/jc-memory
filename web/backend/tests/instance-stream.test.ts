import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, test, vi } from "vitest";
import WebSocket from "ws";

import { buildApp } from "../src/app.js";
import type { StreamIntervals } from "../src/config.js";

const origin = "http://127.0.0.1:5173";
const namespaceId = "00000000-0000-0000-0000-000000000001";
const runId = "00000000-0000-7000-8000-000000000701";
const nextRunId = "00000000-0000-7000-8000-000000000702";
const apiKey = "sk-jcmemory-87s92kdp5vqz4mnh3cr0xfbj6gwy1aue7t";
const temporaryDirectories: string[] = [];

afterEach(async () => {
  await Promise.all(temporaryDirectories.splice(0).map((directory) => rm(directory, { recursive: true, force: true })));
});

describe("统一实例 WebSocket", () => {
  test("一条已认证连接订阅多个 Topic 并收到首轮快照", async () => {
    const app = await createApp(upstreamFixture());
    const cookie = await login(app);
    const instanceId = await createInstance(app, cookie);
    await app.listen({ host: "127.0.0.1", port: 0 });
    const address = app.server.address();
    if (!address || typeof address === "string") throw new Error("测试服务器未监听 TCP 地址");

    const socket = new WebSocket(
      `ws://127.0.0.1:${address.port}/api/v1/instances/${instanceId}/stream`,
      { headers: { cookie, origin } },
    );
    await new Promise<void>((resolve, reject) => {
      socket.once("open", resolve);
      socket.once("error", reject);
    });
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

    const messages = await collectSnapshots(socket, 4);
    expect(new Set(messages.map((message) => message.topic))).toEqual(new Set([
      "overview",
      "metrics",
      "runtime.list",
      "runtime.detail",
    ]));
    expect(messages.every((message) => message.version === 1)).toBe(true);

    await closeSocket(socket);
    await app.close();
  });

  test("重复订阅不会重复启动指标采样", async () => {
    const upstreamFetch = upstreamFixture();
    const { app, socket } = await openStream(upstreamFetch);
    const first = waitForMessage(socket, (message) => message.type === "snapshot");
    sendSubscriptions(socket, [{ topic: "metrics" }]);
    await first;
    const callsBeforeDuplicate = metricCallCount(upstreamFetch);

    sendSubscriptions(socket, [{ topic: "metrics" }]);
    await new Promise((resolve) => setTimeout(resolve, 100));
    expect(metricCallCount(upstreamFetch)).toBe(callsBeforeDuplicate);

    await closeSocket(socket);
    await app.close();
  });

  test("切换 Runtime Detail 会停止旧 Run 并立即采样新 Run", async () => {
    const upstreamFetch = upstreamFixture();
    const { app, socket } = await openStream(upstreamFetch);
    const oldSnapshot = waitForMessage(socket, (message) => message.topic === "runtime.detail");
    sendSubscriptions(socket, [{ topic: "runtime.detail", runId }]);
    await oldSnapshot;
    const newSnapshot = waitForMessage(
      socket,
      (message) => message.topic === "runtime.detail" && message.data?.run?.run_id === nextRunId,
    );
    sendSubscriptions(socket, [{ topic: "runtime.detail", runId: nextRunId }]);
    await newSnapshot;

    const detailUrls = upstreamFetch.mock.calls
      .map(([input]) => String(input))
      .filter((url) => /\/v1\/runtime\/runs\/[0-9a-f-]+$/i.test(url));
    expect(detailUrls).toEqual([
      `http://127.0.0.1:8787/v1/runtime/runs/${runId}`,
      `http://127.0.0.1:8787/v1/runtime/runs/${nextRunId}`,
    ]);

    await closeSocket(socket);
    await app.close();
  });

  test("连续失败只发送一次频道错误且恢复后发送快照", async () => {
    const baseFetch = upstreamFixture();
    let metricAttempt = 0;
    const upstreamFetch = vi.fn(async (input: string | URL | Request, init?: RequestInit) => {
      if (String(input).endsWith("/metrics")) {
        metricAttempt += 1;
        if (metricAttempt < 3) throw new TypeError("fixture metrics offline");
      }
      return baseFetch(input, init);
    });
    const { app, socket } = await openStream(upstreamFetch);
    const received: StreamMessage[] = [];
    socket.on("message", (payload) => received.push(JSON.parse(payload.toString()) as StreamMessage));
    const firstError = waitForMessage(socket, (message) => message.type === "channel_error");
    sendSubscriptions(socket, [{ topic: "metrics" }]);
    await firstError;

    socket.send(JSON.stringify({ version: 1, type: "refresh", topics: ["metrics"] }));
    await new Promise((resolve) => setTimeout(resolve, 100));
    const recovered = waitForMessage(socket, (message) => message.type === "snapshot" && message.topic === "metrics");
    socket.send(JSON.stringify({ version: 1, type: "refresh", topics: ["metrics"] }));
    await recovered;

    expect(received.filter((message) => message.type === "channel_error")).toHaveLength(1);
    expect(received.at(-1)).toMatchObject({ type: "snapshot", topic: "metrics" });

    await closeSocket(socket);
    await app.close();
  });

  test("refresh 未订阅频道返回协议错误且不改变订阅", async () => {
    const { app, socket } = await openStream(upstreamFixture());
    const protocolError = waitForMessage(socket, (message) => message.type === "protocol_error");
    socket.send(JSON.stringify({ version: 1, type: "refresh", topics: ["runtime.list"] }));

    await expect(protocolError).resolves.toMatchObject({
      type: "protocol_error",
      code: "topic_not_subscribed",
    });
    await closeSocket(socket);
    await app.close();
  });

  test("非法订阅返回协议错误并保留上一份合法任务", async () => {
    const upstreamFetch = upstreamFixture();
    const { app, socket } = await openStream(upstreamFetch);
    const firstSnapshot = waitForMessage(socket, (message) => message.type === "snapshot");
    sendSubscriptions(socket, [{ topic: "metrics" }]);
    await firstSnapshot;
    const protocolError = waitForMessage(socket, (message) => message.type === "protocol_error");
    sendSubscriptions(socket, [{ topic: "metrics" }, { topic: "metrics" }]);
    await protocolError;
    const nextSnapshot = waitForMessage(socket, (message) => message.type === "snapshot");
    await nextSnapshot;

    expect(metricCallCount(upstreamFetch)).toBeGreaterThanOrEqual(2);
    await closeSocket(socket);
    await app.close();
  }, 10_000);

  test("关闭连接后停止指标采样", async () => {
    const upstreamFetch = upstreamFixture();
    const { app, socket } = await openStream(upstreamFetch);
    const snapshot = waitForMessage(socket, (message) => message.type === "snapshot");
    sendSubscriptions(socket, [{ topic: "metrics" }]);
    await snapshot;
    await closeSocket(socket);
    const callsAfterClose = metricCallCount(upstreamFetch);
    await new Promise((resolve) => setTimeout(resolve, 1_100));

    expect(metricCallCount(upstreamFetch)).toBe(callsAfterClose);
    await app.close();
  }, 10_000);

  test("使用注入的指标周期连续采样而不是使用固定周期", async () => {
    const upstreamFetch = upstreamFixture();
    const { app, socket } = await openStream(upstreamFetch, { metrics: 20 });
    const firstSnapshot = waitForMessage(socket, (message) => message.topic === "metrics");
    sendSubscriptions(socket, [{ topic: "metrics" }]);
    await firstSnapshot;
    await new Promise((resolve) => setTimeout(resolve, 75));

    expect(metricCallCount(upstreamFetch)).toBeGreaterThanOrEqual(3);
    await closeSocket(socket);
    await app.close();
  });

  test("拒绝非法 Origin 且不会开始上游采样", async () => {
    const upstreamFetch = upstreamFixture();
    const app = await createApp(upstreamFetch);
    const cookie = await login(app);
    const instanceId = await createInstance(app, cookie);
    await app.listen({ host: "127.0.0.1", port: 0 });

    const socket = await connectStream(app, instanceId, {
      cookie,
      origin: "http://malicious.example.com",
    });
    const close = await waitForClose(socket);

    expect(close).toEqual({ code: 1008, reason: "unauthorized" });
    expect(upstreamFetch).not.toHaveBeenCalled();
    await app.close();
  });

  test("拒绝不存在的实例且不会开始上游采样", async () => {
    const upstreamFetch = upstreamFixture();
    const app = await createApp(upstreamFetch);
    const cookie = await login(app);
    await app.listen({ host: "127.0.0.1", port: 0 });

    const socket = await connectStream(app, "00000000-0000-0000-0000-000000000099", {
      cookie,
      origin,
    });
    const close = await waitForClose(socket);

    expect(close).toEqual({ code: 1008, reason: "instance_not_found" });
    expect(upstreamFetch).not.toHaveBeenCalled();
    await app.close();
  });
});

interface StreamMessage {
  version: number;
  type: string;
  topic?: string;
  code?: string;
  data?: { run?: { run_id?: string } };
}

function upstreamFixture() {
  return vi.fn(async (input: string | URL | Request, init?: RequestInit) => {
    const url = String(input);
    if (url.endsWith("/healthz")) return Response.json({ status: "ok" });
    if (url.endsWith("/readyz")) return Response.json({ status: "ready" });
    if (url.endsWith("/metrics")) {
      return new Response([
        'jc_memory_requests_total{operation="recall",transport="http"} 2',
        'jc_memory_requests_total{operation="recall",transport="http",outcome="success"} 2',
        'jc_memory_request_duration_seconds_bucket{operation="recall",transport="http",le="0.025"} 2',
        'jc_memory_request_duration_seconds_bucket{operation="recall",transport="http",le="+Inf"} 2',
        "jc_memory_requests_in_flight 1",
        "jc_memory_server_ready 1",
      ].join("\n"));
    }
    if (url.endsWith("/v1/memory/stats")) {
      const request = JSON.parse(String(init?.body)) as { request_id: string };
      return Response.json({
        api_version: "v1",
        request_id: request.request_id,
        response: { response: "stats", payload: { fragment_count: 3, event_count: 5 } },
      });
    }
    if (url.includes("/v1/runtime/runs?")) {
      return Response.json({
        items: [{ run_id: runId, state: "completed", next_step: 1, version: 1 }],
        total: 1,
        limit: 25,
        offset: 0,
      });
    }
    const detailMatch = /\/v1\/runtime\/runs\/([0-9a-f-]+)$/i.exec(url);
    if (detailMatch?.[1]) {
      return Response.json({
        run: { run_id: detailMatch[1], state: "completed", next_step: 1, version: 1 },
        transitions: [],
      });
    }
    throw new Error(`未处理的上游请求：${url}`);
  });
}

async function openStream(
  fetch: typeof globalThis.fetch,
  intervalOverrides: Partial<StreamIntervals> = {},
) {
  const app = await createApp(fetch, intervalOverrides);
  const cookie = await login(app);
  const instanceId = await createInstance(app, cookie);
  await app.listen({ host: "127.0.0.1", port: 0 });
  const socket = await connectStream(app, instanceId, { cookie, origin });
  await new Promise<void>((resolve, reject) => {
    socket.once("open", resolve);
    socket.once("error", reject);
  });
  return { app, socket };
}

async function connectStream(
  app: Awaited<ReturnType<typeof buildApp>>,
  instanceId: string,
  headers: { cookie: string; origin: string },
): Promise<WebSocket> {
  const address = app.server.address();
  if (!address || typeof address === "string") throw new Error("测试服务器未监听 TCP 地址");
  return new WebSocket(
    `ws://127.0.0.1:${address.port}/api/v1/instances/${instanceId}/stream`,
    { headers },
  );
}

function sendSubscriptions(socket: WebSocket, subscriptions: unknown[]): void {
  socket.send(JSON.stringify({ version: 1, type: "subscribe", subscriptions }));
}

function waitForMessage(
  socket: WebSocket,
  predicate: (message: StreamMessage) => boolean,
): Promise<StreamMessage> {
  return new Promise((resolve, reject) => {
    const timeout = setTimeout(() => {
      socket.off("message", onMessage);
      reject(new Error("等待统一流消息超时"));
    }, 5_000);
    const onMessage = (payload: WebSocket.RawData) => {
      const message = JSON.parse(payload.toString()) as StreamMessage;
      if (!predicate(message)) return;
      clearTimeout(timeout);
      socket.off("message", onMessage);
      resolve(message);
    };
    socket.on("message", onMessage);
  });
}

function metricCallCount(fetch: ReturnType<typeof vi.fn>): number {
  return fetch.mock.calls.filter(([input]) => String(input).endsWith("/metrics")).length;
}

async function collectSnapshots(
  socket: WebSocket,
  count: number,
): Promise<Array<{ version: number; type: string; topic: string }>> {
  return new Promise((resolve, reject) => {
    const messages: Array<{ version: number; type: string; topic: string }> = [];
    const timeout = setTimeout(() => reject(new Error("等待统一流首轮快照超时")), 5_000);
    socket.on("message", (payload) => {
      const message = JSON.parse(payload.toString()) as { version: number; type: string; topic: string };
      if (message.type !== "snapshot") return;
      messages.push(message);
      if (messages.length === count) {
        clearTimeout(timeout);
        resolve(messages);
      }
    });
    socket.once("error", reject);
  });
}

async function closeSocket(socket: WebSocket): Promise<void> {
  await new Promise<void>((resolve) => {
    socket.once("close", () => resolve());
    socket.close();
  });
}

async function waitForClose(socket: WebSocket): Promise<{ code: number; reason: string }> {
  return new Promise((resolve, reject) => {
    socket.once("close", (code, reason) => resolve({ code, reason: reason.toString() }));
    socket.once("error", reject);
  });
}

async function createApp(
  fetch: typeof globalThis.fetch,
  intervalOverrides: Partial<StreamIntervals> = {},
) {
  const directory = await mkdtemp(join(tmpdir(), "jc-memory-stream-"));
  temporaryDirectories.push(directory);
  return buildApp({
    config: {
      databasePath: join(directory, "control-plane.db"),
      adminPassword: "correct-password",
      sessionSecret: Buffer.alloc(32, 3),
      credentialKey: Buffer.alloc(32, 4),
      allowedOrigins: [origin],
      secureCookies: false,
      sessionTtlMs: 60_000,
      bindHost: "127.0.0.1",
      bindPort: 8790,
      upstreamTimeoutMs: 30_000,
      streamIntervals: {
        overview: 15_000,
        metrics: 1_000,
        runtimeList: 10_000,
        runtimeDetail: 5_000,
        ...intervalOverrides,
      },
    },
    fetch,
    logger: false,
  });
}

async function login(app: Awaited<ReturnType<typeof buildApp>>): Promise<string> {
  const response = await app.inject({
    method: "POST",
    url: "/api/v1/session",
    headers: { origin },
    payload: { password: "correct-password" },
  });
  expect(response.statusCode).toBe(200);
  const header = response.headers["set-cookie"];
  const setCookie = Array.isArray(header) ? header[0] : header;
  if (!setCookie) throw new Error("登录未返回 Session Cookie");
  return setCookie.split(";", 1)[0] ?? "";
}

async function createInstance(
  app: Awaited<ReturnType<typeof buildApp>>,
  cookie: string,
): Promise<string> {
  const response = await app.inject({
    method: "POST",
    url: "/api/v1/instances",
    headers: { cookie, origin },
    payload: {
      name: "stream test",
      baseUrl: "http://127.0.0.1:8787",
      namespaceId,
      apiKey,
    },
  });
  expect(response.statusCode).toBe(201);
  return response.json().id as string;
}
