//! `OpenClaw` 适配器配置。

use jc_memory_protocol::{RecallLimits, SecurityLabel};
use serde::{Deserialize, Serialize};

/// `OpenClaw` Memory.md 适配器配置。
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OpenClawConfig {
    /// 当前 Fixture 对应的文件契约版本。
    pub source_version: String,
    /// Hook/MCP Recall 使用的硬预算。
    pub recall_limits: RecallLimits,
    /// 导入 Fragment 的默认重要性。
    #[serde(default = "default_importance")]
    pub default_importance: f32,
    /// 导入 Fragment 的安全标签。
    #[serde(default = "default_security_label")]
    pub security_label: SecurityLabel,
}

impl OpenClawConfig {
    /// 从 YAML 文本解析配置。
    ///
    /// # Errors
    ///
    /// YAML 不符合配置结构时返回解析错误。
    pub fn from_yaml(text: &str) -> Result<Self, yaml_serde::Error> {
        yaml_serde::from_str(text)
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.source_version.trim().is_empty() {
            return Err("source_version 不能为空".to_owned());
        }
        if !self.default_importance.is_finite() || !(0.0..=1.0).contains(&self.default_importance) {
            return Err("default_importance 必须位于 0.0..=1.0".to_owned());
        }
        Ok(())
    }
}

const fn default_importance() -> f32 {
    0.5
}

const fn default_security_label() -> SecurityLabel {
    SecurityLabel::Internal
}
