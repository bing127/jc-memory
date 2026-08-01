import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, test, vi } from "vitest";

import { buildApp } from "../src/app.js";

const origin = "http://127.0.0.1:5173";
const namespaceId = "00000000-0000-0000-0000-000000000001";
const apiKey = "sk-jcmemory-87s92kdp5vqz4mnh3cr0xfbj6gwy1aue7t";
const temporaryDirectories: string[] = [];

afterEach(async () => {
  await Promise.all(
    temporaryDirectories.splice(0).map((directory) =>
      rm(directory, { recursive: true, force: true }),
    ),
  );
});

describe("control-plane BFF", () => {
  test("rejects an instance API Key outside the branded format", async () => {
    const app = await createApp(vi.fn());
    const cookie = await login(app);

    const response = await app.inject({
      method: "POST",
      url: "/api/v1/instances",
      headers: { cookie, origin },
      payload: {
        name: "invalid key",
        baseUrl: "http://127.0.0.1:8787",
        namespaceId,
        apiKey: "server-side-instance-key",
      },
    });

    expect(response.statusCode).toBe(400);
    expect(await app.inject({
      method: "GET",
      url: "/api/v1/instances",
      headers: { cookie },
    }).then((result) => result.json())).toEqual([]);
    await app.close();
  });

  test("protects instance routes and proxies Stats without exposing credentials", async () => {
    const upstreamFetch = vi.fn(async (input: string | URL | Request, init?: RequestInit) => {
      expect(String(input)).toBe("http://127.0.0.1:8787/v1/memory/stats");
      expect(new Headers(init?.headers).get("x-api-key")).toBe(apiKey);
      const envelope = JSON.parse(String(init?.body));
      expect(envelope.access_scope.namespace_id).toBe(namespaceId);
      expect(envelope.command).toEqual({ command: "stats", payload: null });
      return Response.json({
        api_version: "v1",
        request_id: envelope.request_id,
        response: {
          response: "stats",
          payload: {
            scope: envelope.access_scope,
            fragment_count: 4,
            event_count: 5,
          },
        },
      });
    });
    const app = await createApp(upstreamFetch);

    const unauthorized = await app.inject({ method: "GET", url: "/api/v1/instances" });
    expect(unauthorized.statusCode).toBe(401);

    const cookie = await login(app);
    const created = await app.inject({
      method: "POST",
      url: "/api/v1/instances",
      headers: { cookie, origin },
      payload: {
        name: "本地记忆实例",
        baseUrl: "http://127.0.0.1:8787",
        namespaceId,
        apiKey,
      },
    });
    expect(created.statusCode).toBe(201);
    expect(created.body).not.toContain(apiKey);

    const instanceId = created.json().id as string;
    const stats = await app.inject({
      method: "POST",
      url: `/api/v1/instances/${instanceId}/memory/stats`,
      headers: { cookie, origin },
      payload: {
        scope: {
          tenantId: null,
          userId: null,
          agentId: null,
          projectId: null,
        },
      },
    });

    expect(stats.statusCode).toBe(200);
    expect(stats.json().response.payload).toMatchObject({
      fragment_count: 4,
      event_count: 5,
    });
    expect(stats.body).not.toContain(apiKey);
    expect(upstreamFetch).toHaveBeenCalledOnce();

    const listed = await app.inject({
      method: "GET",
      url: "/api/v1/instances",
      headers: { cookie },
    });
    expect(listed.statusCode).toBe(200);
    expect(listed.body).not.toContain(apiKey);
    expect(listed.json()).toHaveLength(1);
    await app.close();
  });

  test("proxies metrics as a normalized performance snapshot", async () => {
    const upstreamFetch = vi.fn(async (input: string | URL | Request, init?: RequestInit) => {
      expect(String(input)).toBe("http://127.0.0.1:8787/metrics");
      expect(new Headers(init?.headers).get("x-api-key")).toBeNull();
      return new Response([
        'jc_memory_requests_total{operation="recall",transport="http"} 2',
        'jc_memory_requests_total{operation="recall",transport="http",outcome="success"} 2',
        'jc_memory_request_duration_seconds_bucket{operation="recall",transport="http",le="0.025"} 2',
        'jc_memory_request_duration_seconds_bucket{operation="recall",transport="http",le="+Inf"} 2',
        "jc_memory_requests_in_flight 1",
        "jc_memory_server_ready 1",
      ].join("\n"));
    });
    const app = await createApp(upstreamFetch);
    const cookie = await login(app);
    const instanceId = await createInstance(app, cookie);

    const response = await app.inject({
      method: "GET",
      url: `/api/v1/instances/${instanceId}/metrics`,
      headers: { cookie },
    });

    expect(response.statusCode).toBe(200);
    expect(response.json()).toMatchObject({
      inFlight: 1,
      operations: [{ operation: "recall", total: 2, success: 2, latencyMs: { p95: 25 } }],
    });
    expect(response.body).not.toContain(apiKey);
    expect(upstreamFetch).toHaveBeenCalledOnce();
    await app.close();
  });

  test("rejects unsafe instance URLs before saving credentials", async () => {
    const app = await createApp(vi.fn());
    const cookie = await login(app);
    const response = await app.inject({
      method: "POST",
      url: "/api/v1/instances",
      headers: { cookie, origin },
      payload: {
        name: "unsafe",
        baseUrl: "http://memory.example.com",
        namespaceId,
        apiKey,
      },
    });

    expect(response.statusCode).toBe(400);
    const listed = await app.inject({
      method: "GET",
      url: "/api/v1/instances",
      headers: { cookie },
    });
    expect(listed.json()).toEqual([]);
    await app.close();
  });

  test("overwrites Observe fragment scope with the selected instance scope", async () => {
    const attackerNamespace = "00000000-0000-0000-0000-000000000099";
    const upstreamFetch = vi.fn(async (_input: string | URL | Request, init?: RequestInit) => {
      const envelope = JSON.parse(String(init?.body));
      expect(envelope.command.payload.fragment.scope).toEqual(envelope.access_scope);
      expect(envelope.command.payload.fragment.scope.namespace_id).toBe(namespaceId);
      expect(envelope.command.payload.fragment.scope.namespace_id).not.toBe(attackerNamespace);
      return Response.json({
        api_version: "v1",
        request_id: envelope.request_id,
        response: { response: "observe", payload: { created: true } },
      });
    });
    const app = await createApp(upstreamFetch);
    const cookie = await login(app);
    const created = await app.inject({
      method: "POST",
      url: "/api/v1/instances",
      headers: { cookie, origin },
      payload: {
        name: "scope test",
        baseUrl: "http://127.0.0.1:8787",
        namespaceId,
        apiKey,
      },
    });

    const response = await app.inject({
      method: "POST",
      url: `/api/v1/instances/${created.json().id}/memory/observe`,
      headers: { cookie, origin },
      payload: {
        scope: { tenantId: null, userId: null, agentId: null, projectId: null },
        fragment: {
          id: "00000000-0000-0000-0000-000000000101",
          scope: {
            namespace_id: attackerNamespace,
            tenant_id: null,
            user_id: null,
            agent_id: null,
            project_id: null,
          },
        },
      },
    });

    expect(response.statusCode).toBe(200);
    expect(upstreamFetch).toHaveBeenCalledOnce();
    await app.close();
  });

  test("binds every Commit fragment to the selected instance scope", async () => {
    const upstreamFetch = vi.fn(async (input: string | URL | Request, init?: RequestInit) => {
      expect(String(input)).toBe("http://127.0.0.1:8787/v1/memory/commit");
      const envelope = JSON.parse(String(init?.body));
      expect(envelope.command.command).toBe("commit");
      expect(envelope.command.payload.fragments).toHaveLength(1);
      expect(envelope.command.payload.fragments[0].scope).toEqual(envelope.access_scope);
      return Response.json({
        api_version: "v1",
        request_id: envelope.request_id,
        response: { response: "commit", payload: { receipts: [] } },
      });
    });
    const app = await createApp(upstreamFetch);
    const cookie = await login(app);
    const instanceId = await createInstance(app, cookie);

    const response = await app.inject({
      method: "POST",
      url: `/api/v1/instances/${instanceId}/memory/commit`,
      headers: { cookie, origin },
      payload: {
        scope: emptyScope(),
        fragments: [fragmentFixture("00000000-0000-0000-0000-000000000201")],
      },
    });

    expect(response.statusCode).toBe(200);
    expect(upstreamFetch).toHaveBeenCalledOnce();
    await app.close();
  });

  test.each(["correct", "forget"] as const)(
    "maps %s input to the stable mutation command",
    async (operation) => {
      const targetFragmentId = "00000000-0000-0000-0000-000000000301";
      const upstreamFetch = vi.fn(async (input: string | URL | Request, init?: RequestInit) => {
        expect(String(input)).toBe(`http://127.0.0.1:8787/v1/memory/${operation}`);
        const envelope = JSON.parse(String(init?.body));
        expect(envelope.command).toEqual({
          command: operation,
          payload: {
            target_fragment_id: targetFragmentId,
            content: { type: "text", text: operation === "correct" ? "修正内容" : "该记录已失效" },
            source: {
              source_type: "user",
              source_id: "control-plane-owner",
              actor_id: null,
              observer_id: null,
              tool_id: null,
              uri: null,
            },
            perspective: "first_person",
            occurred_at: "2026-08-01T00:00:00.000Z",
            verification_method: "manual",
            reason: operation === "correct" ? "来源信息已更新" : "用户明确要求失效",
          },
        });
        return Response.json({
          api_version: "v1",
          request_id: envelope.request_id,
          response: {
            response: "mutation",
            payload: { action: operation, target_fragment_id: targetFragmentId },
          },
        });
      });
      const app = await createApp(upstreamFetch);
      const cookie = await login(app);
      const instanceId = await createInstance(app, cookie);

      const response = await app.inject({
        method: "POST",
        url: `/api/v1/instances/${instanceId}/memory/${operation}`,
        headers: { cookie, origin },
        payload: {
          scope: emptyScope(),
          targetFragmentId,
          content: { type: "text", text: operation === "correct" ? "修正内容" : "该记录已失效" },
          source: {
            sourceType: "user",
            sourceId: "control-plane-owner",
            actorId: null,
            observerId: null,
            toolId: null,
            uri: null,
          },
          perspective: "first_person",
          occurredAt: "2026-08-01T00:00:00.000Z",
          verificationMethod: "manual",
          reason: operation === "correct" ? "来源信息已更新" : "用户明确要求失效",
        },
      });

      expect(response.statusCode).toBe(200);
      expect(upstreamFetch).toHaveBeenCalledOnce();
      await app.close();
    },
  );

  test("rejects mutation commands without an auditable reason", async () => {
    const upstreamFetch = vi.fn();
    const app = await createApp(upstreamFetch);
    const cookie = await login(app);
    const instanceId = await createInstance(app, cookie);

    const response = await app.inject({
      method: "POST",
      url: `/api/v1/instances/${instanceId}/memory/forget`,
      headers: { cookie, origin },
      payload: {
        scope: emptyScope(),
        targetFragmentId: "00000000-0000-0000-0000-000000000301",
        content: { type: "text", text: "该记录已失效" },
        source: {
          sourceType: "user",
          sourceId: "control-plane-owner",
          actorId: null,
          observerId: null,
          toolId: null,
          uri: null,
        },
        perspective: "first_person",
        occurredAt: "2026-08-01T00:00:00.000Z",
        verificationMethod: "manual",
        reason: "   ",
      },
    });

    expect(response.statusCode).toBe(400);
    expect(upstreamFetch).not.toHaveBeenCalled();
    await app.close();
  });

  test("proxies fixed Runtime list and detail routes without exposing credentials", async () => {
    const runId = "00000000-0000-7000-8000-000000000701";
    const upstreamFetch = vi.fn(async (input: string | URL | Request, init?: RequestInit) => {
      expect(new Headers(init?.headers).get("x-api-key")).toBe(apiKey);
      const url = String(input);
      if (url.endsWith("/v1/runtime/runs?state=completed&limit=20&offset=0")) {
        return Response.json({
          items: [{ run_id: runId, state: "completed", next_step: 4, version: 4 }],
          total: 1,
          limit: 20,
          offset: 0,
        });
      }
      expect(url).toBe(`http://127.0.0.1:8787/v1/runtime/runs/${runId}`);
      return Response.json({
        run: { run_id: runId, state: "completed", next_step: 4, version: 4 },
        transitions: [{ id: "transition-1", run_id: runId, step: 0 }],
      });
    });
    const app = await createApp(upstreamFetch);
    const cookie = await login(app);
    const instanceId = await createInstance(app, cookie);

    const list = await app.inject({
      method: "GET",
      url: `/api/v1/instances/${instanceId}/runtime/runs?state=completed&limit=20&offset=0`,
      headers: { cookie },
    });
    expect(list.statusCode).toBe(200);
    expect(list.json().items[0].run_id).toBe(runId);
    expect(list.body).not.toContain(apiKey);

    const detail = await app.inject({
      method: "GET",
      url: `/api/v1/instances/${instanceId}/runtime/runs/${runId}`,
      headers: { cookie },
    });
    expect(detail.statusCode).toBe(200);
    expect(detail.json().transitions[0].step).toBe(0);
    expect(detail.body).not.toContain(apiKey);
    expect(upstreamFetch).toHaveBeenCalledTimes(2);
    await app.close();
  });

  test("proxies Runtime Start with prompt only and keeps scope and credentials server-side", async () => {
    const runId = "00000000-0000-7000-8000-000000000711";
    const upstreamFetch = vi.fn(async (input: string | URL | Request, init?: RequestInit) => {
      expect(String(input)).toBe("http://127.0.0.1:8787/v1/runtime/runs");
      expect(init?.method).toBe("POST");
      expect(new Headers(init?.headers).get("x-api-key")).toBe(apiKey);
      expect(JSON.parse(String(init?.body))).toEqual({ prompt: "执行真实 Runtime" });
      return Response.json({ outcome: "completed", run_id: runId, state: "completed", response: { text: "完成" }, approval_id: null }, { status: 201 });
    });
    const app = await createApp(upstreamFetch);
    const cookie = await login(app);
    const instanceId = await createInstance(app, cookie);
    const response = await app.inject({
      method: "POST",
      url: `/api/v1/instances/${instanceId}/runtime/runs`,
      headers: { cookie, origin },
      payload: { prompt: "执行真实 Runtime" },
    });
    expect(response.statusCode).toBe(201);
    expect(response.json().run_id).toBe(runId);
    expect(response.body).not.toContain(apiKey);
    await app.close();
  });

  test("rejects Runtime Start from an untrusted origin before calling upstream", async () => {
    const upstreamFetch = vi.fn();
    const app = await createApp(upstreamFetch);
    const cookie = await login(app);
    const instanceId = await createInstance(app, cookie);

    const response = await app.inject({
      method: "POST",
      url: `/api/v1/instances/${instanceId}/runtime/runs`,
      headers: { cookie, origin: "https://attacker.example" },
      payload: { prompt: "不得执行的 Runtime" },
    });

    expect(response.statusCode).toBe(403);
    expect(response.json()).toMatchObject({ code: "origin_not_allowed" });
    expect(upstreamFetch).not.toHaveBeenCalled();
    await app.close();
  });
});

function emptyScope() {
  return { tenantId: null, userId: null, agentId: null, projectId: null };
}

function fragmentFixture(id: string) {
  return {
    id,
    scope: {
      namespace_id: "00000000-0000-0000-0000-000000000099",
      tenant_id: null,
      user_id: null,
      agent_id: null,
      project_id: null,
    },
    session_id: null,
    trace_id: null,
    kind: "observation",
    content: { type: "text", text: "需要提交的原始记录" },
    source: {
      source_type: "user",
      source_id: "control-plane-owner",
      actor_id: null,
      observer_id: null,
      tool_id: null,
      uri: null,
    },
    perspective: "first_person",
    occurred_at: "2026-08-01T00:00:00.000Z",
    observed_at: "2026-08-01T00:00:00.000Z",
    confidence: 1,
    importance: 0.5,
    validity: "unverified",
    security_label: "internal",
    evidence: [],
    metadata: {},
  };
}

async function createApp(fetch: typeof globalThis.fetch) {
  const directory = await mkdtemp(join(tmpdir(), "jc-memory-bff-"));
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
  if (!setCookie) {
    throw new Error("login did not return a session cookie");
  }
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
      name: "mutation test",
      baseUrl: "http://127.0.0.1:8787",
      namespaceId,
      apiKey,
    },
  });
  expect(response.statusCode).toBe(201);
  return response.json().id as string;
}
