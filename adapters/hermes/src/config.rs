//! Hermes 适配器配置。

use jc_memory_protocol::{RecallLimits, SecurityLabel};
use serde::{Deserialize, Serialize};

/// Hermes 适配器使用的版本化配置。
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HermesConfig {
    /// Hermes 外部发行版本，只用于来源溯源，不参与语义推断。
    pub hermes_version: String,
    /// Hermes turn 接口版本。
    pub interface_version: String,
    /// 未提供请求级预算时使用的 Recall 硬预算。
    #[serde(default)]
    pub recall_limits: RecallLimits,
    /// 适配器生成 Fragment 的默认重要性。
    #[serde(default = "default_importance")]
    pub default_importance: f32,
    /// 适配器写入 Fragment 的安全标签。
    #[serde(default = "default_security_label")]
    pub security_label: SecurityLabel,
}

impl HermesConfig {
    /// 校验配置中的版本和数值边界。
    ///
    /// # Errors
    ///
    /// 版本为空或默认重要性越界时返回中文配置错误。
    pub fn validate(&self) -> Result<(), String> {
        if self.hermes_version.trim().is_empty() {
            return Err("hermes_version 不能为空".to_owned());
        }
        if self.interface_version.trim().is_empty() {
            return Err("interface_version 不能为空".to_owned());
        }
        if !self.default_importance.is_finite() || !(0.0..=1.0).contains(&self.default_importance) {
            return Err("default_importance 必须位于 0.0..=1.0".to_owned());
        }
        Ok(())
    }

    /// 从 YAML 文本解析 Hermes 配置。
    ///
    /// # Errors
    ///
    /// YAML 语法或配置字段不符合契约时返回错误。
    pub fn from_yaml(text: &str) -> Result<Self, yaml_serde::Error> {
        yaml_serde::from_str(text)
    }
}

const fn default_importance() -> f32 {
    0.5
}

const fn default_security_label() -> SecurityLabel {
    SecurityLabel::Internal
}
