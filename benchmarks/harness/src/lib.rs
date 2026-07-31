//! 统一 Benchmark Case、执行器、指标和报告实现。

mod case;
pub mod cross_year;
pub mod memory_poisoning;
mod metrics;
pub mod model_swap;
mod report;
mod runner;
mod suite;

#[path = "../../locomo/runner.rs"]
pub mod locomo;
#[path = "../../longmemeval/runner.rs"]
pub mod longmemeval;

pub use case::{BenchmarkCase, CaseResult, ExpectedAnswer};
pub use metrics::{
    AbstentionAccuracy, AccuracyMetric, EvidenceCoverage, Metric, MetricValue, RecallAtK,
    percentile_nearest_rank,
};
pub use report::{BenchmarkReport, REPORT_SCHEMA_VERSION};
pub use runner::{CaseExecutor, RunnerError, run_cases};
pub use suite::{Suite, SuiteError, SuiteRunError};
