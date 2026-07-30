//! 不可信记忆的风险聚合与准入决策。

use std::collections::BTreeSet;

use jc_memory_protocol::{
    CognitiveFragment, EvidenceId, EvidenceRef, FragmentKind, SourceType, VerificationMethod,
};
use serde::{Deserialize, Serialize};

use crate::{injection, poisoning, sensitive};

const QUARANTINE_THRESHOLD: f32 = 0.8;

/// 检测器为单个 Fragment 产生的稳定风险标签。
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskLabel {
    /// 内容尝试改变上层指令或读取隐藏提示。
    PromptInjection,
    /// 内容或 Evidence 指向调用 Scope 之外。
    CrossScopeReference,
    /// 相同争议声明已经重复出现。
    RepeatedDisputedClaim,
    /// `ToolResult` 自行宣称授权或验证状态。
    SuspiciousToolResult,
    /// 内容包含应隔离处理的敏感数据。
    SensitiveData,
}

/// 一次检测聚合的标签、分数、触发证据和检测器版本。
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct RiskAssessment {
    /// 确定性排序的风险标签。
    pub labels: BTreeSet<RiskLabel>,
    /// 所有命中检测器中的最高风险分数。
    pub score: f32,
    /// 指向原始 Fragment 的触发证据，不写回或覆盖原 `EvidenceRef`。
    pub evidence: Vec<EvidenceRef>,
    /// 实际命中的检测器及其版本。
    pub detector_versions: Vec<String>,
}

impl Default for RiskAssessment {
    fn default() -> Self {
        Self {
            labels: BTreeSet::new(),
            score: 0.0,
            evidence: Vec::new(),
            detector_versions: Vec::new(),
        }
    }
}

/// 需要隔离时保留的首要风险原因。
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub struct QuarantineReason {
    /// 按稳定优先级选择的首要标签。
    pub label: RiskLabel,
    /// 聚合风险分数。
    pub score: f32,
}

/// 风险规则对权威写入的准入决策。
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Admission {
    /// 保留调用方提供的有效性。
    Accept,
    /// 只允许作为未验证信息进入。
    AcceptUnverified,
    /// 保留原始内容并将有效性设为 Quarantined。
    Quarantine {
        /// 导致隔离的稳定原因。
        reason: QuarantineReason,
    },
}

/// Core 在权威存储范围内补充的检测上下文。
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RiskContext {
    /// 同 Scope 内已经存在的相同争议声明数量。
    pub repeated_disputed_claims: usize,
    /// Evidence 是否引用了不同 Scope 的权威记录。
    pub cross_scope_reference: bool,
}

/// 聚合确定性检测器并产生不可绕过的准入决策。
#[derive(Clone, Copy, Debug, Default)]
pub struct QuarantinePipeline;

impl QuarantinePipeline {
    /// 评估一个 Fragment；评估只读取原记录，不修改内容或 `EvidenceRef`。
    #[must_use]
    pub fn assess(self, fragment: &CognitiveFragment, context: RiskContext) -> RiskAssessment {
        let detections = [
            injection::prompt_injection(fragment),
            injection::cross_scope_reference(fragment, context),
            poisoning::repeated_disputed_claim(fragment, context),
            poisoning::suspicious_tool_result(fragment),
            sensitive::sensitive_data(fragment),
        ];
        let mut assessment = RiskAssessment::default();
        for detection in detections.into_iter().flatten() {
            assessment.labels.insert(detection.label);
            assessment.score = assessment.score.max(detection.score);
            assessment
                .detector_versions
                .push(detection.detector_version.to_owned());
            assessment.evidence.push(EvidenceRef {
                id: EvidenceId::new(),
                fragment_id: Some(fragment.id),
                event_id: None,
                method: VerificationMethod::CrossEvidence,
                note: Some(detection.note.to_owned()),
            });
        }
        assessment.detector_versions.sort();
        assessment.detector_versions.dedup();
        assessment
    }

    /// 根据风险评估和来源类型返回准入决策。
    #[must_use]
    pub fn admission(self, fragment: &CognitiveFragment, assessment: &RiskAssessment) -> Admission {
        if assessment.score >= QUARANTINE_THRESHOLD
            && let Some(label) = assessment.labels.iter().next().copied()
        {
            return Admission::Quarantine {
                reason: QuarantineReason {
                    label,
                    score: assessment.score,
                },
            };
        }
        if fragment.source.source_type == SourceType::Model
            || matches!(
                fragment.kind,
                FragmentKind::Inference | FragmentKind::Prediction | FragmentKind::Plan
            )
        {
            Admission::AcceptUnverified
        } else {
            Admission::Accept
        }
    }
}

pub(crate) struct Detection {
    pub(crate) label: RiskLabel,
    pub(crate) score: f32,
    pub(crate) detector_version: &'static str,
    pub(crate) note: &'static str,
}
