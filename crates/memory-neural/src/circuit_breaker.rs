//! 使用显式时间驱动的三态熔断和超时降级。

use std::{future::Future, time::Duration};

use serde::{Deserialize, Serialize};

use crate::NeuralError;

/// 熔断器状态。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CircuitState {
    /// 正常放行请求。
    Closed,
    /// 冷却期内拒绝主 Provider 请求。
    Open,
    /// 冷却后只允许一个探测请求。
    HalfOpen,
}

/// 触发确定性降级的原因。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FallbackReason {
    /// 主 Provider 超时。
    Timeout,
    /// 主 Provider 返回错误。
    PrimaryFailure,
    /// 熔断器处于打开状态。
    CircuitOpen,
}

/// 主调用或确定性降级的统一结果。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FallbackResult<T> {
    /// 最终返回值。
    pub value: T,
    /// 未使用主 Provider 时的原因。
    pub reason: Option<FallbackReason>,
}

/// 使用调用方提供的单调时间驱动三态转换的熔断器。
#[derive(Clone, Debug)]
pub struct CircuitBreaker {
    state: CircuitState,
    cooldown: Duration,
    opened_at: Option<Duration>,
    half_open_in_flight: bool,
}

impl CircuitBreaker {
    /// 创建初始关闭的熔断器。
    #[must_use]
    pub const fn new(cooldown: Duration) -> Self {
        Self {
            state: CircuitState::Closed,
            cooldown,
            opened_at: None,
            half_open_in_flight: false,
        }
    }

    /// 返回当前状态。
    #[must_use]
    pub const fn state(&self) -> CircuitState {
        self.state
    }

    /// 判断当前时刻是否允许调用主 Provider。
    pub fn allow_request(&mut self, now: Duration) -> bool {
        match self.state {
            CircuitState::Closed => true,
            CircuitState::Open => {
                let cooled_down = self
                    .opened_at
                    .is_some_and(|opened_at| now.saturating_sub(opened_at) >= self.cooldown);
                if cooled_down {
                    self.state = CircuitState::HalfOpen;
                    self.half_open_in_flight = true;
                    true
                } else {
                    false
                }
            }
            CircuitState::HalfOpen => {
                if self.half_open_in_flight {
                    false
                } else {
                    self.half_open_in_flight = true;
                    true
                }
            }
        }
    }

    /// 记录一次成功并关闭熔断器。
    pub fn record_success(&mut self) {
        self.state = CircuitState::Closed;
        self.opened_at = None;
        self.half_open_in_flight = false;
    }

    /// 记录一次失败并重新开始冷却期。
    pub fn record_failure(&mut self, now: Duration) {
        self.state = CircuitState::Open;
        self.opened_at = Some(now);
        self.half_open_in_flight = false;
    }
}

/// 在主调用超时、失败或熔断时执行确定性降级。
///
/// # Errors
///
/// 仅当确定性降级本身失败时返回错误。
pub async fn with_timeout_fallback<T, PrimaryFuture, Fallback, FallbackFuture>(
    breaker: &mut CircuitBreaker,
    now: Duration,
    timeout: Duration,
    primary: PrimaryFuture,
    fallback: Fallback,
) -> Result<FallbackResult<T>, NeuralError>
where
    PrimaryFuture: Future<Output = Result<T, NeuralError>>,
    Fallback: FnOnce() -> FallbackFuture,
    FallbackFuture: Future<Output = Result<T, NeuralError>>,
{
    if !breaker.allow_request(now) {
        return fallback_result(fallback, FallbackReason::CircuitOpen).await;
    }
    match tokio::time::timeout(timeout, primary).await {
        Ok(Ok(value)) => {
            breaker.record_success();
            Ok(FallbackResult {
                value,
                reason: None,
            })
        }
        Ok(Err(_)) => {
            breaker.record_failure(now);
            fallback_result(fallback, FallbackReason::PrimaryFailure).await
        }
        Err(_) => {
            breaker.record_failure(now);
            fallback_result(fallback, FallbackReason::Timeout).await
        }
    }
}

async fn fallback_result<T, Fallback, FallbackFuture>(
    fallback: Fallback,
    reason: FallbackReason,
) -> Result<FallbackResult<T>, NeuralError>
where
    Fallback: FnOnce() -> FallbackFuture,
    FallbackFuture: Future<Output = Result<T, NeuralError>>,
{
    fallback()
        .await
        .map(|value| FallbackResult {
            value,
            reason: Some(reason),
        })
        .map_err(|error| NeuralError::Fallback(error.to_string()))
}
