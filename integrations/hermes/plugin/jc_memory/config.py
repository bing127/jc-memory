from __future__ import annotations

from dataclasses import dataclass, field
from typing import Mapping
from uuid import UUID


_SCOPE_FIELDS = ("namespace_id", "tenant_id", "user_id", "agent_id", "project_id")


class JcMemoryConfigError(ValueError):
    """疆诚记忆配置无效。"""


@dataclass(frozen=True)
class JcMemoryConfig:
    base_url: str
    api_key: str = field(repr=False)
    access_scope: dict[str, str | None]
    timeout_seconds: float
    recall_limits: dict[str, int]

    @classmethod
    def from_sources(
        cls,
        config: Mapping[str, object],
        environment: Mapping[str, str],
    ) -> "JcMemoryConfig":
        memory = _mapping(config.get("memory"), "memory")
        values = _mapping(memory.get("jc_memory"), "memory.jc_memory")

        base_url = _required_string(values.get("base_url"), "base_url")
        api_key = _required_string(environment.get("JC_MEMORY_API_KEY"), "JC_MEMORY_API_KEY")

        access_scope: dict[str, str | None] = {}
        for name in _SCOPE_FIELDS:
            required = name == "namespace_id"
            access_scope[name] = _uuid_string(values.get(name), name, required=required)

        try:
            timeout_seconds = float(values["timeout_seconds"])
        except (KeyError, TypeError, ValueError) as error:
            raise JcMemoryConfigError("timeout_seconds 必须是数字") from error

        recall = _mapping(values.get("recall"), "recall")
        try:
            recall_limits = {name: int(value) for name, value in recall.items()}
        except (TypeError, ValueError) as error:
            raise JcMemoryConfigError("recall 限制必须是整数") from error

        return cls(
            base_url=base_url,
            api_key=api_key,
            access_scope=access_scope,
            timeout_seconds=timeout_seconds,
            recall_limits=recall_limits,
        )


def _mapping(value: object, name: str) -> Mapping[str, object]:
    if not isinstance(value, Mapping):
        raise JcMemoryConfigError(f"{name} 必须是映射")
    return value


def _required_string(value: object, name: str) -> str:
    if not isinstance(value, str) or not value.strip():
        raise JcMemoryConfigError(f"缺少 {name}")
    return value.strip()


def _uuid_string(value: object, name: str, *, required: bool) -> str | None:
    if value is None and not required:
        return None
    text = _required_string(value, name)
    try:
        UUID(text)
    except ValueError as error:
        raise JcMemoryConfigError(f"{name} 必须是 UUID 或 null") from error
    return text
