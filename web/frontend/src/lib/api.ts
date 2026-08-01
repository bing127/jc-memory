import type {
  ApiErrorBody,
  InstanceStatus,
  InstanceSummary,
  RuntimeRunDetail,
  RuntimeRunPage,
  RuntimeStartResult,
  ScopeSelector,
  Session,
} from "./contracts.js";

export class ApiError extends Error {
  constructor(
    readonly status: number,
    readonly body: ApiErrorBody,
  ) {
    super(body.message);
    this.name = "ApiError";
  }
}

export const api = {
  session: () => request<Session>("/api/v1/session"),
  login: (password: string) =>
    request<Session>("/api/v1/session", {
      method: "POST",
      body: JSON.stringify({ password }),
    }),
  logout: () => request<void>("/api/v1/session", { method: "DELETE" }),
  instances: () => request<InstanceSummary[]>("/api/v1/instances"),
  createInstance: (input: {
    name: string;
    baseUrl: string;
    namespaceId: string;
    apiKey: string;
  }) =>
    request<InstanceSummary>("/api/v1/instances", {
      method: "POST",
      body: JSON.stringify(input),
    }),
  removeInstance: (id: string) =>
    request<void>(`/api/v1/instances/${id}`, { method: "DELETE" }),
  instanceStatus: (id: string) =>
    request<InstanceStatus>(`/api/v1/instances/${id}/status`),
  runtimeRuns: (id: string, state?: string, limit = 25, offset = 0) => {
    const query = new URLSearchParams();
    if (state) query.set("state", state);
    query.set("limit", String(limit));
    query.set("offset", String(offset));
    return request<RuntimeRunPage>(`/api/v1/instances/${id}/runtime/runs?${query}`);
  },
  runtimeRun: (id: string, runId: string) =>
    request<RuntimeRunDetail>(`/api/v1/instances/${id}/runtime/runs/${runId}`),
  runtimeStart: (id: string, prompt: string) =>
    request<RuntimeStartResult>(`/api/v1/instances/${id}/runtime/runs`, {
      method: "POST",
      body: JSON.stringify({ prompt }),
    }),
  memory: <T>(
    instanceId: string,
    operation: "stats" | "observe" | "recall" | "explain" | "commit" | "correct" | "forget",
    body: { scope: ScopeSelector } & Record<string, unknown>,
  ) =>
    request<T>(`/api/v1/instances/${instanceId}/memory/${operation}`, {
      method: "POST",
      body: JSON.stringify(body),
    }),
};

async function request<T>(path: string, init: RequestInit = {}): Promise<T> {
  const response = await fetch(path, {
    ...init,
    credentials: "include",
    headers: {
      accept: "application/json",
      ...(init.body ? { "content-type": "application/json" } : {}),
      ...init.headers,
    },
  });
  if (response.status === 204) {
    return undefined as T;
  }
  const body = (await response.json()) as unknown;
  if (!response.ok) {
    const error = isApiError(body)
      ? body
      : { code: "request_failed", message: "控制面请求失败" };
    throw new ApiError(response.status, error);
  }
  return body as T;
}

function isApiError(value: unknown): value is ApiErrorBody {
  return (
    typeof value === "object" &&
    value !== null &&
    "code" in value &&
    typeof value.code === "string" &&
    "message" in value &&
    typeof value.message === "string"
  );
}
