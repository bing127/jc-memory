//! Dify 适配器配置。

use jc_memory_protocol::{NamespaceId, RecallLimits};
use serde::Deserialize;
use thiserror::Error;

/// Dify 来源映射配置。
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct DifyConfig {
    /// Dify 来源版本。
    pub source_version: String,
    /// 所有 Dify 调用默认归属的 Namespace。
    pub namespace_id: NamespaceId,
    /// Dify Recall 的硬预算。
    pub recall_limits: RecallLimits,
}

/// Dify 配置解析错误。
#[derive(Debug, Error)]
pub enum DifyConfigError {
    /// YAML 无法解析。
    #[error("解析 Dify YAML 配置失败: {0}")]
    Parse(#[from] yaml_serde::Error),
    /// 来源版本为空。
    #[error("Dify source_version 不能为空")]
    EmptySourceVersion,
}

impl DifyConfig {
    /// 从 YAML 文本解析配置。
    ///
    /// # Errors
    ///
    /// YAML 无法解析或来源版本为空时返回错误。
    pub fn from_yaml(value: &str) -> Result<Self, DifyConfigError> {
        let config: Self = yaml_serde::from_str(value)?;
        if config.source_version.trim().is_empty() {
            return Err(DifyConfigError::EmptySourceVersion);
        }
        Ok(config)
    }
}
