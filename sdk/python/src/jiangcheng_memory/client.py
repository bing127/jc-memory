"""同步 HTTP 客户端。"""

from collections.abc import Mapping
from typing import Any

import httpx

from .errors import JiangchengMemoryError, code_for_status
from .models import MemoryRequest, MemoryResponse


class RemoteMemoryClient:
    """通过 REST JSON Envelope 访问疆诚记忆服务。"""

    def __init__(
        self,
        *,
        base_url: str,
        api_key: str | None = None,
        timeout: float = 30.0,
        transport: httpx.BaseTransport | None = None,
    ) -> None:
        if not base_url.strip():
            raise JiangchengMemoryError("invalid_argument", "base_url 不能为空")
        headers = {"accept": "application/json", "content-type": "application/json"}
        if api_key:
            headers["authorization"] = f"Bearer {api_key}"
        self._client = httpx.Client(
            base_url=base_url.rstrip("/"),
            headers=headers,
            timeout=timeout,
            transport=transport,
        )
        self._closed = False

    def observe(self, request: MemoryRequest) -> MemoryResponse:
        return self._execute("/v1/memory/observe", request)

    def commit(self, request: MemoryRequest) -> MemoryResponse:
        return self._execute("/v1/memory/commit", request)

    def recall(self, request: MemoryRequest) -> MemoryResponse:
        return self._execute("/v1/memory/recall", request)

    def stats(self, request: MemoryRequest) -> MemoryResponse:
        return self._execute("/v1/memory/stats", request)

    def correct(self, request: MemoryRequest) -> MemoryResponse:
        return self._execute("/v1/memory/correct", request)

    def forget(self, request: MemoryRequest) -> MemoryResponse:
        return self._execute("/v1/memory/forget", request)

    def explain(self, request: MemoryRequest) -> MemoryResponse:
        return self._execute("/v1/memory/explain", request)

    def close(self) -> None:
        if not self._closed:
            self._client.close()
            self._closed = True

    def __enter__(self) -> "RemoteMemoryClient":
        if self._closed:
            raise JiangchengMemoryError("internal", "客户端已经关闭")
        return self

    def __exit__(self, _exc_type: object, _exc: object, _traceback: object) -> None:
        self.close()

    def _execute(self, path: str, request: MemoryRequest) -> MemoryResponse:
        self._ensure_open(request)
        try:
            response = self._client.post(path, json=request)
        except httpx.TimeoutException as error:
            raise JiangchengMemoryError(
                "deadline_exceeded",
                "请求超过客户端 deadline",
                request_id=request["request_id"],
                details=error,
            ) from error
        except httpx.HTTPError as error:
            raise JiangchengMemoryError(
                "transport",
                "无法连接疆诚记忆服务",
                request_id=request["request_id"],
                details=error,
            ) from error
        return _decode_response(response, request["request_id"])

    def _ensure_open(self, request: MemoryRequest) -> None:
        if self._closed:
            raise JiangchengMemoryError("internal", "客户端已经关闭", request_id=request["request_id"])


def _decode_response(response: httpx.Response, fallback_request_id: str) -> MemoryResponse:
    try:
        body: object = response.json()
    except ValueError:
        body = {"message": response.text}
    if response.is_error:
        values = body if isinstance(body, Mapping) else {}
        message = values.get("message")
        raise JiangchengMemoryError(
            code_for_status(response.status_code, values.get("code")),
            str(message) if message else f"HTTP {response.status_code}",
            request_id=str(values.get("request_id") or fallback_request_id),
            details=body,
            status=response.status_code,
        )
    if not isinstance(body, dict):
        raise JiangchengMemoryError(
            "internal", "服务返回了非对象 JSON", request_id=fallback_request_id, details=body
        )
    return body  # type: ignore[return-value]
