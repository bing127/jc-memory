//! 经过 Manifest/Checksum/Shape 校验的 ONNX CPU Provider。

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use ndarray::Array2;
use ort::{session::Session, value::Tensor};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::Mutex as AsyncMutex;

use crate::{CircuitBreaker, FallbackResult, NeuralError, with_timeout_fallback};

const EXPECTED_SCHEMA: &str = "jc-memory://neural/multitask/v1";
const EXPECTED_INPUT_NAME: &str = "features";
const EXPECTED_OUTPUTS: [&str; 3] = ["router_logits", "importance", "contradiction_logits"];

/// ONNX 输出的 Shape 与名称契约。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OnnxOutputManifest {
    /// 输出名称。
    pub name: String,
    /// 输出 Shape，第一维必须是动态 batch。
    pub shape: Vec<i64>,
}

/// ONNX 文件的可审计部署清单。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OnnxManifest {
    /// 模型稳定标识。
    pub model_id: String,
    /// 模型版本。
    pub version: String,
    /// 许可证策略；私有构建固定使用 PRIVATE。
    pub license: String,
    /// ONNX 文件 SHA-256。
    pub sha256: String,
    /// 输入输出协议 Schema。
    pub schema: String,
    /// ONNX opset 版本。
    pub opset: u32,
    /// 输入张量名称。
    pub input_name: String,
    /// 输入张量 Shape。
    pub input_shape: Vec<i64>,
    /// 输出张量契约。
    pub outputs: Vec<OnnxOutputManifest>,
    /// 训练数据摘要，不保存原始样本。
    pub training_data_digest: String,
    /// 允许的设备列表。
    pub device_support: Vec<String>,
    /// 单次允许的最大 batch。
    pub max_batch_size: usize,
    /// 主 Provider 超时毫秒数。
    pub timeout_ms: u64,
}

impl OnnxManifest {
    /// 校验 Manifest 的字段、Schema、Shape 和设备策略。
    ///
    /// # Errors
    ///
    /// 字段为空、Checksum、Shape 或设备策略不符合契约时返回错误。
    pub fn validate(&self) -> Result<(), NeuralError> {
        for (field, value) in [
            ("model_id", self.model_id.as_str()),
            ("version", self.version.as_str()),
            ("license", self.license.as_str()),
            ("schema", self.schema.as_str()),
            ("input_name", self.input_name.as_str()),
            ("training_data_digest", self.training_data_digest.as_str()),
        ] {
            if value.trim().is_empty() {
                return Err(NeuralError::InvalidManifest(format!(
                    "{field} must not be empty"
                )));
            }
        }
        if self.license != "PRIVATE" {
            return Err(NeuralError::InvalidManifest(
                "license must be PRIVATE".into(),
            ));
        }
        if self.schema != EXPECTED_SCHEMA || self.input_name != EXPECTED_INPUT_NAME {
            return Err(NeuralError::InvalidManifest(
                "ONNX schema or input name is unsupported".into(),
            ));
        }
        if self.opset == 0 || self.max_batch_size == 0 || self.timeout_ms == 0 {
            return Err(NeuralError::InvalidManifest(
                "opset, max_batch_size and timeout_ms must be positive".into(),
            ));
        }
        if self.sha256.len() != 64 || !self.sha256.chars().all(|value| value.is_ascii_hexdigit()) {
            return Err(NeuralError::InvalidManifest(
                "sha256 must be a 64-character hexadecimal digest".into(),
            ));
        }
        if self.device_support.len() != 1 || self.device_support[0] != "cpu" {
            return Err(NeuralError::InvalidManifest(
                "only the cpu execution provider is supported".into(),
            ));
        }
        validate_shape(&self.input_shape, "input_shape", true)?;
        if self.outputs.len() != EXPECTED_OUTPUTS.len() {
            return Err(NeuralError::InvalidManifest(
                "ONNX output count does not match the contract".into(),
            ));
        }
        for (index, (output, expected_name)) in
            self.outputs.iter().zip(EXPECTED_OUTPUTS).enumerate()
        {
            if output.name != expected_name {
                return Err(NeuralError::InvalidManifest(format!(
                    "unexpected ONNX output name: {}",
                    output.name
                )));
            }
            validate_shape(&output.shape, "output_shape", true)?;
            let expected_width = match index {
                1 => 1,
                2 => 2,
                _ => output.shape[1],
            };
            if output.shape[1] != expected_width {
                return Err(NeuralError::InvalidManifest(format!(
                    "unexpected width for ONNX output: {}",
                    output.name
                )));
            }
        }
        Ok(())
    }

    fn input_dimension(&self) -> Result<usize, NeuralError> {
        usize::try_from(self.input_shape[1]).map_err(|_| {
            NeuralError::InvalidManifest("input feature dimension must be positive".into())
        })
    }

    fn router_classes(&self) -> Result<usize, NeuralError> {
        usize::try_from(self.outputs[0].shape[1]).map_err(|_| {
            NeuralError::InvalidManifest("router class dimension must be positive".into())
        })
    }
}

/// ONNX 多任务输出。
#[derive(Clone, Debug, PartialEq)]
pub struct OnnxOutput {
    /// Router 分类 logits，按 batch 展平。
    pub router_logits: Vec<f32>,
    /// Importance 回归输出，按 batch 展平。
    pub importance: Vec<f32>,
    /// Contradiction 分类 logits，按 batch 展平。
    pub contradiction_logits: Vec<f32>,
}

/// 校验并运行 ONNX CPU 模型的 Provider。
pub struct OnnxProvider {
    model_path: PathBuf,
    manifest: OnnxManifest,
    session: Arc<Mutex<Session>>,
    breaker: AsyncMutex<CircuitBreaker>,
    started_at: Instant,
}

impl std::fmt::Debug for OnnxProvider {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OnnxProvider")
            .field("model_path", &self.model_path)
            .field("manifest", &self.manifest)
            .finish_non_exhaustive()
    }
}

impl OnnxProvider {
    /// 读取并校验 Manifest、模型 SHA-256、Session 输入输出名称。
    ///
    /// # Errors
    ///
    /// 文件无法读取、Manifest 无效、Checksum 不一致或 Session 契约不匹配时返回错误。
    pub fn load(
        model_path: impl AsRef<Path>,
        manifest_path: impl AsRef<Path>,
    ) -> Result<Self, NeuralError> {
        let model_path = model_path.as_ref().to_path_buf();
        let manifest_bytes = std::fs::read(manifest_path.as_ref()).map_err(|error| {
            NeuralError::Provider(format!("failed to read ONNX manifest: {error}"))
        })?;
        let manifest: OnnxManifest = serde_json::from_slice(&manifest_bytes).map_err(|error| {
            NeuralError::Provider(format!("failed to parse ONNX manifest: {error}"))
        })?;
        manifest.validate()?;
        let model_bytes = std::fs::read(&model_path).map_err(|error| {
            NeuralError::Provider(format!("failed to read ONNX model: {error}"))
        })?;
        let checksum = hex::encode(Sha256::digest(&model_bytes));
        if checksum != manifest.sha256 {
            return Err(NeuralError::Provider(format!(
                "ONNX model checksum mismatch: expected {}, got {}",
                manifest.sha256, checksum
            )));
        }
        ort::init().with_name("jc-memory-neural").commit();
        let session = Session::builder()
            .map_err(|error| {
                NeuralError::Provider(format!("failed to create ONNX session: {error}"))
            })?
            .with_intra_threads(1)
            .map_err(|error| {
                NeuralError::Provider(format!("failed to configure ONNX session: {error}"))
            })?
            .commit_from_file(&model_path)
            .map_err(|error| {
                NeuralError::Provider(format!("failed to load ONNX model: {error}"))
            })?;
        validate_session_contract(&session, &manifest)?;
        let cooldown = Duration::from_millis(manifest.timeout_ms);
        Ok(Self {
            model_path,
            manifest,
            session: Arc::new(Mutex::new(session)),
            breaker: AsyncMutex::new(CircuitBreaker::new(cooldown)),
            started_at: Instant::now(),
        })
    }

    /// 返回已验证的 Manifest。
    #[must_use]
    pub const fn manifest(&self) -> &OnnxManifest {
        &self.manifest
    }

    /// 运行主模型；超时、错误或熔断时返回确定性数值 Fallback。
    ///
    /// # Errors
    ///
    /// 输入 batch/Shape 超过限制，或确定性 Fallback 无法构造时返回错误。
    pub async fn infer(
        &self,
        features: Vec<f32>,
        batch_size: usize,
    ) -> Result<FallbackResult<OnnxOutput>, NeuralError> {
        let input_dimension = self.manifest.input_dimension()?;
        let router_classes = self.manifest.router_classes()?;
        if batch_size == 0 || batch_size > self.manifest.max_batch_size {
            return Err(NeuralError::BatchTooLarge {
                actual: batch_size,
                maximum: self.manifest.max_batch_size,
            });
        }
        let expected_len = batch_size
            .checked_mul(input_dimension)
            .ok_or_else(|| NeuralError::Provider("input size overflow".into()))?;
        if features.len() != expected_len {
            return Err(NeuralError::Provider(format!(
                "input shape mismatch: expected {}, got {}",
                expected_len,
                features.len()
            )));
        }
        let primary_session = Arc::clone(&self.session);
        let primary_features = features.clone();
        let timeout = Duration::from_millis(self.manifest.timeout_ms);
        let mut breaker = self.breaker.lock().await;
        with_timeout_fallback(
            &mut breaker,
            self.started_at.elapsed(),
            timeout,
            async move {
                tokio::task::spawn_blocking(move || {
                    run_session(
                        primary_session,
                        primary_features,
                        batch_size,
                        input_dimension,
                        router_classes,
                    )
                })
                .await
                .map_err(|error| NeuralError::Provider(format!("ONNX worker failed: {error}")))?
            },
            || async move { Ok(Self::fallback_output(&features, batch_size, router_classes)) },
        )
        .await
    }

    /// 生成不依赖外部运行时的稳定数值 Fallback。
    #[must_use]
    pub fn fallback_output(
        features: &[f32],
        batch_size: usize,
        router_classes: usize,
    ) -> OnnxOutput {
        let input_dimension = features.len() / batch_size.max(1);
        let mut router_logits = Vec::with_capacity(batch_size * router_classes);
        let mut importance = Vec::with_capacity(batch_size);
        let mut contradiction_logits = Vec::with_capacity(batch_size * 2);
        for row in features.chunks(input_dimension.max(1)).take(batch_size) {
            let sum = row.iter().copied().sum::<f32>();
            for class in 0..router_classes {
                let class_index = u16::try_from(class).unwrap_or(u16::MAX);
                router_logits.push(sum / (f32::from(class_index) + 1.0));
            }
            let row_len = u16::try_from(row.len().max(1)).unwrap_or(u16::MAX);
            importance.push((sum / f32::from(row_len)).clamp(0.0, 1.0));
            contradiction_logits.extend([sum, -sum]);
        }
        OnnxOutput {
            router_logits,
            importance,
            contradiction_logits,
        }
    }
}

fn validate_shape(shape: &[i64], field: &str, dynamic_batch: bool) -> Result<(), NeuralError> {
    if shape.len() != 2 || (dynamic_batch && shape[0] != -1) || shape[1] <= 0 {
        return Err(NeuralError::InvalidManifest(format!(
            "{field} must be [ -1, positive_dimension ]"
        )));
    }
    Ok(())
}

fn validate_session_contract(
    session: &Session,
    manifest: &OnnxManifest,
) -> Result<(), NeuralError> {
    if session.inputs().len() != 1 || session.inputs()[0].name() != manifest.input_name {
        return Err(NeuralError::Provider("ONNX input name mismatch".into()));
    }
    let names: Vec<_> = session
        .outputs()
        .iter()
        .map(ort::value::Outlet::name)
        .collect();
    if names != EXPECTED_OUTPUTS {
        return Err(NeuralError::Provider("ONNX output names mismatch".into()));
    }
    Ok(())
}

#[allow(clippy::needless_pass_by_value)]
fn run_session(
    session: Arc<Mutex<Session>>,
    features: Vec<f32>,
    batch_size: usize,
    input_dimension: usize,
    router_classes: usize,
) -> Result<OnnxOutput, NeuralError> {
    let input = Array2::from_shape_vec((batch_size, input_dimension), features)
        .map_err(|error| NeuralError::Provider(format!("failed to shape ONNX input: {error}")))?;
    let tensor = Tensor::from_array(input)
        .map_err(|error| NeuralError::Provider(format!("failed to create ONNX tensor: {error}")))?;
    let mut guard = session
        .lock()
        .map_err(|_| NeuralError::Provider("ONNX session lock poisoned".into()))?;
    let outputs = guard
        .run(ort::inputs![tensor])
        .map_err(|error| NeuralError::Provider(format!("ONNX inference failed: {error}")))?;
    let router_logits = extract_output(&outputs, "router_logits")?;
    let importance = extract_output(&outputs, "importance")?;
    let contradiction_logits = extract_output(&outputs, "contradiction_logits")?;
    let output = OnnxOutput {
        router_logits,
        importance,
        contradiction_logits,
    };
    if output.router_logits.len() != batch_size * router_classes
        || output.importance.len() != batch_size
        || output.contradiction_logits.len() != batch_size * 2
    {
        return Err(NeuralError::Provider("ONNX output shape mismatch".into()));
    }
    Ok(output)
}

fn extract_output(
    outputs: &ort::session::SessionOutputs<'_>,
    name: &str,
) -> Result<Vec<f32>, NeuralError> {
    let value = outputs
        .get(name)
        .ok_or_else(|| NeuralError::Provider(format!("ONNX output missing: {name}")))?;
    let (_, values) = value
        .try_extract_tensor::<f32>()
        .map_err(|error| NeuralError::Provider(format!("ONNX output shape mismatch: {error}")))?;
    Ok(values.to_vec())
}
