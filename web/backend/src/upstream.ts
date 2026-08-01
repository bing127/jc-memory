import type { InstanceConnection } from "./instances.js";
import {
  buildMemoryEnvelope,
  memoryOperations,
  memoryResponseSchema,
  type MemoryCommandInput,
  type MemoryOperation,
  type MemoryResponseEnvelope,
} from "./protocol.js";

type FetchLike = typeof globalThis.fetch;

interface UpstreamClientOptions {
  fetch?: FetchLike;
  timeoutMs?: number;
}

interface PublicUpstreamError {
  code: string;
  message: string;
  requestId: string;
  status: number;
}

export class UpstreamError extends Error {
  constructor(
    private readonly publicError: PublicUpstreamError,
    options?: ErrorOptions,
  ) {
    super(publicError.message, options);
    this.name = "UpstreamError";
  }

  toPublic(): PublicUpstreamError {
    return { ...this.publicError };
  }
}

export interface UpstreamClient {
  execute(
    connection: InstanceConnection,
    input: MemoryCommandInput,
  ): Promise<MemoryResponseEnvelope>;
  status(
    connection: InstanceConnection,
  ): Promise<{ health: unknown; ready: unknown; metrics: string }>;
  metrics(connection: InstanceConnection): Promise<string>;
  runtimeRuns(
    connection: InstanceConnection,
    query: { state?: string | undefined; limit: number; offset: number },
  ): Promise<unknown>;
  runtimeRun(connection: InstanceConnection, runId: string): Promise<unknown>;
  runtimeStart(connection: InstanceConnection, prompt: string): Promise<unknown>;
}

export function validateBaseUrl(value: string): URL {
  let url: URL;
  try {
    url = new URL(value);
  } catch {
    throw new Error("instance base URL must be a valid URL");
  }
  if (url.username || url.password) {
    throw new Error("instance base URL must not contain credentials");
  }
  if (url.search || url.hash || url.pathname !== "/") {
    throw new Error("instance base URL must point to the origin root");
  }
  const loopbackHosts = new Set(["127.0.0.1", "localhost", "::1", "[::1]"]);
  if (url.protocol !== "https:" && !(url.protocol === "http:" && loopbackHosts.has(url.hostname))) {
    throw new Error("instance base URL must use HTTPS unless it targets loopback");
  }
  return url;
}

export function createUpstreamClient(
  options: UpstreamClientOptions = {},
): UpstreamClient {
  const fetchImpl = options.fetch ?? globalThis.fetch.bind(globalThis);
  const timeoutMs = options.timeoutMs ?? 30_000;

  return {
    async execute(connection, input) {
      assertOperation(input.operation, input.requestId);
      if (input.accessScope.namespace_id !== connection.namespaceId) {
        throw publicError(
          "scope_mismatch",
          "请求 Scope 与实例 Namespace 不一致",
          input.requestId,
          400,
        );
      }
      const baseUrl = validateBaseUrl(connection.baseUrl);
      const target = new URL(`v1/memory/${input.operation}`, baseUrl);
      const response = await fetchWithTimeout(
        fetchImpl,
        target,
        {
          method: "POST",
          headers: {
            accept: "application/json",
            "content-type": "application/json",
            "x-api-key": connection.apiKey,
          },
          body: JSON.stringify(buildMemoryEnvelope(input)),
        },
        timeoutMs,
        input.requestId,
      );
      if (!response.ok) {
        throw mapHttpError(response.status, input.requestId);
      }
      const body = await parseJson(response, input.requestId);
      const parsed = memoryResponseSchema.safeParse(body);
      if (!parsed.success) {
        throw publicError(
          "invalid_upstream_response",
          "记忆实例返回了不兼容的响应",
          input.requestId,
          502,
        );
      }
      return parsed.data;
    },
    async status(connection) {
      const baseUrl = validateBaseUrl(connection.baseUrl);
      const [health, ready, metrics] = await Promise.all([
        fetchStatusJson(fetchImpl, new URL("healthz", baseUrl), timeoutMs),
        fetchStatusJson(fetchImpl, new URL("readyz", baseUrl), timeoutMs),
        fetchStatusText(fetchImpl, new URL("metrics", baseUrl), timeoutMs),
      ]);
      return { health, ready, metrics };
    },
    async metrics(connection) {
      const baseUrl = validateBaseUrl(connection.baseUrl);
      return fetchStatusText(fetchImpl, new URL("metrics", baseUrl), timeoutMs);
    },
    async runtimeRuns(connection, query) {
      const baseUrl = validateBaseUrl(connection.baseUrl);
      const target = new URL("v1/runtime/runs", baseUrl);
      if (query.state) target.searchParams.set("state", query.state);
      target.searchParams.set("limit", String(query.limit));
      target.searchParams.set("offset", String(query.offset));
      return fetchRuntimeJson(fetchImpl, target, connection.apiKey, timeoutMs);
    },
    async runtimeRun(connection, runId) {
      const baseUrl = validateBaseUrl(connection.baseUrl);
      const target = new URL(`v1/runtime/runs/${runId}`, baseUrl);
      return fetchRuntimeJson(fetchImpl, target, connection.apiKey, timeoutMs);
    },
    async runtimeStart(connection, prompt) {
      const baseUrl = validateBaseUrl(connection.baseUrl);
      const target = new URL("v1/runtime/runs", baseUrl);
      return fetchRuntimeJson(fetchImpl, target, connection.apiKey, timeoutMs, {
        method: "POST",
        body: JSON.stringify({ prompt }),
      });
    },
  };
}

async function fetchRuntimeJson(
  fetchImpl: FetchLike,
  input: URL,
  apiKey: string,
  timeoutMs: number,
  request: { method: "POST"; body: string } | undefined = undefined,
): Promise<unknown> {
  const response = await fetchWithTimeout(
    fetchImpl,
    input,
    {
      method: request?.method ?? "GET",
      headers: {
        accept: "application/json",
        ...(request ? { "content-type": "application/json" } : {}),
        "x-api-key": apiKey,
      },
      ...(request ? { body: request.body } : {}),
    },
    timeoutMs,
    "runtime-query",
  );
  if (!response.ok) throw mapRuntimeHttpError(response.status, "runtime-query");
  return parseJson(response, "runtime-query");
}

function assertOperation(operation: MemoryOperation, requestId: string): void {
  if (!memoryOperations.includes(operation)) {
    throw publicError(
      "unsupported_operation",
      "控制面不支持该记忆操作",
      requestId,
      400,
    );
  }
}

async function fetchWithTimeout(
  fetchImpl: FetchLike,
  input: URL,
  init: RequestInit,
  timeoutMs: number,
  requestId: string,
): Promise<Response> {
  try {
    return await fetchImpl(input, {
      ...init,
      redirect: "error",
      signal: AbortSignal.timeout(timeoutMs),
    });
  } catch (error) {
    const timedOut = error instanceof DOMException && error.name === "TimeoutError";
    throw publicError(
      timedOut ? "deadline_exceeded" : "transport_error",
      timedOut ? "记忆实例请求超时" : "无法连接记忆实例",
      requestId,
      timedOut ? 504 : 502,
      error,
    );
  }
}

async function fetchStatusJson(
  fetchImpl: FetchLike,
  input: URL,
  timeoutMs: number,
): Promise<unknown> {
  const response = await fetchWithTimeout(fetchImpl, input, { method: "GET" }, timeoutMs, "status");
  if (!response.ok) {
    throw mapHttpError(response.status, "status");
  }
  return parseJson(response, "status");
}

async function fetchStatusText(
  fetchImpl: FetchLike,
  input: URL,
  timeoutMs: number,
): Promise<string> {
  const response = await fetchWithTimeout(fetchImpl, input, { method: "GET" }, timeoutMs, "status");
  if (!response.ok) {
    throw mapHttpError(response.status, "status");
  }
  return response.text();
}

async function parseJson(response: Response, requestId: string): Promise<unknown> {
  try {
    return await response.json();
  } catch (error) {
    throw publicError(
      "invalid_upstream_response",
      "记忆实例返回了非 JSON 响应",
      requestId,
      502,
      error,
    );
  }
}

function mapHttpError(status: number, requestId: string): UpstreamError {
  if (status === 401) {
    return publicError("instance_unauthorized", "实例凭据无效", requestId, 502);
  }
  if (status === 403) {
    return publicError("permission_denied", "实例拒绝当前 Scope 或操作", requestId, 403);
  }
  if (status === 408 || status === 504) {
    return publicError("deadline_exceeded", "记忆实例请求超时", requestId, 504);
  }
  if (status === 429) {
    return publicError("resource_exhausted", "记忆实例当前请求过多", requestId, 429);
  }
  if (status >= 400 && status < 500) {
    return publicError("invalid_argument", "记忆实例拒绝了请求", requestId, 400);
  }
  return publicError("upstream_error", "记忆实例请求失败", requestId, 502);
}

function mapRuntimeHttpError(status: number, requestId: string): UpstreamError {
  if (status === 404) {
    return publicError("not_found", "Runtime Run 不存在", requestId, 404);
  }
  if (status === 503) {
    return publicError("unavailable", "Runtime 查询服务不可用", requestId, 503);
  }
  return mapHttpError(status, requestId);
}

function publicError(
  code: string,
  message: string,
  requestId: string,
  status: number,
  cause?: unknown,
): UpstreamError {
  return new UpstreamError(
    { code, message, requestId, status },
    cause === undefined ? undefined : { cause },
  );
}
