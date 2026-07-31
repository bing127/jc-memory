import { JiangchengMemoryError } from "./errors.js";
import type {
  ClientRequestOptions,
  CommitRequest,
  ExplainRequest,
  MemoryRequest,
  MemoryResponse,
  MutationRequest,
  ObserveRequest,
  RecallRequest,
  StatsRequest,
} from "./types.js";

/** napi-rs 或测试实现提供的安全异步 Rust 命令门面。 */
export type EmbeddedExecutor = (
  request: MemoryRequest<unknown>,
  signal?: AbortSignal,
) => Promise<MemoryResponse>;

/** napi-rs 导出的最小 JSON 边界，不把 N-API 类型泄漏到业务客户端。 */
export interface NapiMemoryEngine {
  execute(requestJson: string, signal?: AbortSignal): Promise<string>;
  close(): Promise<void> | void;
}

/** 把 napi-rs JSON 方法适配为类型化 Embedded 客户端。 */
export function createNapiEmbeddedClient(engine: NapiMemoryEngine): EmbeddedMemoryClient {
  return new EmbeddedMemoryClient(
    async (request, signal) => JSON.parse(await engine.execute(JSON.stringify(request), signal)) as MemoryResponse,
    () => engine.close(),
  );
}

/** 复用 Rust Core CommandBus 的进程内客户端。 */
export class EmbeddedMemoryClient {
  private closed = false;

  constructor(
    private readonly executor: EmbeddedExecutor,
    private readonly onClose?: () => void | Promise<void>,
  ) {}

  /** 写入一个原始 Fragment。 */
  observe(request: ObserveRequest, options?: ClientRequestOptions) {
    return this.execute(request, options);
  }

  /** 提交一次完整状态转移的 Fragment 集合。 */
  commit(request: CommitRequest, options?: ClientRequestOptions) {
    return this.execute(request, options);
  }

  /** 执行有界 Recall。 */
  recall(request: RecallRequest, options?: ClientRequestOptions) {
    return this.execute(request, options);
  }

  /** 读取当前授权 Scope 的统计信息。 */
  stats(request: StatsRequest, options?: ClientRequestOptions) {
    return this.execute(request, options);
  }

  /** 追加纠正记录。 */
  correct(request: MutationRequest, options?: ClientRequestOptions) {
    return this.execute(request, options);
  }

  /** 追加失效记录。 */
  forget(request: MutationRequest, options?: ClientRequestOptions) {
    return this.execute(request, options);
  }

  /** 返回 Recall 的结构化证据追踪。 */
  explain(request: ExplainRequest, options?: ClientRequestOptions) {
    return this.execute(request, options);
  }

  /** 关闭 Rust 引擎句柄，并阻止后续请求进入已释放的边界。 */
  async close(): Promise<void> {
    if (this.closed) {
      return;
    }
    this.closed = true;
    await this.onClose?.();
  }

  private async execute(
    request: MemoryRequest<unknown>,
    options?: ClientRequestOptions,
  ): Promise<MemoryResponse> {
    if (this.closed) {
      throw new JiangchengMemoryError("internal", "客户端已经关闭", {
        requestId: request.request_id,
      });
    }
    try {
      return await this.executor(request, options?.signal);
    } catch (error) {
      if (error instanceof JiangchengMemoryError) {
        throw error;
      }
      throw new JiangchengMemoryError("internal", "嵌入式命令执行失败", {
        requestId: request.request_id,
        details: error,
      });
    }
  }
}
