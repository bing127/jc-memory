//! Runtime 完成条件判断。

/// 完成判断输入。
#[derive(Clone, Copy, Debug)]
#[allow(clippy::struct_excessive_bools)]
pub struct CompletionInput {
    /// 目标是否已达成。
    pub goal_met: bool,
    /// 是否仍有待审批动作。
    pub pending_approval: bool,
    /// 是否仍有待执行工具。
    pub pending_tool: bool,
    /// 是否缺少完成所需证据。
    pub missing_evidence: bool,
    /// 已执行步数。
    pub steps: u32,
}
/// 完成判断结果。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompletionStatus {
    /// 满足全部完成条件。
    Completed,
    /// 尚未满足指定条件。
    Incomplete(&'static str),
    /// 达到最大步数。
    MaxSteps,
}
/// 检查目标、依赖和步数的完成判断器。
#[derive(Clone, Copy, Debug)]
pub struct CompletionJudge {
    /// 允许的最大步数。
    pub max_steps: u32,
}
impl CompletionJudge {
    /// 判断 Runtime 是否可以完成。
    #[must_use]
    pub fn judge(&self, input: CompletionInput) -> CompletionStatus {
        if input.steps >= self.max_steps {
            return CompletionStatus::MaxSteps;
        }
        if !input.goal_met {
            return CompletionStatus::Incomplete("goal_not_met");
        }
        if input.pending_approval {
            return CompletionStatus::Incomplete("pending_approval");
        }
        if input.pending_tool {
            return CompletionStatus::Incomplete("pending_tool");
        }
        if input.missing_evidence {
            return CompletionStatus::Incomplete("missing_evidence");
        }
        CompletionStatus::Completed
    }
}
impl Default for CompletionJudge {
    fn default() -> Self {
        Self { max_steps: 100 }
    }
}
