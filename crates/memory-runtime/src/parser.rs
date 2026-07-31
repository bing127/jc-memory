//! 模型输出的严格结构化解析。

use serde::Deserialize;
use serde_json::Value;

use crate::ModelOutput;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OutputEnvelope {
    kind: OutputKind,
    payload: Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum OutputKind {
    Hypothesis,
    Prediction,
    Plan,
    DecisionCandidate,
    ToolRequest,
    FinalResponse,
}

/// 模型输出的稳定解析错误。
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ModelParseError {
    /// JSON 结构或字段类型不符合六类输出契约。
    #[error("model output schema mismatch: {0}")]
    Schema(String),
    /// 字段虽然类型正确，但值不满足语义约束。
    #[error("model output semantic validation failed: {0}")]
    Invalid(&'static str),
}

/// 将 JSON 候选解析为六种严格类型之一。
///
/// # Errors
///
/// kind 未知、字段结构错误或语义约束不满足时返回错误。
pub fn parse_model_output(value: Value) -> Result<ModelOutput, ModelParseError> {
    let envelope: OutputEnvelope = serde_json::from_value(value)
        .map_err(|error| ModelParseError::Schema(error.to_string()))?;
    let output = match envelope.kind {
        OutputKind::Hypothesis => ModelOutput::Hypothesis(decode_payload(envelope.payload)?),
        OutputKind::Prediction => ModelOutput::Prediction(decode_payload(envelope.payload)?),
        OutputKind::Plan => ModelOutput::Plan(decode_payload(envelope.payload)?),
        OutputKind::DecisionCandidate => {
            ModelOutput::DecisionCandidate(decode_payload(envelope.payload)?)
        }
        OutputKind::ToolRequest => ModelOutput::ToolRequest(decode_payload(envelope.payload)?),
        OutputKind::FinalResponse => ModelOutput::FinalResponse(decode_payload(envelope.payload)?),
    };
    validate_output(&output)?;
    Ok(output)
}

fn decode_payload<T>(value: Value) -> Result<T, ModelParseError>
where
    T: for<'de> Deserialize<'de>,
{
    serde_json::from_value(value).map_err(|error| ModelParseError::Schema(error.to_string()))
}

fn validate_output(output: &ModelOutput) -> Result<(), ModelParseError> {
    match output {
        ModelOutput::Hypothesis(value) => require_text(&value.statement, "empty hypothesis"),
        ModelOutput::Prediction(value) => {
            require_text(&value.statement, "empty prediction")?;
            if value.confidence.is_finite() && (0.0..=1.0).contains(&value.confidence) {
                Ok(())
            } else {
                Err(ModelParseError::Invalid(
                    "prediction confidence out of range",
                ))
            }
        }
        ModelOutput::Plan(value) => {
            require_text(&value.goal, "empty plan goal")?;
            if value.steps.is_empty() || value.steps.iter().any(|step| step.trim().is_empty()) {
                Err(ModelParseError::Invalid("plan steps must be non-empty"))
            } else {
                Ok(())
            }
        }
        ModelOutput::DecisionCandidate(value) => {
            require_text(&value.decision, "empty decision")?;
            require_text(&value.rationale, "empty decision rationale")
        }
        ModelOutput::ToolRequest(value) => {
            require_text(&value.capability_id, "empty capability id")?;
            if value.arguments.is_object() {
                Ok(())
            } else {
                Err(ModelParseError::Invalid("tool arguments must be an object"))
            }
        }
        ModelOutput::FinalResponse(value) => require_text(&value.text, "empty final response"),
    }
}

fn require_text(value: &str, reason: &'static str) -> Result<(), ModelParseError> {
    if value.trim().is_empty() {
        Err(ModelParseError::Invalid(reason))
    } else {
        Ok(())
    }
}
