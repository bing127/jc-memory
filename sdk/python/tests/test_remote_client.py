import asyncio
import json
from pathlib import Path
from typing import Any

import httpx
import pytest

from jiangcheng_memory import AsyncRemoteMemoryClient, EmbeddedMemoryClient, RemoteMemoryClient
from jiangcheng_memory.errors import JiangchengMemoryError
from jiangcheng_memory.models import MemoryRequest


FIXTURE = Path(__file__).parents[2] / "contracts" / "fixtures" / "recall-exact.json"


def load_fixture() -> dict[str, Any]:
    return json.loads(FIXTURE.read_text(encoding="utf-8"))


def test_sync_client_returns_canonical_recall_packet() -> None:
    fixture = load_fixture()
    seen: dict[str, object] = {}

    def handler(request: httpx.Request) -> httpx.Response:
        seen["authorization"] = request.headers["authorization"]
        seen["json"] = json.loads(request.content)
        return httpx.Response(200, json=fixture["response"])

    client = RemoteMemoryClient(
        base_url="https://fixture.invalid",
        api_key="fixture",
        transport=httpx.MockTransport(handler),
    )
    try:
        assert client.recall(fixture["request"]) == fixture["response"]
        assert seen == {"authorization": "Bearer fixture", "json": fixture["request"]}
    finally:
        client.close()


@pytest.mark.asyncio
async def test_async_client_supports_cancellation_and_close() -> None:
    fixture = load_fixture()
    entered = asyncio.Event()
    release = asyncio.Event()

    async def handler(request: httpx.Request) -> httpx.Response:
        entered.set()
        await release.wait()
        return httpx.Response(200, json=fixture["response"])

    client = AsyncRemoteMemoryClient(
        base_url="https://fixture.invalid",
        transport=httpx.MockTransport(handler),
    )
    pending = asyncio.create_task(client.recall(fixture["request"]))
    await entered.wait()
    pending.cancel()
    with pytest.raises(asyncio.CancelledError):
        await pending
    await client.close()
    with pytest.raises(JiangchengMemoryError, match="客户端已经关闭"):
        await client.recall(fixture["request"])


@pytest.mark.asyncio
async def test_async_client_supports_context_manager() -> None:
    fixture = load_fixture()

    async def handler(_request: httpx.Request) -> httpx.Response:
        return httpx.Response(200, json=fixture["response"])

    async with AsyncRemoteMemoryClient(
        base_url="https://fixture.invalid",
        transport=httpx.MockTransport(handler),
    ) as client:
        assert await client.recall(fixture["request"]) == fixture["response"]


def test_http_error_maps_to_stable_code_and_request_id() -> None:
    fixture = load_fixture()

    def handler(_request: httpx.Request) -> httpx.Response:
        return httpx.Response(
            400,
            json={
                "code": "invalid_argument",
                "message": "request_id is invalid",
                "request_id": fixture["request"]["request_id"],
            },
        )

    client = RemoteMemoryClient(
        base_url="https://fixture.invalid",
        transport=httpx.MockTransport(handler),
    )
    with pytest.raises(JiangchengMemoryError) as raised:
        client.recall(fixture["request"])
    assert raised.value.code == "invalid_argument"
    assert raised.value.request_id == fixture["request"]["request_id"]


def test_embedded_client_exposes_the_same_command_methods() -> None:
    fixture = load_fixture()
    seen: list[str] = []

    def execute(request_json: str) -> str:
        request = json.loads(request_json)
        seen.append(request["command"]["command"])
        return json.dumps(fixture["response"])

    client = EmbeddedMemoryClient(execute)
    assert client.recall(fixture["request"]) == fixture["response"]
    stats_request: MemoryRequest = {
        "api_version": fixture["request"]["api_version"],
        "request_id": fixture["request"]["request_id"],
        "access_scope": fixture["request"]["access_scope"],
        "command": {"command": "stats", "payload": {}},
    }
    assert client.stats(stats_request) == fixture["response"]
    client.close()
    assert seen == ["recall", "stats"]
