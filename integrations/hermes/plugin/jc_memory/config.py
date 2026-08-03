from __future__ import annotations

from dataclasses import dataclass, field
import math
import os
from typing import Mapping
from urllib.parse import urlparse
from uuid import UUID


_SCOPE_FIELDS = ("namespace_id", "tenant_id", "user_id", "agent_id", "project_id")
_MEMORY_FIELDS = {
    "base_url",
    "api_key",  # 兼容旧配置，但永远不会从 YAML 读取 Secret。
    "namespace_id",
    "tenant_id",
    "user_id",
    "agent_id",
    "project_id",
    "timeout_seconds",
    "recall",
}
_RECALL_FIELDS = {
    "max_seeds",
    "max_neighbors",
    "max_hops",
    "max_nodes",
    "timeout_ms",
}


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
    def load(cls) -> "JcMemoryConfig":
        """从 Hermes 配置与进程环境加载运行配置。"""
        from hermes_cli.config import load_config

        return cls.from_sources(load_config(), os.environ)

    @classmethod
    def from_sources(
        cls,
        config: Mapping[str, object],
        environment: Mapping[str, str],
    ) -> "JcMemoryConfig":
        memory = _mapping(config.get("memory"), "memory")
        values = _mapping(memory.get("jc_memory"), "memory.jc_memory")
        _reject_unknown(values, _MEMORY_FIELDS, "memory.jc_memory")

        base_url = _safe_base_url(values.get("base_url"))
        api_key = _required_string(environment.get("JC_MEMORY_API_KEY"), "JC_MEMORY_API_KEY")

        access_scope: dict[str, str | None] = {}
        for name in _SCOPE_FIELDS:
            required = name == "namespace_id"
            access_scope[name] = _uuid_string(values.get(name), name, required=required)

        try:
            timeout_seconds = float(values["timeout_seconds"])
        except (KeyError, TypeError, ValueError) as error:
            raise JcMemoryConfigError("timeout_seconds 必须是数字") from error
        if not math.isfinite(timeout_seconds) or not 0.25 <= timeout_seconds <= 300:
            raise JcMemoryConfigError("timeout_seconds 必须在 0.25..=300 秒范围内")

        recall = _mapping(values.get("recall"), "recall")
        _reject_unknown(recall, _RECALL_FIELDS, "recall")
        required_limits = {
            "max_seeds": (1, 64),
            "max_neighbors": (1, 128),
            "max_hops": (1, 16),
            "max_nodes": (1, 1024),
            "timeout_ms": (1, 60_000),
        }
        recall_limits: dict[str, int] = {}
        for name, (minimum, maximum) in required_limits.items():
            try:
                value = int(recall[name])
            except (KeyError, TypeError, ValueError) as error:
                raise JcMemoryConfigError(f"recall.{name} 必须是整数") from error
            if value < minimum or value > maximum:
                raise JcMemoryConfigError(
                    f"recall.{name} 必须在 {minimum}..={maximum} 范围内"
                )
            recall_limits[name] = value

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


def _safe_base_url(value: object) -> str:
    text = _required_string(value, "base_url")
    parsed = urlparse(text)
    if (
        parsed.scheme not in {"http", "https"}
        or not parsed.netloc
        or parsed.username is not None
        or parsed.password is not None
        or parsed.path not in {"", "/"}
        or parsed.query
        or parsed.fragment
    ):
        raise JcMemoryConfigError("base_url 必须是无凭据的 HTTP(S) origin")
    return text.rstrip("/")


def _reject_unknown(values: Mapping[str, object], allowed: set[str], name: str) -> None:
    unknown = sorted(set(values) - allowed)
    if unknown:
        raise JcMemoryConfigError(f"{name} 包含未知字段: {', '.join(unknown)}")


def _uuid_string(value: object, name: str, *, required: bool) -> str | None:
    if value is None and not required:
        return None
    text = _required_string(value, name)
    try:
        UUID(text)
    except ValueError as error:
        raise JcMemoryConfigError(f"{name} 必须是 UUID 或 null") from error
    return text
