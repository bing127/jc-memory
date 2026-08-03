//! 完全离线路由关键词策略。

use std::collections::HashSet;

use serde::Deserialize;
use sha2::{Digest as _, Sha256};

const BUILTIN_YAML: &str = include_str!("../../../config/neural/fallback-routing.zh-CN.yaml");
const MAX_KEYWORDS: usize = 128;
const MAX_KEYWORD_LENGTH: usize = 128;

/// 已严格校验的离线路由规则。
#[derive(Clone, Debug)]
pub struct FallbackRoutingPolicy {
    policy_version: String,
    causal_keywords: Vec<String>,
    current_state_keywords: Vec<String>,
    procedural_keywords: Vec<String>,
    sha256: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRoutingPolicy {
    schema_version: u32,
    policy_version: String,
    causal_keywords: Vec<String>,
    current_state_keywords: Vec<String>,
    procedural_keywords: Vec<String>,
}

/// 离线路由策略解析错误。
#[derive(Debug, thiserror::Error)]
pub enum FallbackRoutingPolicyError {
    /// YAML 文档不符合严格 Schema。
    #[error("离线路由策略 YAML 无效: {0}")]
    Parse(#[from] yaml_serde::Error),
    /// 词项、版本或 Schema 违反约束。
    #[error("离线路由策略字段无效: {0}")]
    Invalid(&'static str),
}

impl FallbackRoutingPolicy {
    /// 从 YAML 加载并校验离线路由策略。
    pub fn from_yaml_str(input: &str) -> Result<Self, FallbackRoutingPolicyError> {
        let raw: RawRoutingPolicy = yaml_serde::from_str(input)?;
        if raw.schema_version != 1 {
            return Err(FallbackRoutingPolicyError::Invalid("schema_version"));
        }
        let policy_version = normalize(raw.policy_version, "policy_version")?;
        let causal_keywords = normalize_keywords(raw.causal_keywords, "causal_keywords")?;
        let current_state_keywords =
            normalize_keywords(raw.current_state_keywords, "current_state_keywords")?;
        let procedural_keywords =
            normalize_keywords(raw.procedural_keywords, "procedural_keywords")?;
        Ok(Self {
            policy_version,
            causal_keywords,
            current_state_keywords,
            procedural_keywords,
            sha256: hex::encode(Sha256::digest(input.as_bytes())),
        })
    }

    /// 返回编译内置策略。
    pub fn builtin() -> Result<Self, FallbackRoutingPolicyError> {
        Self::from_yaml_str(BUILTIN_YAML)
    }

    /// 返回策略版本。
    #[must_use]
    pub fn version(&self) -> &str {
        &self.policy_version
    }

    /// 返回原始 YAML 的 SHA-256 摘要。
    #[must_use]
    pub fn sha256(&self) -> &str {
        &self.sha256
    }

    pub(crate) fn matches_causal(&self, text: &str) -> bool {
        contains_any(text, &self.causal_keywords)
    }

    pub(crate) fn matches_current_state(&self, text: &str) -> bool {
        contains_any(text, &self.current_state_keywords)
    }

    pub(crate) fn matches_procedural(&self, text: &str) -> bool {
        contains_any(text, &self.procedural_keywords)
    }
}

fn normalize_keywords(
    keywords: Vec<String>,
    field: &'static str,
) -> Result<Vec<String>, FallbackRoutingPolicyError> {
    if keywords.is_empty() || keywords.len() > MAX_KEYWORDS {
        return Err(FallbackRoutingPolicyError::Invalid(field));
    }
    let mut normalized = Vec::with_capacity(keywords.len());
    let mut seen = HashSet::with_capacity(keywords.len());
    for keyword in keywords {
        let keyword = normalize(keyword, field)?;
        if keyword.len() > MAX_KEYWORD_LENGTH || !seen.insert(keyword.clone()) {
            return Err(FallbackRoutingPolicyError::Invalid(field));
        }
        normalized.push(keyword);
    }
    Ok(normalized)
}

fn normalize(value: String, field: &'static str) -> Result<String, FallbackRoutingPolicyError> {
    let value = value.trim().to_lowercase();
    if value.is_empty() {
        return Err(FallbackRoutingPolicyError::Invalid(field));
    }
    Ok(value)
}

fn contains_any(value: &str, keywords: &[String]) -> bool {
    keywords.iter().any(|keyword| value.contains(keyword))
}
