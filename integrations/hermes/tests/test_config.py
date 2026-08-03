from __future__ import annotations

import pytest

from jc_memory.config import JcMemoryConfig, JcMemoryConfigError


NAMESPACE_ID = "00000000-0000-0000-0000-000000000001"
TENANT_ID = "00000000-0000-0000-0000-000000000002"


def memory_config(**overrides: object) -> dict[str, object]:
    values: dict[str, object] = {
        "base_url": "http://127.0.0.1:17891",
        "namespace_id": NAMESPACE_ID,
        "tenant_id": TENANT_ID,
        "timeout_seconds": 4.5,
        "recall": {
            "max_seeds": 5,
            "max_neighbors": 9,
            "max_hops": 2,
            "max_nodes": 40,
            "timeout_ms": 1500,
        },
    }
    values.update(overrides)
    return {"memory": {"jc_memory": values}}


def test_config_reads_secret_only_from_environment() -> None:
    config = JcMemoryConfig.from_sources(
        memory_config(api_key="yaml-secret-must-be-ignored"),
        {"JC_MEMORY_API_KEY": "runtime-secret"},
    )

    assert config.api_key == "runtime-secret"
    assert config.base_url == "http://127.0.0.1:17891"
    assert config.access_scope["namespace_id"] == NAMESPACE_ID
    assert config.access_scope["tenant_id"] == TENANT_ID
    assert config.timeout_seconds == 4.5
    assert config.recall_limits["max_nodes"] == 40
    assert "yaml-secret" not in repr(config)
    assert "runtime-secret" not in repr(config)


@pytest.mark.parametrize(
    ("config", "environment", "message"),
    [
        (memory_config(base_url=""), {"JC_MEMORY_API_KEY": "secret"}, "base_url"),
        (memory_config(namespace_id=""), {"JC_MEMORY_API_KEY": "secret"}, "namespace_id"),
        (memory_config(), {}, "JC_MEMORY_API_KEY"),
        (memory_config(tenant_id="not-a-uuid"), {"JC_MEMORY_API_KEY": "secret"}, "tenant_id"),
    ],
)
def test_config_rejects_missing_or_invalid_boundaries(
    config: dict[str, object], environment: dict[str, str], message: str
) -> None:
    with pytest.raises(JcMemoryConfigError, match=message):
        JcMemoryConfig.from_sources(config, environment)


@pytest.mark.parametrize(
    "base_url",
    [
        "",
        "ftp://memory.example.com",
        "https://user:pass@memory.example.com",
        "https://memory.example.com/v1",
    ],
)
def test_config_rejects_unsafe_base_urls(base_url: str) -> None:
    with pytest.raises(JcMemoryConfigError, match="base_url"):
        JcMemoryConfig.from_sources(
            memory_config(base_url=base_url),
            {"JC_MEMORY_API_KEY": "secret"},
        )


def test_config_rejects_unknown_fields_and_incomplete_or_out_of_range_limits() -> None:
    with pytest.raises(JcMemoryConfigError, match="未知字段"):
        JcMemoryConfig.from_sources(
            memory_config(unexpected=True),
            {"JC_MEMORY_API_KEY": "secret"},
        )

    for recall in [
        {"max_seeds": 0, "max_neighbors": 9, "max_hops": 2, "max_nodes": 40, "timeout_ms": 1500},
        {"max_seeds": 5, "max_neighbors": 9, "max_hops": 2, "max_nodes": 40, "timeout_ms": 0},
        {"max_seeds": 5, "max_neighbors": 9, "max_hops": 99, "max_nodes": 40, "timeout_ms": 1500},
        {"max_seeds": 5, "max_neighbors": 9, "max_hops": 2, "max_nodes": 40, "timeout_ms": 1500, "unknown": 1},
    ]:
        with pytest.raises(JcMemoryConfigError, match="recall"):
            JcMemoryConfig.from_sources(
                memory_config(recall=recall),
                {"JC_MEMORY_API_KEY": "secret"},
            )
