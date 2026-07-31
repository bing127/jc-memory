//! Hermes turn 的来源映射。

use chrono::Utc;
use jc_memory_protocol::{
    CognitiveFragment, FragmentContent, FragmentId, FragmentKind, FragmentSource, Metadata,
    Perspective, SourceType, ValidityStatus,
};
use jc_memory_runtime::{ModelOutput, ModelResponse};
use serde_json::{Value, json};

use crate::{HermesConfig, HermesTurn};

/// 将一个 Hermes turn 映射为不提升可信度的原始 Fragment 集合。
pub(crate) fn map_turn(turn: &HermesTurn, config: &HermesConfig) -> Vec<CognitiveFragment> {
    let mut fragments = Vec::with_capacity(
        turn.observations.len()
            + usize::from(turn.model_output.is_some())
            + turn.tool_results.len(),
    );
    for (index, observation) in turn.observations.iter().enumerate() {
        fragments.push(fragment(
            turn,
            config,
            FragmentSpec {
                kind: FragmentKind::Observation,
                source_type: SourceType::Agent,
                perspective: Perspective::System,
                content: FragmentContent::Text {
                    text: observation.text.clone(),
                },
                category: "observation",
                index,
                confidence: 0.0,
            },
        ));
    }
    if let Some(model_output) = &turn.model_output {
        fragments.push(model_fragment(turn, config, model_output));
    }
    for (index, tool_result) in turn.tool_results.iter().enumerate() {
        fragments.push(fragment(
            turn,
            config,
            FragmentSpec {
                kind: FragmentKind::ToolResult,
                source_type: SourceType::Tool,
                perspective: Perspective::Tool,
                content: FragmentContent::Json {
                    value: json!({"success": tool_result.success, "value": tool_result.value}),
                },
                category: "tool_result",
                index,
                confidence: 0.0,
            },
        ));
    }
    fragments
}

fn model_fragment(
    turn: &HermesTurn,
    config: &HermesConfig,
    response: &ModelResponse,
) -> CognitiveFragment {
    let (kind, content, confidence) = match &response.output {
        ModelOutput::Hypothesis(value) => (
            FragmentKind::Inference,
            FragmentContent::Text {
                text: value.statement.clone(),
            },
            0.0,
        ),
        ModelOutput::Prediction(value) => (
            FragmentKind::Prediction,
            FragmentContent::Json {
                value: json!({
                    "statement": value.statement,
                    "confidence": value.confidence,
                }),
            },
            0.0,
        ),
        ModelOutput::Plan(value) => (
            FragmentKind::Plan,
            FragmentContent::Json {
                value: json!({"goal": value.goal, "steps": value.steps}),
            },
            0.0,
        ),
        ModelOutput::DecisionCandidate(value) => (
            FragmentKind::Decision,
            FragmentContent::Json {
                value: json!({"decision": value.decision, "rationale": value.rationale}),
            },
            0.0,
        ),
        ModelOutput::ToolRequest(value) => (
            FragmentKind::ToolCall,
            FragmentContent::Json {
                value: json!({
                    "capability_id": value.capability_id,
                    "arguments": value.arguments,
                }),
            },
            0.0,
        ),
        ModelOutput::FinalResponse(value) => (
            FragmentKind::Claim,
            FragmentContent::Text {
                text: value.text.clone(),
            },
            0.0,
        ),
    };
    let mut fragment = fragment(
        turn,
        config,
        FragmentSpec {
            kind,
            source_type: SourceType::Model,
            perspective: Perspective::System,
            content,
            category: "model_output",
            index: 0,
            confidence,
        },
    );
    fragment.metadata.insert(
        "raw_text".to_owned(),
        Value::String(response.raw_text.clone()),
    );
    fragment
}

struct FragmentSpec {
    kind: FragmentKind,
    source_type: SourceType,
    perspective: Perspective,
    content: FragmentContent,
    category: &'static str,
    index: usize,
    confidence: f32,
}

fn fragment(turn: &HermesTurn, config: &HermesConfig, spec: FragmentSpec) -> CognitiveFragment {
    let observed_at = Utc::now();
    let mut metadata = Metadata::new();
    metadata.insert(
        "hermes_turn_id".to_owned(),
        Value::String(turn.turn_id.clone()),
    );
    metadata.insert(
        "hermes_version".to_owned(),
        Value::String(config.hermes_version.clone()),
    );
    metadata.insert(
        "hermes_interface_version".to_owned(),
        Value::String(config.interface_version.clone()),
    );
    CognitiveFragment {
        id: FragmentId::new(),
        scope: turn.scope.clone(),
        session_id: None,
        trace_id: None,
        kind: spec.kind,
        content: spec.content,
        source: FragmentSource {
            source_type: spec.source_type,
            source_id: source_id(config, &turn.turn_id, spec.category, spec.index),
            actor_id: None,
            observer_id: Some("jc-memory-adapter-hermes".to_owned()),
            tool_id: (spec.source_type == SourceType::Tool).then(|| "hermes.tool".to_owned()),
            uri: None,
        },
        perspective: spec.perspective,
        occurred_at: turn.occurred_at,
        observed_at,
        confidence: spec.confidence.clamp(0.0, 1.0),
        importance: config.default_importance,
        validity: ValidityStatus::Unverified,
        security_label: config.security_label,
        evidence: Vec::new(),
        metadata,
    }
}

fn source_id(config: &HermesConfig, turn_id: &str, category: &str, index: usize) -> String {
    format!(
        "hermes/{}/{}/turn/{turn_id}/{category}/{index}",
        config.hermes_version, config.interface_version
    )
}
