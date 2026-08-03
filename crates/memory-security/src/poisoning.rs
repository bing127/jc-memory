//! 重复争议声明和可疑 `ToolResult` 检测。

use jc_memory_protocol::{CognitiveFragment, FragmentKind, SourceType, ValidityStatus};

use crate::{
    injection::content_text,
    policy::QuarantinePolicy,
    quarantine::{Detection, RiskContext, RiskLabel},
};

const DETECTOR_VERSION: &str = "poisoning-rules/1";

pub(crate) fn repeated_disputed_claim(
    fragment: &CognitiveFragment,
    context: RiskContext,
    policy: &QuarantinePolicy,
) -> Option<Detection> {
    (matches!(fragment.kind, FragmentKind::Claim | FragmentKind::Fact)
        && fragment.validity == ValidityStatus::Disputed
        && context.repeated_disputed_claims >= policy.repeated_disputed_threshold())
    .then_some(Detection {
        label: RiskLabel::RepeatedDisputedClaim,
        score: policy.repeated_disputed_claim_score(),
        detector_version: DETECTOR_VERSION,
        note: "同一精确 Scope 已重复出现相同的争议声明",
    })
}

pub(crate) fn suspicious_tool_result(
    fragment: &CognitiveFragment,
    policy: &QuarantinePolicy,
) -> Option<Detection> {
    if fragment.kind != FragmentKind::ToolResult || fragment.source.source_type != SourceType::Tool
    {
        return None;
    }
    let text = content_text(&fragment.content).to_lowercase();
    let rules = policy.suspicious_tool_result();
    let self_verified = rules.matches_verified(&text);
    let self_authorized = rules.matches_authorization(&text);
    (self_verified || self_authorized).then_some(Detection {
        label: RiskLabel::SuspiciousToolResult,
        score: rules.score(),
        detector_version: DETECTOR_VERSION,
        note: "ToolResult 自行声明了验证或授权状态",
    })
}
