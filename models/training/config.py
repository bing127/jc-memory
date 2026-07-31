"""训练配置的严格解析与敏感字段防护。"""

from __future__ import annotations

import json
import re
from collections.abc import Mapping
from dataclasses import asdict, dataclass
from pathlib import Path, PureWindowsPath
from typing import Any

import yaml


class ConfigError(ValueError):
    """训练配置不满足稳定性或安全约束。"""


_SENSITIVE_KEY_PARTS = ("token", "secret", "password", "api_key", "credential")
_WINDOWS_ABSOLUTE = re.compile(r"^[A-Za-z]:[\\/]")


@dataclass(frozen=True)
class TrainingConfig:
    """Task 1 的版本化训练参数。"""

    seed: int
    input_dim: int
    hidden_dim: int
    router_classes: int
    batch_size: int
    epochs: int
    learning_rate: float
    validation_ratio: float
    checkpoint_path: str
    schema_version: str = "m6.training.v1"
    device: str = "cpu"
    num_workers: int = 0

    @classmethod
    def from_mapping(cls, values: Mapping[str, Any]) -> TrainingConfig:
        """从字典解析配置，并拒绝未知、敏感和绝对路径字段。"""
        _reject_sensitive(values)
        expected = set(cls.__dataclass_fields__)
        unknown = sorted(set(values) - expected)
        if unknown:
            raise ConfigError(f"未知配置字段: {', '.join(unknown)}")
        required = {
            "seed",
            "input_dim",
            "hidden_dim",
            "router_classes",
            "batch_size",
            "epochs",
            "learning_rate",
            "validation_ratio",
            "checkpoint_path",
        }
        missing = sorted(required - set(values))
        if missing:
            raise ConfigError(f"缺少配置字段: {', '.join(missing)}")
        try:
            config = cls(**dict(values))
        except (TypeError, ValueError) as error:
            raise ConfigError(f"配置类型错误: {error}") from error
        config.validate()
        return config

    @classmethod
    def from_json(cls, path: str | Path) -> TrainingConfig:
        """读取 JSON 配置。"""
        return cls.from_mapping(json.loads(Path(path).read_text(encoding="utf-8")))

    @classmethod
    def from_yaml(cls, path: str | Path) -> TrainingConfig:
        """读取 YAML 配置。"""
        loaded = yaml.safe_load(Path(path).read_text(encoding="utf-8"))
        if not isinstance(loaded, Mapping):
            raise ConfigError("YAML 顶层必须是对象")
        return cls.from_mapping(loaded)

    def to_mapping(self) -> dict[str, Any]:
        """返回不含运行时路径的可序列化配置。"""
        return asdict(self)

    def to_json(self, path: str | Path) -> None:
        """以稳定排序写入 JSON。"""
        Path(path).write_text(self.canonical_json() + "\n", encoding="utf-8")

    def to_yaml(self, path: str | Path) -> None:
        """以固定键顺序写入 YAML。"""
        Path(path).write_text(
            yaml.safe_dump(self.to_mapping(), allow_unicode=True, sort_keys=False),
            encoding="utf-8",
        )

    def canonical_json(self) -> str:
        """返回用于 Checkpoint 摘要的 canonical JSON。"""
        return json.dumps(
            self.to_mapping(), ensure_ascii=False, sort_keys=True, separators=(",", ":")
        )

    def validate(self) -> None:
        """校验维度、训练范围、设备和相对路径约束。"""
        non_negative_ints = {"seed": self.seed}
        for name, value in non_negative_ints.items():
            if isinstance(value, bool) or not isinstance(value, int) or value < 0:
                raise ConfigError(f"{name} 必须是非负整数")
        positive_ints = {
            "input_dim": self.input_dim,
            "hidden_dim": self.hidden_dim,
            "router_classes": self.router_classes,
            "batch_size": self.batch_size,
            "epochs": self.epochs,
        }
        for name, value in positive_ints.items():
            if isinstance(value, bool) or not isinstance(value, int) or value <= 0:
                raise ConfigError(f"{name} 必须是正整数")
        if not isinstance(self.num_workers, int) or self.num_workers < 0:
            raise ConfigError("num_workers 必须是非负整数")
        if not isinstance(self.learning_rate, (int, float)) or self.learning_rate <= 0:
            raise ConfigError("learning_rate 必须是正数")
        if not 0.0 < self.validation_ratio < 1.0:
            raise ConfigError("validation_ratio 必须在 0 和 1 之间")
        if self.device != "cpu":
            raise ConfigError("Task 1 只允许使用 cpu 设备")
        if not self.checkpoint_path or _is_absolute(self.checkpoint_path):
            raise ConfigError(
                "checkpoint_path 必须是非空相对路径，不能使用 absolute path"
            )
        if not self.schema_version.strip():
            raise ConfigError("schema_version 不能为空")


def _is_absolute(value: str) -> bool:
    return (
        Path(value).is_absolute()
        or PureWindowsPath(value).is_absolute()
        or bool(_WINDOWS_ABSOLUTE.match(value))
    )


def _reject_sensitive(value: Any, path: str = "") -> None:
    if isinstance(value, Mapping):
        for key, nested in value.items():
            key_text = str(key).lower()
            if any(part in key_text for part in _SENSITIVE_KEY_PARTS):
                raise ConfigError(f"sensitive 字段被禁止: {path + str(key)}")
            _reject_sensitive(nested, f"{path}{key}.")
    elif isinstance(value, (list, tuple)):
        for index, nested in enumerate(value):
            _reject_sensitive(nested, f"{path}{index}.")
