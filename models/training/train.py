"""固定随机性并运行 CPU 多任务训练 Smoke。"""

from __future__ import annotations

import random
from collections.abc import Sequence
from dataclasses import dataclass
from typing import Any

import numpy as np
import torch
from torch import Tensor
from torch.utils.data import DataLoader

from .config import TrainingConfig
from .data import FeatureDataset, TrainingExample, stable_split
from .model import MultiTaskModel, parameter_digest


@dataclass(frozen=True)
class TrainingResult:
    """训练后的模型、优化器、验证批次和可审计指标。"""

    model: MultiTaskModel
    optimizer: torch.optim.Optimizer
    metrics: tuple[dict[str, float], ...]
    parameter_digest: str
    validation_batch: Tensor
    git_commit: str


def train_model(
    config: TrainingConfig,
    examples: Sequence[TrainingExample],
    git_commit: str,
) -> TrainingResult:
    """执行固定 CPU 训练并返回结果。"""
    config.validate()
    if not examples:
        raise ValueError("训练样本不能为空")
    for example in examples:
        if len(example.features) != config.input_dim:
            raise ValueError(f"样本 {example.sample_id} 的特征维度不匹配")
        if example.router_label >= config.router_classes:
            raise ValueError(f"样本 {example.sample_id} 的 Router 标签超出范围")
    _seed_everything(config.seed)
    train_examples, validation_examples = stable_split(
        examples, config.validation_ratio, config.seed
    )
    model = MultiTaskModel.from_config(config).to("cpu")
    optimizer = torch.optim.Adam(model.parameters(), lr=config.learning_rate)
    train_loader = _loader(FeatureDataset(train_examples), config, shuffle=True)
    validation_loader = _loader(
        FeatureDataset(validation_examples), config, shuffle=False
    )
    metrics: list[dict[str, float]] = []
    for epoch in range(config.epochs):
        model.train()
        total_loss = 0.0
        total_samples = 0
        for batch in train_loader:
            outputs = model(batch["features"])
            loss = _loss(outputs, batch)
            optimizer.zero_grad(set_to_none=True)
            loss.backward()
            optimizer.step()
            count = int(batch["features"].shape[0])
            total_loss += float(loss.detach()) * count
            total_samples += count
        validation_loss = _validation_loss(model, validation_loader)
        metrics.append(
            {
                "epoch": float(epoch + 1),
                "train_loss": total_loss / total_samples,
                "validation_loss": validation_loss,
                "samples": float(total_samples),
            }
        )
    validation_batch = torch.stack(
        [
            torch.tensor(item.features, dtype=torch.float32)
            for item in validation_examples
        ]
    )
    return TrainingResult(
        model=model,
        optimizer=optimizer,
        metrics=tuple(metrics),
        parameter_digest=parameter_digest(model),
        validation_batch=validation_batch,
        git_commit=git_commit,
    )


def _seed_everything(seed: int) -> None:
    random.seed(seed)
    np.random.seed(seed)
    torch.manual_seed(seed)
    torch.use_deterministic_algorithms(True)


def _loader(
    dataset: FeatureDataset, config: TrainingConfig, shuffle: bool
) -> DataLoader:
    generator = torch.Generator(device="cpu")
    generator.manual_seed(config.seed)
    return DataLoader(
        dataset,
        batch_size=config.batch_size,
        shuffle=shuffle,
        num_workers=config.num_workers,
        generator=generator,
        worker_init_fn=_worker_seed,
    )


def _worker_seed(worker_id: int) -> None:
    seed = torch.initial_seed() % (2**32)
    random.seed(seed + worker_id)
    np.random.seed(seed + worker_id)


def _loss(outputs: dict[str, Tensor], batch: dict[str, Any]) -> Tensor:
    return (
        torch.nn.functional.cross_entropy(
            outputs["router_logits"], batch["router_label"]
        )
        + torch.nn.functional.mse_loss(outputs["importance"], batch["importance"])
        + torch.nn.functional.cross_entropy(
            outputs["contradiction_logits"], batch["contradiction_label"]
        )
    )


def _validation_loss(model: MultiTaskModel, loader: DataLoader) -> float:
    model.eval()
    total_loss = 0.0
    total_samples = 0
    with torch.no_grad():
        for batch in loader:
            loss = _loss(model(batch["features"]), batch)
            count = int(batch["features"].shape[0])
            total_loss += float(loss) * count
            total_samples += count
    return total_loss / total_samples
