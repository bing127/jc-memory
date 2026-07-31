//! Tool 执行与权威 Commit 载荷。

use jc_memory_protocol::AccessContext;
use semver::VersionReq;
use serde_json::Value;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{
    ApprovalRequest, ApprovalStore, PolicyDecision, PolicyEngine, ToolError, ToolRegistry,
};

/// 一次待执行的 Tool 调用。
#[derive(Clone, Debug)]
pub struct ToolInvocation {
    /// Tool 标识。
    pub id: String,
    /// Tool 版本范围。
    pub version: VersionReq,
    /// 结构化参数。
    pub arguments: Value,
    /// 待验证的预测文本。
    pub prediction: Option<String>,
}

/// Commit 中记录的动作。
#[derive(Clone, Debug)]
pub struct ActionRecord {
    /// Tool 标识。
    pub tool_id: String,
    /// 实际参数。
    pub arguments: Value,
}

/// Commit 中记录的 Tool 结果。
#[derive(Clone, Debug)]
pub struct ToolResultRecord {
    /// 是否成功。
    pub success: bool,
    /// 返回值或错误文本。
    pub value: Value,
}

/// Commit 中记录的最终结果。
#[derive(Clone, Debug)]
pub struct OutcomeRecord {
    /// `success` 或 `failure`。
    pub kind: &'static str,
}

/// 预测与实际结果之间的证据关系。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PredictionRelation {
    /// 结果支持预测。
    ValidatedBy,
    /// 结果否定预测。
    DisprovedBy,
}

/// Tool Loop 产生的不可变 Commit 载荷。
#[derive(Clone, Debug)]
pub struct ToolCommit {
    /// 动作记录。
    pub action: ActionRecord,
    /// Tool 结果。
    pub tool_result: ToolResultRecord,
    /// 最终结果。
    pub outcome: OutcomeRecord,
    /// 预测关系；没有预测时为空。
    pub prediction_relation: Option<PredictionRelation>,
    /// 是否写入 Verified Fact，始终为 `false`。
    pub writes_fact: bool,
}

/// Policy、Approval、Execute、Capture、Commit 的顺序闭环。
pub struct ToolLoop {
    registry: ToolRegistry,
    approvals: ApprovalStore,
    policy: PolicyEngine,
}

impl ToolLoop {
    /// 创建 Tool Loop。
    #[must_use]
    pub fn new(registry: ToolRegistry, approvals: ApprovalStore) -> Self {
        Self {
            registry,
            approvals,
            policy: PolicyEngine,
        }
    }

    /// 创建待审批记录，不执行 Tool。
    ///
    /// # Errors
    ///
    /// Tool 不存在、版本不匹配或权限不足时返回错误。
    pub fn request_approval(
        &self,
        access: &AccessContext,
        invocation: &ToolInvocation,
    ) -> Result<ApprovalRequest, ToolError> {
        let resolved = self
            .registry
            .resolve(&invocation.id, &invocation.version, access)?;
        Ok(self.approvals.request(
            resolved.descriptor.id,
            invocation.version.clone(),
            access.scope.clone(),
            access.principal_id.clone(),
        ))
    }

    /// 执行已获准的 Tool，并生成结构化 Commit 载荷。
    ///
    /// # Errors
    ///
    /// Policy 拒绝、审批未完成、权限不足、Tool 取消或注册表解析失败时返回错误。
    pub async fn execute(
        &self,
        access: &AccessContext,
        invocation: ToolInvocation,
        cancellation: CancellationToken,
    ) -> Result<ToolCommit, ToolError> {
        let resolved = self
            .registry
            .resolve(&invocation.id, &invocation.version, access)?;
        let approval = self.approvals.find(
            &invocation.id,
            &invocation.version,
            &access.scope,
            &access.principal_id,
        );
        let decision = self.policy.decide(&resolved.descriptor, approval.as_ref());
        match decision {
            PolicyDecision::Allow => {}
            PolicyDecision::Deny => {
                let id = approval.map_or_else(Uuid::now_v7, |item| item.id);
                return Err(ToolError::ApprovalRejected(id));
            }
            PolicyDecision::RequireApproval => {
                let id = approval.map_or_else(
                    || {
                        self.request_approval(access, &invocation)
                            .map(|item| item.id)
                    },
                    |item| Ok(item.id),
                )?;
                return Err(ToolError::ApprovalRequired(id));
            }
        }
        if cancellation.is_cancelled() {
            return Err(ToolError::Cancelled);
        }
        let action = ActionRecord {
            tool_id: resolved.descriptor.id,
            arguments: invocation.arguments.clone(),
        };
        let result = resolved
            .executor
            .execute(invocation.arguments, cancellation)
            .await;
        match result {
            Ok(value) => Ok(ToolCommit {
                action,
                tool_result: ToolResultRecord {
                    success: true,
                    value,
                },
                outcome: OutcomeRecord { kind: "success" },
                prediction_relation: invocation
                    .prediction
                    .as_ref()
                    .map(|_| PredictionRelation::ValidatedBy),
                writes_fact: false,
            }),
            Err(ToolError::Cancelled) => Err(ToolError::Cancelled),
            Err(ToolError::Execution(error)) => Ok(ToolCommit {
                action,
                tool_result: ToolResultRecord {
                    success: false,
                    value: Value::String(error),
                },
                outcome: OutcomeRecord { kind: "failure" },
                prediction_relation: invocation
                    .prediction
                    .as_ref()
                    .map(|_| PredictionRelation::DisprovedBy),
                writes_fact: false,
            }),
            Err(error) => Err(error),
        }
    }
}
