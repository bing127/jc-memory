export type JiangchengMemoryErrorCode =
  | "invalid_argument"
  | "permission_denied"
  | "not_found"
  | "conflict"
  | "resource_exhausted"
  | "deadline_exceeded"
  | "unauthorized"
  | "cancelled"
  | "transport"
  | "internal";

/** 所有远程和嵌入式模式共用的稳定错误类型。 */
export class JiangchengMemoryError extends Error {
  readonly code: JiangchengMemoryErrorCode;
  readonly requestId?: string;
  readonly details?: unknown;
  readonly status?: number;

  constructor(
    code: JiangchengMemoryErrorCode,
    message: string,
    options: { requestId?: string; details?: unknown; status?: number } = {},
  ) {
    super(message);
    this.name = "JiangchengMemoryError";
    this.code = code;
    this.requestId = options.requestId;
    this.details = options.details;
    this.status = options.status;
  }
}
