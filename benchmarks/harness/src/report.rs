//! Benchmark JSON 和中文 Markdown 报告。

use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

use super::{BenchmarkCase, CaseResult};
use crate::metrics::all_metrics;

/// 当前报告 Schema 版本。
pub const REPORT_SCHEMA_VERSION: &str = "jcmem.benchmark.v1";

/// 可审计的 Benchmark 报告。
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct BenchmarkReport {
    /// 报告 Schema 标识。
    pub schema_version: String,
    /// 参与报告的 Case 数量。
    pub case_count: usize,
    /// 指标结果，包含原始样本数。
    pub metrics: Vec<crate::MetricValue>,
}

impl BenchmarkReport {
    /// 根据 Case 和执行结果生成报告。
    #[must_use]
    pub fn build(cases: &[BenchmarkCase], results: &[CaseResult]) -> Self {
        Self {
            schema_version: REPORT_SCHEMA_VERSION.to_owned(),
            case_count: cases.len(),
            metrics: all_metrics(cases, results),
        }
    }

    /// 生成格式稳定的 JSON 文本。
    ///
    /// # Errors
    ///
    /// 当报告字段无法被 `serde_json` 序列化时返回错误。
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// 生成中文 Markdown 摘要；数值来源仍以 JSON 为准。
    #[must_use]
    pub fn to_markdown(&self) -> String {
        let mut markdown = format!(
            "# Benchmark 报告\n\n- Schema：`{}`\n- Case 数量：{}\n\n| 指标 | 数值 | 样本数 |\n| --- | ---: | ---: |\n",
            self.schema_version, self.case_count
        );
        for metric in &self.metrics {
            let _ = writeln!(
                markdown,
                "| `{}` | {:.6} | {} |",
                metric.name, metric.value, metric.sample_count
            );
        }
        markdown
    }
}
