//! 统一 Benchmark 指标及其确定性计算。

// 指标协议使用 f64；计数和字节数在报告中不会超过 f64 的有效业务精度。
#![allow(clippy::cast_precision_loss)]

use std::collections::{BTreeMap, BTreeSet};

use jc_memory_protocol::{FragmentId, ValidityStatus};

use super::{BenchmarkCase, CaseResult};

/// 一个指标的可序列化结果。
#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct MetricValue {
    /// 稳定的英文指标标识。
    pub name: String,
    /// 指标值；比例在 `[0, 1]`，吞吐和耗时使用文档声明的单位。
    pub value: f64,
    /// 参与分母计算的样本数量。
    pub sample_count: usize,
}

/// 对一组 Case 计算一个指标。
pub trait Metric {
    /// 返回稳定的英文指标标识。
    fn name(&self) -> &'static str;
    /// 计算指标；缺少对应结果的 Case 不进入分母。
    fn evaluate(&self, cases: &[BenchmarkCase], results: &[CaseResult]) -> MetricValue;
}

/// Recall@K 指标。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecallAtK {
    /// 只检查 Packet 前 K 个 Point。
    pub k: usize,
}

impl RecallAtK {
    /// 创建一个固定 K 的 Recall 指标。
    #[must_use]
    pub const fn new(k: usize) -> Self {
        Self { k }
    }
}

impl Metric for RecallAtK {
    fn name(&self) -> &'static str {
        "critical_memory_recall_at_5"
    }

    fn evaluate(&self, cases: &[BenchmarkCase], results: &[CaseResult]) -> MetricValue {
        let result_map = result_map(results);
        let mut sum = 0.0;
        let mut sample_count = 0;
        for case in cases {
            let Some(result) = result_map.get(case.id.as_str()) else {
                continue;
            };
            if case.expected.point_ids.is_empty() {
                continue;
            }
            let retrieved: BTreeSet<_> = result
                .packet
                .points
                .iter()
                .take(self.k)
                .map(|point| point.point_id)
                .collect();
            let hit_count = retrieved.intersection(&case.expected.point_ids).count();
            sum += hit_count as f64 / case.expected.point_ids.len() as f64;
            sample_count += 1;
        }
        value("critical_memory_recall_at_5", sum, sample_count)
    }
}

/// Evidence Coverage 指标。
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct EvidenceCoverage;

impl Metric for EvidenceCoverage {
    fn name(&self) -> &'static str {
        "evidence_coverage"
    }

    fn evaluate(&self, cases: &[BenchmarkCase], results: &[CaseResult]) -> MetricValue {
        let result_map = result_map(results);
        let mut sum = 0.0;
        let mut sample_count = 0;
        for case in cases {
            let Some(result) = result_map.get(case.id.as_str()) else {
                continue;
            };
            if case.expected.evidence_ids.is_empty() {
                continue;
            }
            let actual: BTreeSet<_> = result
                .packet
                .evidence
                .iter()
                .filter_map(|evidence| evidence.evidence_ref_id)
                .collect();
            sum += actual.intersection(&case.expected.evidence_ids).count() as f64
                / case.expected.evidence_ids.len() as f64;
            sample_count += 1;
        }
        value(self.name(), sum, sample_count)
    }
}

/// 共享的答案正确性指标。
#[derive(Clone, Copy, Debug)]
pub struct AccuracyMetric {
    name: &'static str,
}

impl AccuracyMetric {
    /// 当前状态答案准确率。
    pub const CURRENT_STATE: Self = Self {
        name: "current_state_accuracy",
    };
    /// 历史状态答案准确率。
    pub const HISTORICAL_STATE: Self = Self {
        name: "historical_state_accuracy",
    };
    /// 时间关系准确率。
    pub const TEMPORAL: Self = Self {
        name: "temporal_accuracy",
    };
    /// 因果路径准确率。
    pub const CAUSAL: Self = Self {
        name: "causal_path_accuracy",
    };
}

impl Metric for AccuracyMetric {
    fn name(&self) -> &'static str {
        self.name
    }

    fn evaluate(&self, cases: &[BenchmarkCase], results: &[CaseResult]) -> MetricValue {
        let result_map = result_map(results);
        let mut sum = 0.0;
        let mut sample_count = 0;
        for case in cases {
            let Some(result) = result_map.get(case.id.as_str()) else {
                continue;
            };
            let expected = match self.name {
                "current_state_accuracy" => !case.expected.current_state_ids.is_empty(),
                "historical_state_accuracy" => !case.expected.historical_state_ids.is_empty(),
                "temporal_accuracy" => !case.expected.temporal_point_ids.is_empty(),
                "causal_path_accuracy" => !case.expected.causal_point_ids.is_empty(),
                _ => false,
            };
            if !expected {
                continue;
            }
            let actual = case
                .expected
                .answer_correct
                .unwrap_or_else(|| match self.name {
                    "current_state_accuracy" => result
                        .packet
                        .current_states
                        .iter()
                        .any(|state| case.expected.current_state_ids.contains(&state.state_id)),
                    "historical_state_accuracy" => {
                        result.packet.historical_states.iter().any(|state| {
                            case.expected.historical_state_ids.contains(&state.state_id)
                        })
                    }
                    "temporal_accuracy" => {
                        path_contains(&result.packet, &case.expected.temporal_point_ids)
                    }
                    "causal_path_accuracy" => {
                        path_contains(&result.packet, &case.expected.causal_point_ids)
                    }
                    _ => false,
                });
            sum += f64::from(actual);
            sample_count += 1;
        }
        value(self.name(), sum, sample_count)
    }
}

/// 拒答准确率，依据 Packet 的结构化缺证信号判断。
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AbstentionAccuracy;

impl Metric for AbstentionAccuracy {
    fn name(&self) -> &'static str {
        "abstention_accuracy"
    }

    fn evaluate(&self, cases: &[BenchmarkCase], results: &[CaseResult]) -> MetricValue {
        let result_map = result_map(results);
        let mut sum = 0.0;
        let mut sample_count = 0;
        for case in cases {
            let Some(expected) = case.expected.abstention_expected else {
                continue;
            };
            let Some(result) = result_map.get(case.id.as_str()) else {
                continue;
            };
            let actual =
                result.packet.points.is_empty() && !result.packet.missing_evidence.is_empty();
            sum += f64::from(actual == expected);
            sample_count += 1;
        }
        value(self.name(), sum, sample_count)
    }
}

/// 按 nearest-rank 计算百分位，输入单位由调用方决定。
#[must_use]
pub fn percentile_nearest_rank(values: &[u64], percentile: u8) -> Option<u64> {
    if values.is_empty() || !(1..=100).contains(&percentile) {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let rank = (usize::from(percentile) * sorted.len()).div_ceil(100);
    sorted.get(rank.saturating_sub(1)).copied()
}

pub(crate) fn all_metrics(cases: &[BenchmarkCase], results: &[CaseResult]) -> Vec<MetricValue> {
    let metrics: [&dyn Metric; 13] = [
        &RecallAtK::new(5),
        &EvidenceCoverage,
        &AccuracyMetric::CURRENT_STATE,
        &AccuracyMetric::HISTORICAL_STATE,
        &AccuracyMetric::TEMPORAL,
        &AccuracyMetric::CAUSAL,
        &ConflictDetectionRate,
        &StaleMemoryRate,
        &FalseMemoryRate,
        &CrossTenantLeakageRate,
        &SourceTraceability,
        &AbstentionAccuracy,
        &ModelSwapConsistency,
    ];
    let mut values: Vec<_> = metrics
        .iter()
        .map(|metric| metric.evaluate(cases, results))
        .collect();
    values.extend([WriteThroughput.evaluate(cases, results)]);
    let latencies: Vec<u64> = results
        .iter()
        .map(|result| result.latency.as_millis().try_into().unwrap_or(u64::MAX))
        .collect();
    for (name, percentile) in [("recall_p50", 50), ("recall_p95", 95), ("recall_p99", 99)] {
        values.push(value(
            name,
            percentile_nearest_rank(&latencies, percentile).unwrap_or(0) as f64,
            latencies.len(),
        ));
    }
    values.extend([
        StorageAmplification.evaluate(cases, results),
        ConsolidationCost.evaluate(cases, results),
    ]);
    values
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct ConflictDetectionRate;

impl Metric for ConflictDetectionRate {
    fn name(&self) -> &'static str {
        "conflict_detection_rate"
    }
    fn evaluate(&self, cases: &[BenchmarkCase], results: &[CaseResult]) -> MetricValue {
        let result_map = result_map(results);
        let mut sum = 0.0;
        let mut sample_count = 0;
        for case in cases {
            let Some(expected) = case.expected.conflict_detected else {
                continue;
            };
            let Some(result) = result_map.get(case.id.as_str()) else {
                continue;
            };
            sum += f64::from(result.packet.conflicts.is_empty() != expected);
            sample_count += 1;
        }
        value(self.name(), sum, sample_count)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct StaleMemoryRate;

impl Metric for StaleMemoryRate {
    fn name(&self) -> &'static str {
        "stale_memory_rate"
    }
    fn evaluate(&self, cases: &[BenchmarkCase], results: &[CaseResult]) -> MetricValue {
        let result_map = result_map(results);
        let mut stale = 0usize;
        let mut total = 0usize;
        for case in cases {
            let Some(result) = result_map.get(case.id.as_str()) else {
                continue;
            };
            stale += result
                .packet
                .points
                .iter()
                .filter(|point| {
                    matches!(
                        point.validity,
                        ValidityStatus::Superseded | ValidityStatus::Invalidated
                    )
                })
                .count();
            total += result.packet.points.len();
        }
        value(self.name(), ratio(stale, total), total)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct FalseMemoryRate;

impl Metric for FalseMemoryRate {
    fn name(&self) -> &'static str {
        "false_memory_rate"
    }
    fn evaluate(&self, cases: &[BenchmarkCase], results: &[CaseResult]) -> MetricValue {
        let result_map = result_map(results);
        let mut false_count = 0usize;
        let mut total = 0usize;
        for case in cases {
            let Some(result) = result_map.get(case.id.as_str()) else {
                continue;
            };
            false_count += result
                .packet
                .points
                .iter()
                .filter(|point| !case.expected.point_ids.contains(&point.point_id))
                .count();
            total += result.packet.points.len();
        }
        value(self.name(), ratio(false_count, total), total)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct CrossTenantLeakageRate;

impl Metric for CrossTenantLeakageRate {
    fn name(&self) -> &'static str {
        "cross_tenant_leakage_rate"
    }
    fn evaluate(&self, cases: &[BenchmarkCase], results: &[CaseResult]) -> MetricValue {
        let result_map = result_map(results);
        let mut leaked = 0usize;
        let mut total = 0usize;
        for case in cases {
            let Some(result) = result_map.get(case.id.as_str()) else {
                continue;
            };
            let scopes: BTreeMap<FragmentId, _> = case
                .setup
                .iter()
                .map(|fragment| (fragment.id, &fragment.scope))
                .collect();
            for point in &result.packet.points {
                total += 1;
                if point.source_fragments.iter().any(|fragment_id| {
                    scopes
                        .get(fragment_id)
                        .is_some_and(|scope| *scope != &case.query.scope)
                }) {
                    leaked += 1;
                }
            }
        }
        value(self.name(), ratio(leaked, total), total)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct SourceTraceability;

impl Metric for SourceTraceability {
    fn name(&self) -> &'static str {
        "source_traceability"
    }
    fn evaluate(&self, cases: &[BenchmarkCase], results: &[CaseResult]) -> MetricValue {
        let result_map = result_map(results);
        let mut traced = 0usize;
        let mut total = 0usize;
        for case in cases {
            let Some(result) = result_map.get(case.id.as_str()) else {
                continue;
            };
            for point in &result.packet.points {
                total += 1;
                if !point.source_fragments.is_empty() {
                    traced += 1;
                }
            }
        }
        value(self.name(), ratio(traced, total), total)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct ModelSwapConsistency;

impl Metric for ModelSwapConsistency {
    fn name(&self) -> &'static str {
        "model_swap_consistency"
    }
    fn evaluate(&self, cases: &[BenchmarkCase], results: &[CaseResult]) -> MetricValue {
        let result_map = result_map(results);
        let mut sum = 0.0;
        let mut sample_count = 0;
        for case in cases {
            let Some(result) = result_map.get(case.id.as_str()) else {
                continue;
            };
            let Some(consistent) = result.model_swap_consistent else {
                continue;
            };
            sum += f64::from(consistent);
            sample_count += 1;
        }
        value(self.name(), sum, sample_count)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct WriteThroughput;

impl Metric for WriteThroughput {
    fn name(&self) -> &'static str {
        "write_throughput"
    }
    fn evaluate(&self, _cases: &[BenchmarkCase], results: &[CaseResult]) -> MetricValue {
        let mut sum = 0.0;
        let mut sample_count = 0;
        for result in results {
            let Some(duration) = result.write_duration else {
                continue;
            };
            let seconds = duration.as_secs_f64();
            if seconds <= 0.0 {
                continue;
            }
            sum += result.written_fragments as f64 / seconds;
            sample_count += 1;
        }
        value(self.name(), average(sum, sample_count), sample_count)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct StorageAmplification;

impl Metric for StorageAmplification {
    fn name(&self) -> &'static str {
        "storage_amplification"
    }
    fn evaluate(&self, _cases: &[BenchmarkCase], results: &[CaseResult]) -> MetricValue {
        let mut sum = 0.0;
        let mut sample_count = 0;
        for result in results {
            if result.written_fragments == 0 {
                continue;
            }
            sum += result.database_bytes as f64 / result.written_fragments as f64;
            sample_count += 1;
        }
        value(self.name(), average(sum, sample_count), sample_count)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct ConsolidationCost;

impl Metric for ConsolidationCost {
    fn name(&self) -> &'static str {
        "consolidation_cost"
    }
    fn evaluate(&self, _cases: &[BenchmarkCase], results: &[CaseResult]) -> MetricValue {
        let mut sum = 0.0;
        let mut sample_count = 0;
        for result in results {
            let Some(duration) = result.consolidation_duration else {
                continue;
            };
            sum += duration.as_secs_f64() * 1_000.0;
            sample_count += 1;
        }
        value(self.name(), average(sum, sample_count), sample_count)
    }
}

fn result_map(results: &[CaseResult]) -> BTreeMap<&str, &CaseResult> {
    results
        .iter()
        .map(|result| (result.case_id.as_str(), result))
        .collect()
}

fn path_contains(
    packet: &jc_memory_protocol::MemoryPacket,
    expected: &BTreeSet<jc_memory_protocol::PointId>,
) -> bool {
    packet
        .paths
        .iter()
        .any(|path| expected.is_subset(&path.point_ids.iter().copied().collect()))
}

fn value(name: &str, sum: f64, sample_count: usize) -> MetricValue {
    MetricValue {
        name: name.to_owned(),
        value: average(sum, sample_count),
        sample_count,
    }
}

fn average(sum: f64, sample_count: usize) -> f64 {
    if sample_count == 0 {
        0.0
    } else {
        sum / sample_count as f64
    }
}

fn ratio(numerator: usize, denominator: usize) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    }
}
