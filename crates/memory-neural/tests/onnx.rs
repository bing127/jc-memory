//! ONNX Manifest、Checksum、Shape 和 Fallback 契约测试。

use std::{fs, path::Path, time::Duration};

use jc_memory_neural::onnx::{OnnxManifest, OnnxOutputManifest, OnnxProvider};
use tempfile::tempdir;

fn manifest(sha256: &str) -> OnnxManifest {
    OnnxManifest {
        model_id: "jc-memory-fixture".into(),
        version: "0.1.0".into(),
        license: "PRIVATE".into(),
        sha256: sha256.into(),
        schema: "jc-memory://neural/multitask/v1".into(),
        opset: 17,
        input_name: "features".into(),
        input_shape: vec![-1, 4],
        outputs: vec![
            OnnxOutputManifest {
                name: "router_logits".into(),
                shape: vec![-1, 3],
            },
            OnnxOutputManifest {
                name: "importance".into(),
                shape: vec![-1, 1],
            },
            OnnxOutputManifest {
                name: "contradiction_logits".into(),
                shape: vec![-1, 2],
            },
        ],
        training_data_digest: "synthetic-fixture-v1".into(),
        device_support: vec!["cpu".into()],
        max_batch_size: 16,
        timeout_ms: 25,
    }
}

#[test]
fn manifest_rejects_checksum_mismatch_before_runtime_loading() {
    let directory = tempdir().unwrap();
    let model_path = directory.path().join("model.onnx");
    let manifest_path = directory.path().join("manifest.json");
    fs::write(&model_path, b"not-an-onnx-model").unwrap();
    fs::write(
        &manifest_path,
        serde_json::to_vec(&manifest(&"0".repeat(64))).unwrap(),
    )
    .unwrap();

    let error = OnnxProvider::load(&model_path, &manifest_path).unwrap_err();
    assert!(error.to_string().contains("checksum"));
}

#[test]
fn manifest_validation_rejects_unknown_device_and_shape() {
    let mut value = manifest(&"a".repeat(64));
    value.device_support = vec!["cuda".into()];
    assert!(value.validate().is_err());
    value.device_support = vec!["cpu".into()];
    value.input_shape = vec![4];
    assert!(value.validate().is_err());
}

#[test]
fn deterministic_fallback_is_stable_and_has_expected_shapes() {
    let output = OnnxProvider::fallback_output(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0], 2, 3);
    assert_eq!(output.router_logits.len(), 6);
    assert_eq!(output.importance.len(), 2);
    assert_eq!(output.contradiction_logits.len(), 4);
    assert_eq!(
        output,
        OnnxProvider::fallback_output(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0], 2, 3)
    );
}

#[test]
fn timeout_configuration_is_bounded() {
    let mut value = manifest(&"a".repeat(64));
    value.timeout_ms = 0;
    assert!(value.validate().is_err());
    assert_eq!(Duration::from_millis(value.timeout_ms), Duration::ZERO);
    assert!(Path::new("models/onnx/fixtures/test-model.onnx").ends_with("test-model.onnx"));
}

#[tokio::test]
async fn committed_fixture_runs_through_cpu_provider() {
    let repository_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .unwrap();
    let provider = OnnxProvider::load(
        repository_root.join("models/onnx/fixtures/test-model.onnx"),
        repository_root.join("models/manifests/test-model.json"),
    )
    .unwrap();
    let result = provider
        .infer(vec![0.0, 0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7], 2)
        .await
        .unwrap();

    assert!(result.reason.is_none());
    assert_eq!(result.value.router_logits.len(), 6);
    assert_eq!(result.value.importance.len(), 2);
    assert_eq!(result.value.contradiction_logits.len(), 4);
}
