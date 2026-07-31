//! 按错误类型和幂等性约束的重试策略。

use crate::ToolIdempotency;
use std::time::{Duration, SystemTime};

/// 可分类的执行错误。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RetryClass {
    /// 可暂时恢复的错误。
    Transient,
    /// 受到服务端限流的错误。
    RateLimited,
    /// 不应重试的永久错误。
    Permanent,
    /// 调用已取消。
    Cancelled,
}
/// 重试决策。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RetryDecision {
    /// 在指定时间后重试。
    Retry {
        /// 下一次尝试时间。
        next_attempt_at: SystemTime,
    },
    /// 停止重试。
    Stop,
}
/// 有界重试配置。
#[derive(Clone, Copy, Debug)]
pub struct RetryPolicy {
    max_attempts: u32,
    base_delay: Duration,
    max_delay: Duration,
}
impl RetryPolicy {
    /// 创建重试策略。
    #[must_use]
    pub const fn new(max_attempts: u32, base_delay: Duration, max_delay: Duration) -> Self {
        Self {
            max_attempts,
            base_delay,
            max_delay,
        }
    }
    /// 根据错误、当前尝试次数和幂等性决定是否重试。
    #[must_use]
    pub fn decide(
        &self,
        class: RetryClass,
        attempt: u32,
        idempotency: ToolIdempotency,
    ) -> RetryDecision {
        if attempt >= self.max_attempts
            || idempotency == ToolIdempotency::NonIdempotent
            || !matches!(class, RetryClass::Transient | RetryClass::RateLimited)
        {
            return RetryDecision::Stop;
        }
        let exponent = attempt.saturating_sub(1).min(31);
        let multiplier = 1_u32 << exponent;
        let delay = self
            .base_delay
            .saturating_mul(multiplier)
            .min(self.max_delay);
        RetryDecision::Retry {
            next_attempt_at: SystemTime::now() + delay,
        }
    }
}
