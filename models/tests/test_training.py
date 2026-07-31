import json
from pathlib import Path

import pytest

from models.training.checkpoint import CheckpointError, load_checkpoint, save_checkpoint
from models.training.config import ConfigError, TrainingConfig
from models.training.data import TrainingExample, stable_split
from models.training.model import MultiTaskModel
from models.training.train import train_model


def fixture_examples() -> list[TrainingExample]:
    return [
        TrainingExample(
            sample_id=f"fixture-{index:02d}",
            features=(float(index), 1.0, float(index % 2), 0.5),
            router_label=index % 3,
            importance=(index % 10) / 10.0,
            contradiction_label=index % 2,
        )
        for index in range(12)
    ]


def test_config_supports_yaml_json_and_rejects_sensitive_or_absolute_values(
    tmp_path: Path,
) -> None:
    config = TrainingConfig(
        seed=17,
        input_dim=4,
        hidden_dim=8,
        router_classes=3,
        batch_size=3,
        epochs=2,
        learning_rate=0.01,
        validation_ratio=0.25,
        checkpoint_path="artifacts/checkpoint.pt",
    )
    yaml_path = tmp_path / "config.yaml"
    json_path = tmp_path / "config.json"
    config.to_yaml(yaml_path)
    config.to_json(json_path)

    assert TrainingConfig.from_yaml(yaml_path) == config
    assert TrainingConfig.from_json(json_path) == config
    assert "checkpoint_path" in json.loads(json_path.read_text())

    with pytest.raises(ConfigError, match="absolute"):
        TrainingConfig.from_mapping(
            {**config.to_mapping(), "checkpoint_path": "/tmp/model.pt"}
        )
    with pytest.raises(ConfigError, match="sensitive"):
        TrainingConfig.from_mapping(
            {**config.to_mapping(), "api_token": "do-not-store"}
        )


def test_stable_split_uses_sample_id_and_seed_not_input_order() -> None:
    examples = fixture_examples()
    train_a, valid_a = stable_split(examples, validation_ratio=0.25, seed=17)
    train_b, valid_b = stable_split(
        list(reversed(examples)), validation_ratio=0.25, seed=17
    )
    _train_c, valid_c = stable_split(examples, validation_ratio=0.25, seed=18)

    assert {item.sample_id for item in train_a} == {item.sample_id for item in train_b}
    assert {item.sample_id for item in valid_a} == {item.sample_id for item in valid_b}
    assert {item.sample_id for item in valid_a} != {item.sample_id for item in valid_c}
    assert len(train_a) + len(valid_a) == len(examples)


def test_cpu_training_is_reproducible_and_returns_all_heads() -> None:
    config = TrainingConfig(
        seed=17,
        input_dim=4,
        hidden_dim=8,
        router_classes=3,
        batch_size=3,
        epochs=2,
        learning_rate=0.01,
        validation_ratio=0.25,
        checkpoint_path="artifacts/checkpoint.pt",
    )
    first = train_model(config, fixture_examples(), git_commit="fixture-commit")
    second = train_model(config, fixture_examples(), git_commit="fixture-commit")

    assert first.metrics == second.metrics
    assert first.parameter_digest == second.parameter_digest
    assert set(first.model(first.validation_batch)) == {
        "router_logits",
        "importance",
        "contradiction_logits",
    }


def test_checkpoint_round_trip_and_tamper_rejection(tmp_path: Path) -> None:
    config = TrainingConfig(
        seed=17,
        input_dim=4,
        hidden_dim=8,
        router_classes=3,
        batch_size=3,
        epochs=2,
        learning_rate=0.01,
        validation_ratio=0.25,
        checkpoint_path="artifacts/checkpoint.pt",
    )
    result = train_model(config, fixture_examples(), git_commit="fixture-commit")
    checkpoint_path = tmp_path / "checkpoint.pt"
    metadata = save_checkpoint(
        checkpoint_path,
        result.model,
        result.optimizer,
        config,
        result.metrics,
        git_commit="fixture-commit",
    )
    restored = MultiTaskModel.from_config(config)
    loaded = load_checkpoint(checkpoint_path, restored)

    assert metadata.checksum_sha256 == loaded.checksum_sha256
    assert loaded.git_commit == "fixture-commit"
    assert restored.state_dict().keys() == result.model.state_dict().keys()

    checkpoint_path.write_bytes(checkpoint_path.read_bytes() + b"tampered")
    with pytest.raises(CheckpointError, match="校验和"):
        load_checkpoint(checkpoint_path, MultiTaskModel.from_config(config))
