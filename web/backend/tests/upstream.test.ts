import { describe, expect, test, vi } from "vitest";

import {
  UpstreamError,
  createUpstreamClient,
  validateBaseUrl,
} from "../src/upstream.js";

const connection = {
  id: "019b8c36-e7b0-7f41-8b92-cc77e129fc2d",
  name: "local",
  baseUrl: "http://127.0.0.1:8787",
  namespaceId: "00000000-0000-0000-0000-000000000001",
  protocolVersion: "v1" as const,
  apiKey: "instance-secret",
  createdAt: "2026-08-01T00:00:00.000Z",
  updatedAt: "2026-08-01T00:00:00.000Z",
};

describe("memory-server upstream policy", () => {
  test("allows HTTPS and loopback HTTP but rejects unsafe base URLs", () => {
    expect(validateBaseUrl("https://memory.example.com").href).toBe(
      "https://memory.example.com/",
    );
    expect(validateBaseUrl("http://127.0.0.1:8787").href).toBe(
      "http://127.0.0.1:8787/",
    );
    expect(validateBaseUrl("http://localhost:8787").href).toBe(
      "http://localhost:8787/",
    );
    expect(() => validateBaseUrl("http://memory.example.com")).toThrow("HTTPS");
    expect(() => validateBaseUrl("https://user:pass@memory.example.com")).toThrow(
      "credentials",
    );
    expect(() => validateBaseUrl("https://memory.example.com/api")).toThrow(
      "origin root",
    );
  });

  test("builds the stable envelope and sends the API key only upstream", async () => {
    const fetchMock = vi.fn(async (_input: string | URL | Request, init?: RequestInit) => {
      const headers = new Headers(init?.headers);
      expect(init?.redirect).toBe("error");
      expect(headers.get("x-api-key")).toBe(connection.apiKey);
      expect(headers.get("authorization")).toBeNull();
      expect(JSON.parse(String(init?.body))).toEqual({
        api_version: "v1",
        request_id: "request-stats-001",
        access_scope: {
          namespace_id: connection.namespaceId,
          tenant_id: null,
          user_id: null,
          agent_id: null,
          project_id: null,
        },
        command: { command: "stats", payload: null },
      });
      return Response.json({
        api_version: "v1",
        request_id: "request-stats-001",
        response: {
          response: "stats",
          payload: { fragment_count: 1, event_count: 1 },
        },
      });
    });
    const client = createUpstreamClient({ fetch: fetchMock });

    const response = await client.execute(connection, {
      operation: "stats",
      requestId: "request-stats-001",
      accessScope: {
        namespace_id: connection.namespaceId,
        tenant_id: null,
        user_id: null,
        agent_id: null,
        project_id: null,
      },
      payload: {},
    });

    expect(fetchMock).toHaveBeenCalledOnce();
    expect(response.response.payload).toEqual({ fragment_count: 1, event_count: 1 });
  });

  test("does not retry writes and redacts upstream failures", async () => {
    const fetchMock = vi.fn(async () =>
      Response.json(
        { message: `upstream leaked ${connection.apiKey}` },
        { status: 500 },
      ),
    );
    const client = createUpstreamClient({ fetch: fetchMock });

    let caught: unknown;
    try {
      await client.execute(connection, {
        operation: "observe",
        requestId: "request-observe-001",
        accessScope: {
          namespace_id: connection.namespaceId,
          tenant_id: null,
          user_id: null,
          agent_id: null,
          project_id: null,
        },
        payload: { fragment: {} },
      });
    } catch (error) {
      caught = error;
    }

    expect(fetchMock).toHaveBeenCalledOnce();
    expect(caught).toBeInstanceOf(UpstreamError);
    const publicError = (caught as UpstreamError).toPublic();
    expect(publicError).toEqual({
      code: "upstream_error",
      message: "记忆实例请求失败",
      requestId: "request-observe-001",
      status: 502,
    });
    expect(JSON.stringify(publicError)).not.toContain(connection.apiKey);
  });

  test("使用调用方传入的上游超时构造 AbortSignal", async () => {
    const timeout = vi.spyOn(AbortSignal, "timeout");
    const fetchMock = vi.fn(async () => Response.json({
      api_version: "v1",
      request_id: "request-stats-timeout",
      response: { response: "stats", payload: {} },
    }));
    const client = createUpstreamClient({ fetch: fetchMock, timeoutMs: 321 });

    await client.execute(connection, {
      operation: "stats",
      requestId: "request-stats-timeout",
      accessScope: {
        namespace_id: connection.namespaceId,
        tenant_id: null,
        user_id: null,
        agent_id: null,
        project_id: null,
      },
      payload: {},
    });

    expect(timeout).toHaveBeenCalledWith(321);
    timeout.mockRestore();
  });

  test.each([
    { upstreamStatus: 404, code: "not_found", status: 404 },
    { upstreamStatus: 503, code: "unavailable", status: 503 },
  ])(
    "preserves Runtime $upstreamStatus boundaries as $code",
    async ({ upstreamStatus, code, status }) => {
      const client = createUpstreamClient({
        fetch: vi.fn(async () => new Response(null, { status: upstreamStatus })),
      });

      let caught: unknown;
      try {
        await client.runtimeRun(
          connection,
          "00000000-0000-7000-8000-000000000701",
        );
      } catch (error) {
        caught = error;
      }

      expect(caught).toBeInstanceOf(UpstreamError);
      expect((caught as UpstreamError).toPublic()).toMatchObject({ code, status });
    },
  );
});
