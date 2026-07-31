//! Graphiti 适配器配置。

use serde::Deserialize;
use thiserror::Error;

/// Graphiti 来源映射配置。
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct GraphitiConfig {
    /// 外部 Graphiti 数据的版本标识。
    pub source_version: String,
}

/// Graphiti 配置解析错误。
#[derive(Debug, Error)]
pub enum GraphitiConfigError {
    /// YAML 无法解析。
    #[error("解析 Graphiti YAML 配置失败: {0}")]
    Parse(#[from] yaml_serde::Error),
    /// 来源版本为空。
    #[error("Graphiti source_version 不能为空")]
    EmptySourceVersion,
}

impl GraphitiConfig {
    /// 从 YAML 文本解析配置。
    ///
    /// # Errors
    ///
    /// YAML 无法解析或来源版本为空时返回错误。
    pub fn from_yaml(value: &str) -> Result<Self, GraphitiConfigError> {
        let config: Self = yaml_serde::from_str(value)?;
        if config.source_version.trim().is_empty() {
            return Err(GraphitiConfigError::EmptySourceVersion);
        }
        Ok(config)
    }
}
