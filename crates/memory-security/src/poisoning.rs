//! 重复争议声明和可疑 `ToolResult` 检测。

use jc_memory_protocol::{CognitiveFragment, FragmentKind, SourceType, ValidityStatus};

use crate::{
    injection::content_text,
    quarantine::{Detection, RiskContext, RiskLabel},
};

const DETECTOR_VERSION: &str = "poisoning-rules/1";
const REPEATED_DISPUTED_THRESHOLD: usize = 2;

pub(crate) fn repeated_disputed_claim(
    fragment: &CognitiveFragment,
    context: RiskContext,
) -> Option<Detection> {
    (matches!(fragment.kind, FragmentKind::Claim | FragmentKind::Fact)
        && fragment.validity == ValidityStatus::Disputed
        && context.repeated_disputed_claims >= REPEATED_DISPUTED_THRESHOLD)
        .then_some(Detection {
            label: RiskLabel::RepeatedDisputedClaim,
            score: 0.85,
            detector_version: DETECTOR_VERSION,
            note: "同一精确 Scope 已重复出现相同的争议声明",
        })
}

pub(crate) fn suspicious_tool_result(fragment: &CognitiveFragment) -> Option<Detection> {
    if fragment.kind != FragmentKind::ToolResult || fragment.source.source_type != SourceType::Tool
    {
        return None;
    }
    let text = content_text(&fragment.content).to_lowercase();
    let self_verified = text.contains("\"verified\":true")
        || text.contains("\"verified\": true")
        || text.contains("verified=true");
    let self_authorized = text.contains("\"authorization\":\"granted\"")
        || text.contains("\"authorization\": \"granted\"")
        || text.contains("authorization=granted");
    (self_verified || self_authorized).then_some(Detection {
        label: RiskLabel::SuspiciousToolResult,
        score: 0.9,
        detector_version: DETECTOR_VERSION,
        note: "ToolResult 自行声明了验证或授权状态",
    })
}
