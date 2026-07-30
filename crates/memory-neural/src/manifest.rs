//! 模型清单与校验和。

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::NeuralError;

/// 模型运行时允许使用的设备策略。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DevicePolicy {
    /// 只使用 CPU，保证离线环境具有一致的可用性。
    CpuOnly,
    /// 优先使用可用加速设备，失败时回退到 CPU。
    PreferAccelerator,
}

/// 创建模型清单所需的不可变契约字段。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ModelManifestSpec {
    /// 写入派生记录的模型版本。
    pub model_version: String,
    /// 模型提供方标识。
    pub provider: String,
    /// 模型名称。
    pub name: String,
    /// 提供方版本。
    pub version: String,
    /// 输入 Schema 标识。
    pub input_schema: String,
    /// 输出 Schema 标识。
    pub output_schema: String,
    /// 设备策略。
    pub device_policy: DevicePolicy,
    /// 单次允许的最大批量。
    pub max_batch_size: usize,
    /// 单次调用超时毫秒数。
    pub timeout_ms: u64,
}

/// 可验证、可序列化的模型运行契约。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ModelManifest {
    /// 写入派生记录的模型版本。
    pub model_version: String,
    /// 模型提供方标识。
    pub provider: String,
    /// 模型名称。
    pub name: String,
    /// 提供方版本。
    pub version: String,
    /// 输入 Schema 标识。
    pub input_schema: String,
    /// 输出 Schema 标识。
    pub output_schema: String,
    /// 设备策略。
    pub device_policy: DevicePolicy,
    /// 单次允许的最大批量。
    pub max_batch_size: usize,
    /// 单次调用超时毫秒数。
    pub timeout_ms: u64,
    /// 其余字段 canonical JSON 的 SHA-256。
    pub checksum_sha256: String,
}

impl ModelManifest {
    /// 校验字段并生成带 SHA-256 的模型清单。
    ///
    /// # Errors
    ///
    /// 必填字符串为空、批量为零或超时为零时返回错误。
    pub fn new(spec: ModelManifestSpec) -> Result<Self, NeuralError> {
        validate_spec(&spec)?;
        let checksum_sha256 = checksum(&spec)?;
        Ok(Self {
            model_version: spec.model_version,
            provider: spec.provider,
            name: spec.name,
            version: spec.version,
            input_schema: spec.input_schema,
            output_schema: spec.output_schema,
            device_policy: spec.device_policy,
            max_batch_size: spec.max_batch_size,
            timeout_ms: spec.timeout_ms,
            checksum_sha256,
        })
    }

    /// 验证当前字段是否仍与清单校验和一致。
    #[must_use]
    pub fn verify_checksum(&self) -> bool {
        let spec = ModelManifestSpec {
            model_version: self.model_version.clone(),
            provider: self.provider.clone(),
            name: self.name.clone(),
            version: self.version.clone(),
            input_schema: self.input_schema.clone(),
            output_schema: self.output_schema.clone(),
            device_policy: self.device_policy,
            max_batch_size: self.max_batch_size,
            timeout_ms: self.timeout_ms,
        };
        checksum(&spec).is_ok_and(|value| value == self.checksum_sha256)
    }
}

fn validate_spec(spec: &ModelManifestSpec) -> Result<(), NeuralError> {
    let required = [
        ("model_version", spec.model_version.as_str()),
        ("provider", spec.provider.as_str()),
        ("name", spec.name.as_str()),
        ("version", spec.version.as_str()),
        ("input_schema", spec.input_schema.as_str()),
        ("output_schema", spec.output_schema.as_str()),
    ];
    if let Some((field, _)) = required.iter().find(|(_, value)| value.trim().is_empty()) {
        return Err(NeuralError::InvalidManifest(format!(
            "{field} must not be empty"
        )));
    }
    if spec.max_batch_size == 0 {
        return Err(NeuralError::InvalidManifest(
            "max_batch_size must be positive".into(),
        ));
    }
    if spec.timeout_ms == 0 {
        return Err(NeuralError::InvalidManifest(
            "timeout_ms must be positive".into(),
        ));
    }
    Ok(())
}

fn checksum(spec: &ModelManifestSpec) -> Result<String, NeuralError> {
    let bytes = serde_json::to_vec(spec)
        .map_err(|error| NeuralError::ManifestEncoding(error.to_string()))?;
    Ok(hex::encode(Sha256::digest(bytes)))
}
