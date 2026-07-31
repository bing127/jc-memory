import json
from pathlib import Path

import pytest


native = pytest.importorskip("jiangcheng_memory._native")

FIXTURE = Path(__file__).parents[2] / "contracts" / "fixtures" / "recall-exact.json"


@pytest.mark.asyncio
async def test_pyo3_embedded_matches_recall_fixture(tmp_path: Path) -> None:
    fixture = json.loads(FIXTURE.read_text(encoding="utf-8"))
    engine = await native.open_memory_engine(str(tmp_path), "fixture-principal")
    try:
        response = json.loads(await engine.execute(json.dumps(fixture["request"])))
        assert response["api_version"] == fixture["response"]["api_version"]
        assert response["request_id"] == fixture["response"]["request_id"]
        assert response["response"]["response"] == "recall"
        payload = response["response"]["payload"]
        assert payload["intent"] == fixture["response"]["response"]["payload"]["intent"]
        assert isinstance(payload["query_id"], str)
        assert isinstance(payload["recommended_context"], str)
    finally:
        await engine.close()
