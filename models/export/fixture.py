"""生成版本化的最小 ONNX 测试模型和 Manifest。"""

from __future__ import annotations

from pathlib import Path

import torch

from ..training.config import TrainingConfig
from ..training.model import MultiTaskModel
from .export import export_model


def main() -> None:
    root = Path(__file__).resolve().parents[2]
    config = TrainingConfig.from_yaml(root / "models/configs/smoke.yaml")
    torch.manual_seed(config.seed)
    model = MultiTaskModel.from_config(config)
    export_model(
        model,
        config,
        root / "models/onnx/fixtures/test-model.onnx",
        root / "models/manifests/test-model.json",
        model_id="jc-memory-fixture",
        version="0.1.0",
    )


if __name__ == "__main__":
    main()
