"""带结构校验和内容 SHA-256 的 PyTorch Checkpoint。"""

from __future__ import annotations

import hashlib
import io
import os
import tempfile
from dataclasses import dataclass
from pathlib import Path
from typing import Any

import torch
from torch import nn

from .config import ConfigError, TrainingConfig

_CHECKSUM_MARKER = b"\nJC_MEMORY_CHECKPOINT_SHA256:"


class CheckpointError(ValueError):
    """Checkpoint 无法安全保存或恢复。"""


@dataclass(frozen=True)
class CheckpointMetadata:
    """已验证的 Checkpoint 元数据。"""

    checksum_sha256: str
    git_commit: str
    schema_version: str
    metrics: tuple[dict[str, float], ...]


def save_checkpoint(
    path: str | Path,
    model: nn.Module,
    optimizer: torch.optim.Optimizer,
    config: TrainingConfig,
    metrics: tuple[dict[str, float], ...] | list[dict[str, float]],
    git_commit: str,
) -> CheckpointMetadata:
    """原子保存模型、优化器、配置、指标和内容摘要。"""
    config.validate()
    if not git_commit.strip() or _contains_sensitive(git_commit):
        raise CheckpointError("git_commit 无效或包含敏感内容")
    payload: dict[str, Any] = {
        "schema_version": config.schema_version,
        "git_commit": git_commit,
        "config": config.to_mapping(),
        "metrics": [dict(item) for item in metrics],
        "model_state": model.state_dict(),
        "optimizer_state": optimizer.state_dict(),
    }
    destination = Path(path)
    destination.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(
        dir=destination.parent, prefix=f".{destination.name}.", delete=False
    ) as handle:
        temporary = Path(handle.name)
    try:
        serialized = io.BytesIO()
        torch.save(payload, serialized)
        content = serialized.getvalue()
        checksum = hashlib.sha256(content).hexdigest()
        temporary.write_bytes(
            content + _CHECKSUM_MARKER + checksum.encode("ascii") + b"\n"
        )
        os.replace(temporary, destination)
    finally:
        temporary.unlink(missing_ok=True)
    return CheckpointMetadata(
        checksum, git_commit, config.schema_version, tuple(payload["metrics"])
    )


def load_checkpoint(
    path: str | Path,
    model: nn.Module,
    optimizer: torch.optim.Optimizer | None = None,
) -> CheckpointMetadata:
    """验证摘要和结构后恢复模型及可选优化器。"""
    try:
        encoded = Path(path).read_bytes()
        content, footer = encoded.rsplit(_CHECKSUM_MARKER, 1)
        declared = footer.strip().decode("ascii")
        actual = hashlib.sha256(content).hexdigest()
        if declared != actual:
            raise CheckpointError("Checkpoint 校验和不一致")
        payload = torch.load(
            io.BytesIO(content), map_location="cpu", weights_only=False
        )
    except Exception as error:
        if isinstance(error, CheckpointError):
            raise
        raise CheckpointError(f"校验和或格式校验失败: {error}") from error
    if not isinstance(payload, dict):
        raise CheckpointError("Checkpoint 内容不是对象")
    try:
        config = TrainingConfig.from_mapping(payload["config"])
        if payload["schema_version"] != config.schema_version:
            raise CheckpointError("Checkpoint schema_version 不一致")
        model.load_state_dict(payload["model_state"], strict=True)
        if optimizer is not None:
            optimizer.load_state_dict(payload["optimizer_state"])
        metrics = tuple(dict(item) for item in payload["metrics"])
        git_commit = str(payload["git_commit"])
    except (KeyError, RuntimeError, TypeError, ConfigError, ValueError) as error:
        raise CheckpointError(f"Checkpoint 结构校验失败: {error}") from error
    return CheckpointMetadata(declared, git_commit, config.schema_version, metrics)


def _contains_sensitive(value: str) -> bool:
    lowered = value.lower()
    return any(part in lowered for part in ("token", "secret", "password", "api_key"))
