//! `LoCoMo` Fixture Runner。

use std::path::Path;

use jc_memory_protocol::MemoryPacket;

pub use crate::{BenchmarkCase, BenchmarkReport, CaseExecutor, Suite, SuiteError, SuiteRunError};

/// 读取并校验 `LoCoMo` Fixture。
///
/// # Errors
///
/// 当文件不存在或 Fixture 不符合版本化契约时返回错误。
pub fn load_fixture<P: AsRef<Path>>(path: P) -> Result<Suite, SuiteError> {
    crate::suite::load_suite(path, "locomo")
}

/// 将 `LoCoMo` Fixture 转换为统一 Benchmark Case。
///
/// # Errors
///
/// 当 Fixture 标识无法转换为协议类型时返回错误。
pub fn to_benchmark_cases(suite: &Suite) -> Result<Vec<BenchmarkCase>, SuiteError> {
    suite.to_benchmark_cases()
}

/// 使用注入执行器运行 `LoCoMo` Smoke 并生成报告。
///
/// # Errors
///
/// 当 Fixture 转换或注入执行器失败时返回错误。
pub fn run_smoke<E: CaseExecutor>(
    suite: &Suite,
    executor: &E,
) -> Result<BenchmarkReport, SuiteRunError> {
    suite.run(executor)
}

/// 只用 Packet 的缺证结构判断拒答。
#[must_use]
pub fn is_abstention(packet: &MemoryPacket) -> bool {
    crate::suite::is_abstention(packet)
}
