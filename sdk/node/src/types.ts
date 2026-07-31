/** 访问范围；服务端授权 Grant 不会从客户端传入。 */
export interface AccessScope {
  namespace_id: string;
  tenant_id: string | null;
  user_id: string | null;
  agent_id: string | null;
  project_id: string | null;
}

/** 命令 Envelope 中的命令部分。 */
export interface CommandEnvelope<TPayload> {
  command: string;
  payload: TPayload;
}

/** 所有远程命令共享的请求 Envelope。 */
export interface MemoryRequest<TPayload = unknown> {
  api_version: string;
  request_id: string;
  access_scope: AccessScope;
  command: CommandEnvelope<TPayload>;
}

/** Recall 的有界执行预算。 */
export interface RecallLimits {
  max_seeds: number;
  max_neighbors: number;
  max_hops: number;
  max_nodes: number;
  timeout_ms: number;
}

/** Recall 请求的 payload。 */
export interface RecallPayload {
  mode: string;
  text: string;
  entities: string[];
  at_time: string | null;
  limits: RecallLimits;
}

/** Observe 请求的 payload。 */
export interface ObservePayload {
  fragment: Record<string, unknown>;
}

/** Commit 请求的 payload。 */
export interface CommitPayload {
  fragments: Record<string, unknown>[];
}

/** Correct/Forget 请求的 payload。 */
export interface MutationPayload {
  target_fragment_id: string;
  content: Record<string, unknown>;
  source: Record<string, unknown>;
  perspective: string;
  occurred_at: string;
  verification_method: string;
  reason: string;
}

/** 统一响应 Envelope。 */
export interface MemoryResponse<TPayload = unknown> {
  api_version: string;
  request_id: string;
  response: {
    response: string;
    payload: TPayload;
  };
}

export type RecallRequest = MemoryRequest<RecallPayload>;
export type ObserveRequest = MemoryRequest<ObservePayload>;
export type CommitRequest = MemoryRequest<CommitPayload>;
export type MutationRequest = MemoryRequest<MutationPayload>;
export type StatsRequest = MemoryRequest<Record<string, never>>;
export type ExplainRequest = RecallRequest;

export interface ClientRequestOptions {
  signal?: AbortSignal;
}

export interface RemoteMemoryClientOptions {
  /** 服务根地址，例如 http://127.0.0.1:8080。 */
  baseUrl: string;
  /** 使用 Bearer 方案发送的 API Key。 */
  apiKey?: string;
  /** 默认请求超时；设置为 0 可关闭自动超时。 */
  timeoutMs?: number;
  /** 测试或运行时注入的 fetch 实现。 */
  fetch?: typeof globalThis.fetch;
}
