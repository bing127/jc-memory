"""Python SDK 的稳定错误分类。"""

from typing import Any


ErrorCode = str


class JiangchengMemoryError(RuntimeError):
    """远程和嵌入式模式共用的可检查错误。"""

    def __init__(
        self,
        code: ErrorCode,
        message: str,
        *,
        request_id: str | None = None,
        details: Any = None,
        status: int | None = None,
    ) -> None:
        super().__init__(message)
        self.code = code
        self.request_id = request_id
        self.details = details
        self.status = status

    def __str__(self) -> str:
        suffix = f" (request_id={self.request_id})" if self.request_id else ""
        return f"{self.code}: {super().__str__()}{suffix}"


def code_for_status(status: int, value: object) -> ErrorCode:
    if isinstance(value, str) and value:
        return value
    return {
        401: "unauthorized",
        403: "permission_denied",
        404: "not_found",
        408: "deadline_exceeded",
        409: "conflict",
        429: "resource_exhausted",
        504: "deadline_exceeded",
    }.get(status, "invalid_argument" if 400 <= status < 500 else "internal")
