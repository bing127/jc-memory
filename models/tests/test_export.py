from pathlib import Path

import numpy as np
import pytest

from models.export.export import export_model
from models.export.verify import ExportVerificationError, verify_export
from models.training.config import TrainingConfig
from models.training.model import MultiTaskModel


def test_export_has_dynamic_batch_named_outputs_and_manifest_checksum(
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
    model_path = tmp_path / "test-model.onnx"
    manifest_path = tmp_path / "test-model.json"

    manifest = export_model(
        MultiTaskModel.from_config(config),
        config,
        model_path,
        manifest_path,
        model_id="jc-memory-fixture",
        version="0.1.0",
    )

    assert model_path.stat().st_size > 0
    assert manifest["input_name"] == "features"
    assert manifest["input_shape"] == [-1, 4]
    assert [output["name"] for output in manifest["outputs"]] == [
        "router_logits",
        "importance",
        "contradiction_logits",
    ]
    assert len(manifest["sha256"]) == 64


def test_export_matches_pytorch_and_rejects_shape_or_checksum_changes(
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
    model = MultiTaskModel.from_config(config).eval()
    model_path = tmp_path / "test-model.onnx"
    manifest_path = tmp_path / "test-model.json"
    export_model(
        model,
        config,
        model_path,
        manifest_path,
        model_id="jc-memory-fixture",
        version="0.1.0",
    )
    batch = np.arange(8, dtype=np.float32).reshape(2, 4) / 10.0

    report = verify_export(model, model_path, manifest_path, batch, atol=1e-5)
    assert report.max_abs_error <= 1e-5
    assert report.batch_size == 2

    with pytest.raises(ExportVerificationError, match="shape"):
        verify_export(
            model, model_path, manifest_path, np.zeros((2, 3), dtype=np.float32)
        )

    model_path.write_bytes(model_path.read_bytes() + b"tampered")
    with pytest.raises(ExportVerificationError, match="checksum"):
        verify_export(model, model_path, manifest_path, batch)
