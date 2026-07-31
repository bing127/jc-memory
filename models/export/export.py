"""将 Task 1 PyTorch 多任务模型导出为可校验的 ONNX 产物。"""

from __future__ import annotations

import hashlib
import json
from pathlib import Path
from typing import Any

import torch
from torch import Tensor, nn

from ..training.config import TrainingConfig

OPSET_VERSION = 17
SCHEMA = "jc-memory://neural/multitask/v1"
OUTPUT_NAMES = ("router_logits", "importance", "contradiction_logits")


class _ExportWrapper(nn.Module):
    """把字典输出固定为 ONNX 的有序 Tuple。"""

    def __init__(self, model: nn.Module) -> None:
        super().__init__()
        self.model = model

    def forward(self, features: Tensor) -> tuple[Tensor, Tensor, Tensor]:
        outputs = self.model(features)
        return tuple(outputs[name] for name in OUTPUT_NAMES)  # type: ignore[return-value]


def export_model(
    model: nn.Module,
    config: TrainingConfig,
    model_path: str | Path,
    manifest_path: str | Path,
    *,
    model_id: str,
    version: str,
    training_data_digest: str = "synthetic-fixture-v1",
) -> dict[str, Any]:
    """导出动态 batch ONNX，并写入包含文件 SHA-256 的 Manifest。"""
    config.validate()
    if not model_id.strip() or not version.strip():
        raise ValueError("model_id 和 version 不能为空")
    if not training_data_digest.strip():
        raise ValueError("training_data_digest 不能为空")
    destination = Path(model_path)
    destination.parent.mkdir(parents=True, exist_ok=True)
    wrapper = _ExportWrapper(model.eval())
    dummy = torch.zeros((1, config.input_dim), dtype=torch.float32)
    torch.onnx.export(
        wrapper,
        dummy,
        destination,
        input_names=["features"],
        output_names=list(OUTPUT_NAMES),
        dynamic_axes={
            "features": {0: "batch"},
            "router_logits": {0: "batch"},
            "importance": {0: "batch"},
            "contradiction_logits": {0: "batch"},
        },
        opset_version=OPSET_VERSION,
        dynamo=False,
    )
    checksum = hashlib.sha256(destination.read_bytes()).hexdigest()
    manifest = {
        "model_id": model_id,
        "version": version,
        "license": "PRIVATE",
        "sha256": checksum,
        "schema": SCHEMA,
        "opset": OPSET_VERSION,
        "input_name": "features",
        "input_shape": [-1, config.input_dim],
        "outputs": [
            {"name": "router_logits", "shape": [-1, config.router_classes]},
            {"name": "importance", "shape": [-1, 1]},
            {"name": "contradiction_logits", "shape": [-1, 2]},
        ],
        "training_data_digest": training_data_digest,
        "device_support": ["cpu"],
        "max_batch_size": 256,
        "timeout_ms": 250,
    }
    manifest_file = Path(manifest_path)
    manifest_file.parent.mkdir(parents=True, exist_ok=True)
    manifest_file.write_text(
        json.dumps(manifest, ensure_ascii=False, sort_keys=True, indent=2) + "\n",
        encoding="utf-8",
    )
    return manifest
