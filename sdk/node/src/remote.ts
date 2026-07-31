import {
  JiangchengMemoryError,
  type JiangchengMemoryErrorCode,
} from "./errors.js";
import type {
  ClientRequestOptions,
  CommitRequest,
  ExplainRequest,
  MemoryRequest,
  MemoryResponse,
  MutationRequest,
  ObserveRequest,
  RecallRequest,
  RemoteMemoryClientOptions,
  StatsRequest,
} from "./types.js";

type FetchLike = typeof globalThis.fetch;

/** 通过 REST JSON Envelope 访问疆诚记忆服务。 */
export class RemoteMemoryClient {
  private readonly baseUrl: string;
  private readonly apiKey?: string;
  private readonly timeoutMs: number;
  private readonly fetchImpl: FetchLike;
  private closed = false;

  constructor(options: RemoteMemoryClientOptions) {
    if (!options.baseUrl.trim()) {
      throw new JiangchengMemoryError("invalid_argument", "baseUrl 不能为空");
    }
    this.baseUrl = options.baseUrl.replace(/\/+$/, "");
    this.apiKey = options.apiKey;
    this.timeoutMs = options.timeoutMs ?? 30_000;
    this.fetchImpl = options.fetch ?? globalThis.fetch.bind(globalThis);
  }

  /** 写入一个原始 Fragment。 */
  observe(request: ObserveRequest, options?: ClientRequestOptions) {
    return this.execute<ObserveRequest>("/v1/memory/observe", request, options);
  }

  /** 提交一次完整 Runtime 状态转移产生的 Fragment 集合。 */
  commit(request: CommitRequest, options?: ClientRequestOptions) {
    return this.execute<CommitRequest>("/v1/memory/commit", request, options);
  }

  /** 执行有界 Recall。 */
  recall(request: RecallRequest, options?: ClientRequestOptions) {
    return this.execute<RecallRequest>("/v1/memory/recall", request, options);
  }

  /** 读取当前授权 Scope 的统计信息。 */
  stats(request: StatsRequest, options?: ClientRequestOptions) {
    return this.execute<StatsRequest>("/v1/memory/stats", request, options);
  }

  /** 追加纠正记录。 */
  correct(request: MutationRequest, options?: ClientRequestOptions) {
    return this.execute<MutationRequest>("/v1/memory/correct", request, options);
  }

  /** 追加失效记录，不覆盖原始 Fragment。 */
  forget(request: MutationRequest, options?: ClientRequestOptions) {
    return this.execute<MutationRequest>("/v1/memory/forget", request, options);
  }

  /** 返回 Recall 的结构化证据追踪。 */
  explain(request: ExplainRequest, options?: ClientRequestOptions) {
    return this.execute<ExplainRequest>("/v1/memory/explain", request, options);
  }

  /** 释放客户端资源；HTTP 模式当前没有需要关闭的连接池。 */
  close(): void {
    this.closed = true;
  }

  private async execute<TRequest extends MemoryRequest<unknown>>(
    path: string,
    request: TRequest,
    options?: ClientRequestOptions,
  ): Promise<MemoryResponse> {
    if (this.closed) {
      throw new JiangchengMemoryError("internal", "客户端已经关闭");
    }
    const timeoutController = this.timeoutMs > 0 ? new AbortController() : undefined;
    const signal = combineSignals(options?.signal, timeoutController?.signal);
    const timer = timeoutController
      ? setTimeout(() => timeoutController.abort(), this.timeoutMs)
      : undefined;
    try {
      const headers: Record<string, string> = {
        "content-type": "application/json",
        accept: "application/json",
      };
      if (this.apiKey) {
        headers.authorization = `Bearer ${this.apiKey}`;
      }
      const response = await this.fetchImpl(`${this.baseUrl}${path}`, {
        method: "POST",
        headers,
        body: JSON.stringify(request),
        signal,
      });
      const body = await parseJson(response);
      if (!response.ok) {
        throw fromHttpError(response.status, body, request.request_id);
      }
      return body as MemoryResponse;
    } catch (error) {
      if (error instanceof JiangchengMemoryError) {
        throw error;
      }
      if (isAbortError(error)) {
        const timedOut = timeoutController?.signal.aborted && !options?.signal?.aborted;
        throw new JiangchengMemoryError(
          timedOut ? "deadline_exceeded" : "cancelled",
          timedOut ? "请求超过客户端 deadline" : "请求已取消",
          { requestId: request.request_id },
        );
      }
      throw new JiangchengMemoryError("transport", "无法连接疆诚记忆服务", {
        requestId: request.request_id,
        details: error,
      });
    } finally {
      if (timer) {
        clearTimeout(timer);
      }
    }
  }
}

function combineSignals(...signals: (AbortSignal | undefined)[]): AbortSignal | undefined {
  const available = signals.filter((signal): signal is AbortSignal => signal !== undefined);
  if (available.length === 0) {
    return undefined;
  }
  if (available.length === 1) {
    return available[0];
  }
  const controller = new AbortController();
  const abort = () => controller.abort();
  for (const signal of available) {
    if (signal.aborted) {
      abort();
      break;
    }
    signal.addEventListener("abort", abort, { once: true });
  }
  return controller.signal;
}

async function parseJson(response: Response): Promise<unknown> {
  const text = await response.text();
  if (!text) {
    return {};
  }
  try {
    return JSON.parse(text) as unknown;
  } catch {
    return { message: text };
  }
}

function fromHttpError(status: number, body: unknown, fallbackRequestId: string) {
  const record = isRecord(body) ? body : {};
  const code = toErrorCode(record.code, status);
  return new JiangchengMemoryError(code, stringValue(record.message) ?? `HTTP ${status}`, {
    requestId: stringValue(record.request_id) ?? fallbackRequestId,
    details: body,
    status,
  });
}

function toErrorCode(value: unknown, status: number): JiangchengMemoryErrorCode {
  if (
    value === "invalid_argument" ||
    value === "permission_denied" ||
    value === "not_found" ||
    value === "conflict" ||
    value === "resource_exhausted" ||
    value === "deadline_exceeded" ||
    value === "unauthorized" ||
    value === "internal"
  ) {
    return value;
  }
  if (status === 401) return "unauthorized";
  if (status === 403) return "permission_denied";
  if (status === 404) return "not_found";
  if (status === 409) return "conflict";
  if (status === 408 || status === 504) return "deadline_exceeded";
  if (status === 429) return "resource_exhausted";
  if (status >= 400 && status < 500) return "invalid_argument";
  return "internal";
}

function isAbortError(error: unknown): boolean {
  return error instanceof DOMException && error.name === "AbortError";
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function stringValue(value: unknown): string | undefined {
  return typeof value === "string" ? value : undefined;
}
