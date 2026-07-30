//! 批次结果与 Checkpoint 预算不变量。

use crate::{JobCheckpoint, JobError, ResourceLimits};

/// 一个 Handler 成功处理批次后提交给 Runner 的结果。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BatchOutcome {
    /// 成功批次之后的游标。
    pub next_cursor: u64,
    /// 是否已经处理完当前 Job。
    pub complete: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BatchUsage {
    pub(crate) processed_items: u32,
    pub(crate) cpu_ms: u64,
    pub(crate) io_ops: u64,
}

impl JobCheckpoint {
    pub(crate) fn advanced(
        &self,
        outcome: BatchOutcome,
        usage: BatchUsage,
        limits: &ResourceLimits,
    ) -> Result<Self, JobError> {
        if usage.processed_items > limits.max_batch_size {
            return Err(JobError::BudgetExceeded("batch_size"));
        }
        if outcome.next_cursor < self.cursor {
            return Err(JobError::InvalidOutcome("cursor_regressed"));
        }
        if usage.processed_items > 0 && outcome.next_cursor == self.cursor {
            return Err(JobError::InvalidOutcome("cursor_did_not_advance"));
        }
        let cpu_ms = self
            .cpu_ms
            .checked_add(usage.cpu_ms)
            .ok_or(JobError::BudgetExceeded("cpu"))?;
        if cpu_ms > limits.max_cpu_ms {
            return Err(JobError::BudgetExceeded("cpu"));
        }
        let io_ops = self
            .io_ops
            .checked_add(usage.io_ops)
            .ok_or(JobError::BudgetExceeded("io"))?;
        if io_ops > limits.max_io_ops {
            return Err(JobError::BudgetExceeded("io"));
        }
        Ok(Self {
            scope: self.scope.clone(),
            cursor: outcome.next_cursor,
            committed_batches: self.committed_batches.saturating_add(1),
            processed_items: self
                .processed_items
                .saturating_add(u64::from(usage.processed_items)),
            cpu_ms,
            io_ops,
            lease: self.lease.clone(),
            next_retry_at_us: None,
        })
    }
}
