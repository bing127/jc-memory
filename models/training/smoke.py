"""运行不含敏感数据的两 epoch CPU 训练 Smoke。"""

from __future__ import annotations

import argparse
import json
from pathlib import Path

from .checkpoint import save_checkpoint
from .config import TrainingConfig
from .data import TrainingExample
from .train import train_model


def smoke_examples() -> list[TrainingExample]:
    """返回版本化的合成数值样本。"""
    return [
        TrainingExample(
            sample_id=f"smoke-{index:02d}",
            features=(float(index), 1.0, float(index % 2), 0.5),
            router_label=index % 3,
            importance=(index % 10) / 10.0,
            contradiction_label=index % 2,
        )
        for index in range(12)
    ]


def main() -> None:
    """读取配置、训练模型并输出可审计摘要。"""
    parser = argparse.ArgumentParser(description="疆诚记忆 M6 Task 1 CPU Smoke")
    parser.add_argument("--config", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    config = (
        TrainingConfig.from_yaml(args.config)
        if args.config.suffix in {".yaml", ".yml"}
        else TrainingConfig.from_json(args.config)
    )
    result = train_model(config, smoke_examples(), git_commit="working-tree-smoke")
    checkpoint = save_checkpoint(
        args.output,
        result.model,
        result.optimizer,
        config,
        result.metrics,
        git_commit=result.git_commit,
    )
    print(
        json.dumps(
            {
                "schema_version": checkpoint.schema_version,
                "git_commit": checkpoint.git_commit,
                "epochs": len(checkpoint.metrics),
                "checkpoint_sha256": checkpoint.checksum_sha256,
            },
            ensure_ascii=False,
            sort_keys=True,
        )
    )


if __name__ == "__main__":
    main()
