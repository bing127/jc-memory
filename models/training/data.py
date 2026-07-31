"""可重复的训练样本、稳定切分和 PyTorch Dataset。"""

from __future__ import annotations

from collections.abc import Sequence
from dataclasses import dataclass
from hashlib import sha256

import torch
from torch.utils.data import Dataset


@dataclass(frozen=True)
class TrainingExample:
    """不包含原始正文的数值化训练样本。"""

    sample_id: str
    features: tuple[float, ...]
    router_label: int
    importance: float
    contradiction_label: int

    def __post_init__(self) -> None:
        if not self.sample_id.strip():
            raise ValueError("sample_id 不能为空")
        if not self.features:
            raise ValueError("features 不能为空")
        if self.router_label < 0 or self.contradiction_label not in (0, 1):
            raise ValueError("标签超出允许范围")
        if not 0.0 <= self.importance <= 1.0:
            raise ValueError("importance 必须在 0 和 1 之间")


def stable_split(
    examples: Sequence[TrainingExample], validation_ratio: float, seed: int
) -> tuple[list[TrainingExample], list[TrainingExample]]:
    """按 seed 和 sample ID 的 SHA-256 排序，得到与输入顺序无关的切分。"""
    if len(examples) < 2:
        raise ValueError("训练样本至少需要两个样本")
    if not 0.0 < validation_ratio < 1.0:
        raise ValueError("validation_ratio 必须在 0 和 1 之间")
    ids = [example.sample_id for example in examples]
    if len(ids) != len(set(ids)):
        raise ValueError("sample_id 必须唯一")
    ordered = sorted(
        examples, key=lambda item: sha256(f"{seed}:{item.sample_id}".encode()).digest()
    )
    validation_count = max(
        1, min(len(ordered) - 1, int(len(ordered) * validation_ratio))
    )
    validation = sorted(ordered[:validation_count], key=lambda item: item.sample_id)
    train = sorted(ordered[validation_count:], key=lambda item: item.sample_id)
    return train, validation


class FeatureDataset(Dataset[dict[str, object]]):
    """把数值 Fixture 转为带标签的 Tensor 字典。"""

    def __init__(self, examples: Sequence[TrainingExample]) -> None:
        self._examples = tuple(examples)
        if not self._examples:
            raise ValueError("Dataset 不能为空")

    def __len__(self) -> int:
        return len(self._examples)

    def __getitem__(self, index: int) -> dict[str, object]:
        example = self._examples[index]
        return {
            "sample_id": example.sample_id,
            "features": torch.tensor(example.features, dtype=torch.float32),
            "router_label": torch.tensor(example.router_label, dtype=torch.long),
            "importance": torch.tensor(example.importance, dtype=torch.float32),
            "contradiction_label": torch.tensor(
                example.contradiction_label, dtype=torch.long
            ),
        }
