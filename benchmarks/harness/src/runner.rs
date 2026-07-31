//! Benchmark Case 执行器边界。

use std::fmt;

use super::{BenchmarkCase, CaseResult};

/// 执行一个 Case 的最小注入接口。
pub trait CaseExecutor {
    /// 执行查询并返回可审计的计量结果。
    ///
    /// # Errors
    ///
    /// 执行器无法完成 Case 时返回 `RunnerError`。
    fn execute(&self, case: &BenchmarkCase) -> Result<CaseResult, RunnerError>;
}

impl<F> CaseExecutor for F
where
    F: Fn(&BenchmarkCase) -> Result<CaseResult, RunnerError>,
{
    fn execute(&self, case: &BenchmarkCase) -> Result<CaseResult, RunnerError> {
        self(case)
    }
}

/// Runner 未能完成某个 Case 时的错误。
#[derive(Debug, Eq, PartialEq)]
pub enum RunnerError {
    /// 执行器拒绝了 Case。
    Rejected(String),
}

impl fmt::Display for RunnerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Rejected(message) => write!(formatter, "执行器拒绝 Benchmark Case: {message}"),
        }
    }
}

impl std::error::Error for RunnerError {}

/// 按输入顺序执行一组 Case，任意失败都会停止并保留原始错误。
///
/// # Errors
///
/// 任意注入执行器返回错误时，立即返回该错误。
pub fn run_cases<E: CaseExecutor>(
    cases: &[BenchmarkCase],
    executor: &E,
) -> Result<Vec<CaseResult>, RunnerError> {
    cases.iter().map(|case| executor.execute(case)).collect()
}
