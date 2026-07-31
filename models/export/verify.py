"""验证 ONNX Manifest、输入 Shape 和 PyTorch/ONNX 输出一致性。"""

from __future__ import annotations

import hashlib
import json
from dataclasses import dataclass
from pathlib import Path
from typing import Any

import numpy as np
import onnxruntime as ort
import torch
from torch import nn

from .export import OUTPUT_NAMES


class ExportVerificationError(ValueError):
    """ONNX 产物不满足 Manifest 或一致性契约。"""


@dataclass(frozen=True)
class VerificationReport:
    """一次固定输入一致性验证报告。"""

    max_abs_error: float
    batch_size: int
    output_names: tuple[str, ...]


def verify_export(
    model: nn.Module,
    model_path: str | Path,
    manifest_path: str | Path,
    batch: np.ndarray,
    *,
    atol: float = 1e-5,
) -> VerificationReport:
    """校验 Manifest/Checksum/Shape，并比较 PyTorch 与 ONNX 输出。"""
    manifest = _load_manifest(model_path, manifest_path)
    expected_shape = manifest["input_shape"]
    if batch.ndim != 2 or batch.shape[1] != expected_shape[1]:
        raise ExportVerificationError(
            f"shape mismatch: expected [batch, {expected_shape[1]}], got {list(batch.shape)}"
        )
    if batch.shape[0] > manifest["max_batch_size"]:
        raise ExportVerificationError("shape batch exceeds manifest maximum")
    session = ort.InferenceSession(str(model_path), providers=["CPUExecutionProvider"])
    if session.get_inputs()[0].name != manifest["input_name"]:
        raise ExportVerificationError("input name mismatch")
    output_names = tuple(output.name for output in session.get_outputs())
    if output_names != OUTPUT_NAMES:
        raise ExportVerificationError("output names mismatch")
    onnx_outputs = session.run(
        list(OUTPUT_NAMES), {manifest["input_name"]: batch.astype(np.float32)}
    )
    with torch.no_grad():
        pytorch = model(torch.from_numpy(batch.astype(np.float32)))
    pytorch_outputs = [pytorch[name].detach().cpu().numpy() for name in OUTPUT_NAMES]
    max_error = max(
        float(np.max(np.abs(expected - actual)))
        for expected, actual in zip(pytorch_outputs, onnx_outputs, strict=True)
    )
    if max_error > atol:
        raise ExportVerificationError(
            f"output mismatch: max_abs_error={max_error} > atol={atol}"
        )
    return VerificationReport(max_error, int(batch.shape[0]), output_names)


def _load_manifest(model_path: str | Path, manifest_path: str | Path) -> dict[str, Any]:
    try:
        manifest = json.loads(Path(manifest_path).read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ExportVerificationError(f"manifest load failed: {error}") from error
    checksum = hashlib.sha256(Path(model_path).read_bytes()).hexdigest()
    if manifest.get("sha256") != checksum:
        raise ExportVerificationError("checksum mismatch")
    required = {
        "model_id",
        "version",
        "license",
        "schema",
        "opset",
        "input_name",
        "input_shape",
        "outputs",
        "training_data_digest",
        "device_support",
        "max_batch_size",
        "timeout_ms",
    }
    missing = sorted(required - set(manifest))
    if missing:
        raise ExportVerificationError(f"manifest missing fields: {', '.join(missing)}")
    if manifest["license"] != "PRIVATE" or manifest["device_support"] != ["cpu"]:
        raise ExportVerificationError("manifest policy mismatch")
    if (
        manifest["opset"] != 17
        or manifest["schema"] != "jc-memory://neural/multitask/v1"
    ):
        raise ExportVerificationError("manifest schema mismatch")
    if manifest["input_shape"][0] != -1 or len(manifest["input_shape"]) != 2:
        raise ExportVerificationError("manifest input shape mismatch")
    if tuple(output["name"] for output in manifest["outputs"]) != OUTPUT_NAMES:
        raise ExportVerificationError("manifest output names mismatch")
    return manifest
