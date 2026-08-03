//! 版本化 Quarantine 规则的解析、校验和访问接口。

use std::collections::HashSet;

use serde::Deserialize;
use sha2::{Digest as _, Sha256};

const BUILTIN_YAML: &str = include_str!("../../../config/security/quarantine.zh-CN.yaml");
const MAX_MARKERS: usize = 128;
const MAX_MARKER_LENGTH: usize = 256;
const MAX_POLICY_VERSION_LENGTH: usize = 128;

/// 已通过结构校验的隔离策略。
#[derive(Clone, Debug)]
pub struct QuarantinePolicy {
    policy_version: String,
    quarantine_threshold: f32,
    repeated_disputed_threshold: usize,
    prompt_injection: PromptInjectionRules,
    sensitive_data: SensitiveDataRules,
    repeated_disputed_claim_score: f32,
    suspicious_tool_result: SuspiciousToolResultRules,
    cross_scope_reference_score: f32,
    sha256: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPolicy {
    schema_version: u32,
    policy_version: String,
    quarantine_threshold: f32,
    repeated_disputed_threshold: usize,
    prompt_injection: RawPromptInjectionRules,
    sensitive_data: RawSensitiveDataRules,
    repeated_disputed_claim: RawScore,
    suspicious_tool_result: RawSuspiciousToolResultRules,
    cross_scope_reference_score: f32,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPromptInjectionRules {
    instruction_override_markers: Vec<String>,
    hidden_prompt_markers: Vec<String>,
    disclosure_markers: Vec<String>,
    score: f32,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSensitiveDataRules {
    markers: Vec<String>,
    score: f32,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSuspiciousToolResultRules {
    verified_markers: Vec<String>,
    authorization_markers: Vec<String>,
    score: f32,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawScore {
    score: f32,
}

#[derive(Clone, Debug)]
pub(crate) struct PromptInjectionRules {
    instruction_override_markers: Vec<String>,
    hidden_prompt_markers: Vec<String>,
    disclosure_markers: Vec<String>,
    score: f32,
}

#[derive(Clone, Debug)]
pub(crate) struct SensitiveDataRules {
    markers: Vec<String>,
    score: f32,
}

#[derive(Clone, Debug)]
pub(crate) struct SuspiciousToolResultRules {
    verified_markers: Vec<String>,
    authorization_markers: Vec<String>,
    score: f32,
}

/// 隔离策略加载和校验错误。
#[derive(Debug, thiserror::Error)]
pub enum QuarantinePolicyError {
    /// YAML 文档无法反序列化为严格 Schema。
    #[error("Quarantine 策略 YAML 无效: {0}")]
    Parse(#[from] yaml_serde::Error),
    /// 策略字段违反范围、长度或唯一性约束。
    #[error("Quarantine 策略字段无效: {0}")]
    Invalid(&'static str),
}

impl QuarantinePolicy {
    /// 从原始 YAML 载荷加载并严格校验策略。
    pub fn from_yaml_str(input: &str) -> Result<Self, QuarantinePolicyError> {
        let raw: RawPolicy = yaml_serde::from_str(input)?;
        if raw.schema_version != 1 {
            return Err(QuarantinePolicyError::Invalid("schema_version"));
        }
        let policy_version = normalize_text(raw.policy_version, "policy_version")?;
        if policy_version.len() > MAX_POLICY_VERSION_LENGTH {
            return Err(QuarantinePolicyError::Invalid("policy_version"));
        }
        validate_score(raw.quarantine_threshold, "quarantine_threshold")?;
        if raw.repeated_disputed_threshold == 0 {
            return Err(QuarantinePolicyError::Invalid(
                "repeated_disputed_threshold",
            ));
        }
        let prompt_injection = PromptInjectionRules {
            instruction_override_markers: normalize_markers(
                raw.prompt_injection.instruction_override_markers,
                "instruction_override_markers",
            )?,
            hidden_prompt_markers: normalize_markers(
                raw.prompt_injection.hidden_prompt_markers,
                "hidden_prompt_markers",
            )?,
            disclosure_markers: normalize_markers(
                raw.prompt_injection.disclosure_markers,
                "disclosure_markers",
            )?,
            score: checked_score(raw.prompt_injection.score, "prompt_injection.score")?,
        };
        let sensitive_data = SensitiveDataRules {
            markers: normalize_markers(raw.sensitive_data.markers, "sensitive_data.markers")?,
            score: checked_score(raw.sensitive_data.score, "sensitive_data.score")?,
        };
        let suspicious_tool_result = SuspiciousToolResultRules {
            verified_markers: normalize_markers(
                raw.suspicious_tool_result.verified_markers,
                "verified_markers",
            )?,
            authorization_markers: normalize_markers(
                raw.suspicious_tool_result.authorization_markers,
                "authorization_markers",
            )?,
            score: checked_score(
                raw.suspicious_tool_result.score,
                "suspicious_tool_result.score",
            )?,
        };
        let repeated_disputed_claim_score = checked_score(
            raw.repeated_disputed_claim.score,
            "repeated_disputed_claim.score",
        )?;
        let cross_scope_reference_score = checked_score(
            raw.cross_scope_reference_score,
            "cross_scope_reference_score",
        )?;
        let sha256 = hex::encode(Sha256::digest(input.as_bytes()));
        Ok(Self {
            policy_version,
            quarantine_threshold: raw.quarantine_threshold,
            repeated_disputed_threshold: raw.repeated_disputed_threshold,
            prompt_injection,
            sensitive_data,
            repeated_disputed_claim_score,
            suspicious_tool_result,
            cross_scope_reference_score,
            sha256,
        })
    }

    /// 返回编译内置的同一版本化策略。
    pub fn builtin() -> Result<Self, QuarantinePolicyError> {
        Self::from_yaml_str(BUILTIN_YAML)
    }

    /// 返回策略版本标识。
    #[must_use]
    pub fn version(&self) -> &str {
        &self.policy_version
    }

    /// 返回原始策略 YAML 的 SHA-256 十六进制摘要。
    #[must_use]
    pub fn sha256(&self) -> &str {
        &self.sha256
    }

    pub(crate) const fn quarantine_threshold(&self) -> f32 {
        self.quarantine_threshold
    }

    pub(crate) const fn repeated_disputed_threshold(&self) -> usize {
        self.repeated_disputed_threshold
    }

    pub(crate) fn prompt_injection(&self) -> &PromptInjectionRules {
        &self.prompt_injection
    }

    pub(crate) fn sensitive_data(&self) -> &SensitiveDataRules {
        &self.sensitive_data
    }

    pub(crate) fn suspicious_tool_result(&self) -> &SuspiciousToolResultRules {
        &self.suspicious_tool_result
    }

    pub(crate) const fn repeated_disputed_claim_score(&self) -> f32 {
        self.repeated_disputed_claim_score
    }

    pub(crate) const fn cross_scope_reference_score(&self) -> f32 {
        self.cross_scope_reference_score
    }
}

impl PromptInjectionRules {
    pub(crate) fn matches_instruction_override(&self, text: &str) -> bool {
        contains_any(text, &self.instruction_override_markers)
    }

    pub(crate) fn matches_hidden_prompt(&self, text: &str) -> bool {
        contains_any(text, &self.hidden_prompt_markers)
    }

    pub(crate) fn matches_disclosure(&self, text: &str) -> bool {
        contains_any(text, &self.disclosure_markers)
    }

    pub(crate) const fn score(&self) -> f32 {
        self.score
    }
}

impl SensitiveDataRules {
    pub(crate) fn matches(&self, text: &str) -> bool {
        contains_any(text, &self.markers)
    }

    pub(crate) const fn score(&self) -> f32 {
        self.score
    }
}

impl SuspiciousToolResultRules {
    pub(crate) fn matches_verified(&self, text: &str) -> bool {
        contains_any(text, &self.verified_markers)
    }

    pub(crate) fn matches_authorization(&self, text: &str) -> bool {
        contains_any(text, &self.authorization_markers)
    }

    pub(crate) const fn score(&self) -> f32 {
        self.score
    }
}

fn normalize_markers(
    markers: Vec<String>,
    field: &'static str,
) -> Result<Vec<String>, QuarantinePolicyError> {
    if markers.is_empty() || markers.len() > MAX_MARKERS {
        return Err(QuarantinePolicyError::Invalid(field));
    }
    let mut normalized = Vec::with_capacity(markers.len());
    let mut seen = HashSet::with_capacity(markers.len());
    for marker in markers {
        let marker = normalize_text(marker, field)?;
        if marker.len() > MAX_MARKER_LENGTH || !seen.insert(marker.clone()) {
            return Err(QuarantinePolicyError::Invalid(field));
        }
        normalized.push(marker);
    }
    Ok(normalized)
}

fn normalize_text(value: String, field: &'static str) -> Result<String, QuarantinePolicyError> {
    let value = value.trim().to_lowercase();
    if value.is_empty() {
        return Err(QuarantinePolicyError::Invalid(field));
    }
    Ok(value)
}

fn validate_score(value: f32, field: &'static str) -> Result<(), QuarantinePolicyError> {
    if !value.is_finite() || !(0.0..=1.0).contains(&value) {
        Err(QuarantinePolicyError::Invalid(field))
    } else {
        Ok(())
    }
}

fn checked_score(value: f32, field: &'static str) -> Result<f32, QuarantinePolicyError> {
    validate_score(value, field)?;
    Ok(value)
}

fn contains_any(text: &str, markers: &[String]) -> bool {
    markers.iter().any(|marker| text.contains(marker))
}
