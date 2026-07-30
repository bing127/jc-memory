//! 核心变更命令及其追加结果。

use chrono::{DateTime, Utc};
use jc_memory_protocol::{
    EdgeKind, EventId, FragmentContent, FragmentId, FragmentSource, Perspective, VerificationMethod,
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
