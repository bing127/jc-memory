//! Prompt Injection 与跨 Scope 引用检测。

use jc_memory_protocol::{CognitiveFragment, FragmentContent};

use crate::quarantine::{Detection, RiskContext, RiskLabel};

const DETECTOR_VERSION: &str = "injection-rules/1";

pub(crate) fn prompt_injection(fragment: &CognitiveFragment) -> Option<Detection> {
    let text = content_text(&fragment.content).to_lowercase();
    let instruction_override = [
        "ignore previous instructions",
        "ignore all previous instructions",
        "disregard previous instructions",
        "忽略之前的指令",
        "忽略所有先前指令",
    ]
    .iter()
    .any(|marker| text.contains(marker));
    let hidden_prompt_access = ["system prompt", "hidden memory", "隐藏提示词", "系统提示词"]
        .iter()
        .any(|marker| text.contains(marker));
    let disclosure_request = [
        "reveal", "expose", "disclose", "show", "print", "泄露", "显示", "输出",
    ]
    .iter()
    .any(|marker| text.contains(marker));
    (instruction_override || (hidden_prompt_access && disclosure_request)).then_some(Detection {
        label: RiskLabel::PromptInjection,
        score: 0.95,
        detector_version: DETECTOR_VERSION,
        note: "注入规则命中了改变指令或读取隐藏上下文的内容",
    })
}

pub(crate) fn cross_scope_reference(
    _fragment: &CognitiveFragment,
    context: RiskContext,
) -> Option<Detection> {
    context.cross_scope_reference.then_some(Detection {
        label: RiskLabel::CrossScopeReference,
        score: 1.0,
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
