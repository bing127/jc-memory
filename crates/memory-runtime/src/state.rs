//! Runtime 状态与纯转换规则。

use serde::{Deserialize, Serialize};

/// Agent Runtime 的持久化状态。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeState {
    /// 正在接收并记录输入。
    Observing,
    /// 正在召回相关记忆。
    Recalling,
    /// 正在形成候选推理。
    Reasoning,
    /// 正在等待审批决策。
    AwaitingApproval,
    /// 正在执行已获准的动作。
    Executing,
    /// 正在提交经过验证的结果。
    Committing,
    /// 正在等待用户输入。
    WaitingUser,
    /// 运行已暂停。
    Paused,
    /// 运行已完成。
    Completed,
    /// 运行已失败。
    Failed,
    /// 运行已取消。
    Cancelled,
}

impl RuntimeState {
    /// 根据事件计算下一状态，不执行任何副作用。
    ///
    /// # Errors
    ///
    /// 当前状态不接受该事件时返回 [`TransitionError::Invalid`]。
    pub const fn transition(self, event: RuntimeEvent) -> Result<Self, TransitionError> {
        match self {
            Self::Observing => from_observing(event),
            Self::Recalling => from_recalling(event),
            Self::Reasoning => from_reasoning(event),
            Self::AwaitingApproval => from_awaiting_approval(event),
            Self::Executing => from_executing(event),
            Self::Committing => from_committing(event),
            Self::WaitingUser | Self::Paused => from_inactive(event),
            Self::Completed | Self::Failed | Self::Cancelled => from_terminal(event),
        }
    }

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Observing => "observing",
            Self::Recalling => "recalling",
            Self::Reasoning => "reasoning",
            Self::AwaitingApproval => "awaiting_approval",
            Self::Executing => "executing",
            Self::Committing => "committing",
            Self::WaitingUser => "waiting_user",
            Self::Paused => "paused",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "observing" => Some(Self::Observing),
            "recalling" => Some(Self::Recalling),
            "reasoning" => Some(Self::Reasoning),
            "awaiting_approval" => Some(Self::AwaitingApproval),
            "executing" => Some(Self::Executing),
            "committing" => Some(Self::Committing),
            "waiting_user" => Some(Self::WaitingUser),
            "paused" => Some(Self::Paused),
            "completed" => Some(Self::Completed),
            "failed" => Some(Self::Failed),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }
}

const fn from_observing(event: RuntimeEvent) -> Result<RuntimeState, TransitionError> {
    match event {
        RuntimeEvent::Recall => Ok(RuntimeState::Recalling),
        RuntimeEvent::Fail => Ok(RuntimeState::Failed),
        RuntimeEvent::Cancel => Ok(RuntimeState::Cancelled),
        RuntimeEvent::Reason
        | RuntimeEvent::RequestApproval
        | RuntimeEvent::Approve
        | RuntimeEvent::Execute
        | RuntimeEvent::Commit
        | RuntimeEvent::WaitForUser
        | RuntimeEvent::Complete => Err(TransitionError::Invalid),
    }
}

const fn from_recalling(event: RuntimeEvent) -> Result<RuntimeState, TransitionError> {
    match event {
        RuntimeEvent::Reason => Ok(RuntimeState::Reasoning),
        RuntimeEvent::Fail => Ok(RuntimeState::Failed),
        RuntimeEvent::Cancel => Ok(RuntimeState::Cancelled),
        RuntimeEvent::Recall
        | RuntimeEvent::RequestApproval
        | RuntimeEvent::Approve
        | RuntimeEvent::Execute
        | RuntimeEvent::Commit
        | RuntimeEvent::WaitForUser
        | RuntimeEvent::Complete => Err(TransitionError::Invalid),
    }
}

const fn from_reasoning(event: RuntimeEvent) -> Result<RuntimeState, TransitionError> {
    match event {
        RuntimeEvent::RequestApproval => Ok(RuntimeState::AwaitingApproval),
        RuntimeEvent::Execute => Ok(RuntimeState::Executing),
        RuntimeEvent::WaitForUser => Ok(RuntimeState::WaitingUser),
        RuntimeEvent::Fail => Ok(RuntimeState::Failed),
        RuntimeEvent::Cancel => Ok(RuntimeState::Cancelled),
        RuntimeEvent::Recall
        | RuntimeEvent::Reason
        | RuntimeEvent::Approve
        | RuntimeEvent::Commit
        | RuntimeEvent::Complete => Err(TransitionError::Invalid),
    }
}

const fn from_awaiting_approval(event: RuntimeEvent) -> Result<RuntimeState, TransitionError> {
    match event {
        RuntimeEvent::Approve => Ok(RuntimeState::Executing),
        RuntimeEvent::Fail => Ok(RuntimeState::Failed),
        RuntimeEvent::Cancel => Ok(RuntimeState::Cancelled),
        RuntimeEvent::Recall
        | RuntimeEvent::Reason
        | RuntimeEvent::RequestApproval
        | RuntimeEvent::Execute
        | RuntimeEvent::Commit
        | RuntimeEvent::WaitForUser
        | RuntimeEvent::Complete => Err(TransitionError::Invalid),
    }
}

const fn from_executing(event: RuntimeEvent) -> Result<RuntimeState, TransitionError> {
    match event {
        RuntimeEvent::Commit => Ok(RuntimeState::Committing),
        RuntimeEvent::Fail => Ok(RuntimeState::Failed),
        RuntimeEvent::Cancel => Ok(RuntimeState::Cancelled),
        RuntimeEvent::Recall
        | RuntimeEvent::Reason
        | RuntimeEvent::RequestApproval
        | RuntimeEvent::Approve
        | RuntimeEvent::Execute
        | RuntimeEvent::WaitForUser
        | RuntimeEvent::Complete => Err(TransitionError::Invalid),
    }
}

const fn from_committing(event: RuntimeEvent) -> Result<RuntimeState, TransitionError> {
    match event {
        RuntimeEvent::Complete => Ok(RuntimeState::Completed),
        RuntimeEvent::Fail => Ok(RuntimeState::Failed),
        RuntimeEvent::Cancel => Ok(RuntimeState::Cancelled),
        RuntimeEvent::Recall
        | RuntimeEvent::Reason
        | RuntimeEvent::RequestApproval
        | RuntimeEvent::Approve
        | RuntimeEvent::Execute
        | RuntimeEvent::Commit
        | RuntimeEvent::WaitForUser => Err(TransitionError::Invalid),
    }
}

const fn from_inactive(event: RuntimeEvent) -> Result<RuntimeState, TransitionError> {
    match event {
        RuntimeEvent::Fail => Ok(RuntimeState::Failed),
        RuntimeEvent::Cancel => Ok(RuntimeState::Cancelled),
        RuntimeEvent::Recall
        | RuntimeEvent::Reason
        | RuntimeEvent::RequestApproval
        | RuntimeEvent::Approve
        | RuntimeEvent::Execute
        | RuntimeEvent::Commit
        | RuntimeEvent::WaitForUser
        | RuntimeEvent::Complete => Err(TransitionError::Invalid),
    }
}

const fn from_terminal(event: RuntimeEvent) -> Result<RuntimeState, TransitionError> {
    match event {
        RuntimeEvent::Recall
        | RuntimeEvent::Reason
        | RuntimeEvent::RequestApproval
        | RuntimeEvent::Approve
        | RuntimeEvent::Execute
        | RuntimeEvent::Commit
        | RuntimeEvent::WaitForUser
        | RuntimeEvent::Complete
        | RuntimeEvent::Fail
        | RuntimeEvent::Cancel => Err(TransitionError::Invalid),
    }
}

/// 触发 Runtime 状态变化的事件。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeEvent {
    /// 开始召回。
    Recall,
    /// 开始推理。
    Reason,
    /// 请求审批。
    RequestApproval,
    /// 审批通过。
    Approve,
    /// 开始执行无需审批或已经获批的动作。
    Execute,
    /// 开始提交执行结果。
    Commit,
    /// 等待用户补充输入。
    WaitForUser,
    /// 完成已提交的运行。
    Complete,
    /// 标记运行失败。
    Fail,
    /// 取消运行。
    Cancel,
}

/// 纯状态转换错误。
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum TransitionError {
    /// 当前状态不允许请求的事件。
    #[error("invalid runtime state transition")]
    Invalid,
}
