//! 统一 Benchmark Case、执行器、指标和报告实现。

mod case;
mod metrics;
mod report;
mod runner;

pub use case::{BenchmarkCase, CaseResult, ExpectedAnswer};
pub use metrics::{
    AbstentionAccuracy, AccuracyMetric, EvidenceCoverage, Metric, MetricValue, RecallAtK,
    percentile_nearest_rank,
};
pub use report::{BenchmarkReport, REPORT_SCHEMA_VERSION};
pub use runner::{CaseExecutor, RunnerError, run_cases};
