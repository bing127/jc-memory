"""Python Embedded 门面；PyO3 原生模块只提供 JSON 执行边界。"""

import asyncio
import json
from collections.abc import Awaitable, Callable
from typing import Any, cast

from .errors import JiangchengMemoryError
from .models import MemoryRequest, MemoryResponse

SyncExecutor = Callable[[str], str]
AsyncExecutor = Callable[[str], Awaitable[str]]


class EmbeddedMemoryClient:
    """把 PyO3 的 JSON 方法适配为同步类型化客户端。"""

    def __init__(self, executor: SyncExecutor) -> None:
        self._executor = executor
        self._closed = False

    def execute(self, request: MemoryRequest) -> MemoryResponse:
        self._ensure_open(request)
        try:
            return _decode(self._executor(json.dumps(request, separators=(",", ":"))), request)
        except JiangchengMemoryError:
            raise
        except Exception as error:
            raise JiangchengMemoryError(
                "internal", "嵌入式命令执行失败", request_id=request["request_id"], details=error
            ) from error

    def observe(self, request: MemoryRequest) -> MemoryResponse:
        return self.execute(request)

    def commit(self, request: MemoryRequest) -> MemoryResponse:
        return self.execute(request)

    def recall(self, request: MemoryRequest) -> MemoryResponse:
        return self.execute(request)

    def stats(self, request: MemoryRequest) -> MemoryResponse:
        return self.execute(request)

    def correct(self, request: MemoryRequest) -> MemoryResponse:
        return self.execute(request)

    def forget(self, request: MemoryRequest) -> MemoryResponse:
        return self.execute(request)

    def explain(self, request: MemoryRequest) -> MemoryResponse:
        return self.execute(request)

    def close(self) -> None:
        self._closed = True

    def _ensure_open(self, request: MemoryRequest) -> None:
        if self._closed:
            raise JiangchengMemoryError("internal", "客户端已经关闭", request_id=request["request_id"])


class AsyncEmbeddedMemoryClient:
    """把 PyO3 的异步 JSON 方法适配为原生 await 客户端。"""

    def __init__(self, executor: AsyncExecutor) -> None:
        self._executor = executor
        self._closed = False

    async def execute(self, request: MemoryRequest) -> MemoryResponse:
        if self._closed:
            raise JiangchengMemoryError("internal", "客户端已经关闭", request_id=request["request_id"])
        try:
            value = await self._executor(json.dumps(request, separators=(",", ":")))
            return _decode(value, request)
        except asyncio.CancelledError:
            raise
        except JiangchengMemoryError:
            raise
        except Exception as error:
            raise JiangchengMemoryError(
                "internal", "嵌入式命令执行失败", request_id=request["request_id"], details=error
            ) from error

    async def observe(self, request: MemoryRequest) -> MemoryResponse:
        return await self.execute(request)

    async def commit(self, request: MemoryRequest) -> MemoryResponse:
        return await self.execute(request)

    async def recall(self, request: MemoryRequest) -> MemoryResponse:
        return await self.execute(request)

    async def stats(self, request: MemoryRequest) -> MemoryResponse:
        return await self.execute(request)

    async def correct(self, request: MemoryRequest) -> MemoryResponse:
        return await self.execute(request)

    async def forget(self, request: MemoryRequest) -> MemoryResponse:
        return await self.execute(request)

    async def explain(self, request: MemoryRequest) -> MemoryResponse:
        return await self.execute(request)

    async def close(self) -> None:
        self._closed = True


def _decode(value: str, request: MemoryRequest) -> MemoryResponse:
    result: Any = json.loads(value)
    if not isinstance(result, dict):
        raise JiangchengMemoryError("internal", "嵌入式响应不是对象", request_id=request["request_id"])
    return cast(MemoryResponse, result)
