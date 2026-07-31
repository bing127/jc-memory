//! Tool 风险策略。

use super::{ApprovalRequest, ApprovalStatus, ToolDescriptor};

/// 策略计算结果。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PolicyDecision {
    /// 可以立即执行。
    Allow,
    /// 必须先完成审批。
    RequireApproval,
    /// 永久拒绝。
    Deny,
}

/// 默认拒绝高风险无审批 Tool 的策略引擎。
#[derive(Clone, Copy, Debug, Default)]
pub struct PolicyEngine;

impl PolicyEngine {
    /// 根据描述符和审批状态计算策略决策。
    #[must_use]
    pub fn decide(
        &self,
        descriptor: &ToolDescriptor,
        approval: Option<&ApprovalRequest>,
    ) -> PolicyDecision {
        if descriptor.requires_approval || descriptor.risk >= crate::RiskLevel::High {
            return match approval.map(|item| item.status) {
                Some(ApprovalStatus::Approved) => PolicyDecision::Allow,
                Some(ApprovalStatus::Rejected) => PolicyDecision::Deny,
                Some(ApprovalStatus::Pending) | None => PolicyDecision::RequireApproval,
            };
        }
        PolicyDecision::Allow
    }
}
