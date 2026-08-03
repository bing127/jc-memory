//! Prompt Injection 与跨 Scope 引用检测。

use jc_memory_protocol::{CognitiveFragment, FragmentContent};

use crate::{
    policy::QuarantinePolicy,
    quarantine::{Detection, RiskContext, RiskLabel},
};

const DETECTOR_VERSION: &str = "injection-rules/1";

pub(crate) fn prompt_injection(
    fragment: &CognitiveFragment,
    policy: &QuarantinePolicy,
) -> Option<Detection> {
    let text = content_text(&fragment.content).to_lowercase();
    let rules = policy.prompt_injection();
    let instruction_override = rules.matches_instruction_override(&text);
    let hidden_prompt_access = rules.matches_hidden_prompt(&text);
    let disclosure_request = rules.matches_disclosure(&text);
    (instruction_override || (hidden_prompt_access && disclosure_request)).then_some(Detection {
        label: RiskLabel::PromptInjection,
        score: rules.score(),
        detector_version: DETECTOR_VERSION,
        note: "注入规则命中了改变指令或读取隐藏上下文的内容",
    })
}

pub(crate) fn cross_scope_reference(
    _fragment: &CognitiveFragment,
    context: RiskContext,
    policy: &QuarantinePolicy,
) -> Option<Detection> {
    context.cross_scope_reference.then_some(Detection {
        label: RiskLabel::CrossScopeReference,
        score: policy.cross_scope_reference_score(),
        detector_version: DETECTOR_VERSION,
        note: "EvidenceRef 指向了当前精确 Scope 之外的权威记录",
    })
}

pub(crate) fn content_text(content: &FragmentContent) -> String {
    match content {
        FragmentContent::Text { text } => text.clone(),
        FragmentContent::Json { value } => value.to_string(),
        FragmentContent::Blob { .. } => String::new(),
    }
}
