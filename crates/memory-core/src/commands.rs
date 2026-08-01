//! 核心变更命令及其追加结果。

use chrono::{DateTime, Utc};
use jc_memory_protocol::{
    EdgeKind, EventId, FragmentContent, FragmentId, FragmentSource, Perspective, StateDeclaration,
    VerificationMethod,
};
use serde::{Deserialize, Serialize};

/// 权威记忆变更类型。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MutationKind {
    /// 追加一条纠正记录。
    Correct,
    /// 追加支持目标记忆的新证据。
    Reinforce,
    /// 追加降低目标可信度的证据。
    Weaken,
    /// 追加替代目标记忆的新版本。
    Supersede,
    /// 追加与目标冲突的证据。
    Contradict,
    /// 追加失效记录，而不是删除原始记录。
    Forget,
}

/// 创建一条证据保留型变更所需的调用方输入。
#[derive(Clone, Debug, PartialEq)]
pub struct MutationCommand {
    /// 被变更的权威 Fragment。
    pub target_fragment_id: FragmentId,
    /// 新变更 Fragment 的原始内容。
    pub content: FragmentContent,
    /// 可选的显式 State 新值或竞争值。
    pub state: Option<StateDeclaration>,
    /// 发起变更的来源信息。
    pub source: FragmentSource,
    /// 来源观察视角。
    pub perspective: Perspective,
    /// 变更在来源系统中的发生时间。
    pub occurred_at: DateTime<Utc>,
    /// 验证目标引用所使用的方法。
    pub verification_method: VerificationMethod,
    /// 可审计的变更原因。
    pub reason: String,
}

/// 一次已持久化权威变更的结果。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MutationReceipt {
    /// 已执行的变更类型。
    pub kind: MutationKind,
    /// 被引用但未被覆盖的原始 Fragment。
    pub target_fragment_id: FragmentId,
    /// 新追加的变更 Fragment。
    pub fragment_id: FragmentId,
    /// 承载变更 Fragment 的新 Event。
    pub event_id: EventId,
    /// 在精确 Scope 内分配的 Event 序号。
    pub sequence: u64,
    /// 派生图重建时应建立的关系类型。
    pub relation: Option<EdgeKind>,
}

/// 授权审查对 Quarantine 内容作出的决定。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewDecision {
    /// 允许原 Fragment 以未验证状态重新参与默认 Recall。
    Release,
    /// 确认原 Fragment 应继续隔离。
    Reject,
}

/// 审查一个 Quarantine Fragment 的调用方输入。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuarantineReview {
    /// 被审查且不会被改写的原始 Fragment。
    pub target_fragment_id: FragmentId,
    /// 授权审查决定。
    pub decision: ReviewDecision,
    /// 写入审计和状态 Event 的非空理由。
    pub reason: String,
}

/// 一次追加式 Quarantine 审查的持久化结果。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuarantineReviewReceipt {
    /// 被审查的原始 Fragment。
    pub target_fragment_id: FragmentId,
    /// 承载审计记录的权威 Event。
    pub audit_event_id: EventId,
    /// 承载最新 Quarantine 状态的权威 Event。
    pub state_event_id: EventId,
    /// 已执行的审查决定。
    pub decision: ReviewDecision,
}
