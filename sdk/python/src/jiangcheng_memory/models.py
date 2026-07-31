"""跨语言 JSON Envelope 的轻量类型别名。

原始 Fragment、Event 和 EvidenceRef 在 SDK 中按字典原样传递，避免客户端重写权威记录。
"""

from typing import Any, TypeAlias, TypedDict

JsonObject: TypeAlias = dict[str, Any]


class MemoryRequest(TypedDict):
    api_version: str
    request_id: str
    access_scope: JsonObject
    command: JsonObject


class MemoryResponse(TypedDict):
    api_version: str
    request_id: str
    response: JsonObject
