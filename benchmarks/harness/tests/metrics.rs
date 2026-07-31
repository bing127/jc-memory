#![allow(missing_docs)]

use std::collections::BTreeSet;
use std::time::Duration;

use jc_memory_activation::{RecallLimits, RecallQuery};
use jc_memory_benchmark_harness::{
    BenchmarkCase, BenchmarkReport, CaseResult, ExpectedAnswer, Metric, RecallAtK, RunnerError,
    percentile_nearest_rank, run_cases,
};
use jc_memory_protocol::{
    Coverage, EvidenceId, EvidenceView, FragmentId, MemoryPacket, MemoryScope, NamespaceId,
    PointId, PointKind, PointView, RecallId, RecallMode, ValidityStatus,
};

fn query(scope: MemoryScope) -> RecallQuery {
    RecallQuery {
        scope,
        mode: RecallMode::Semantic,
        text: "数据库迁移".to_owned(),
        entities: Vec::new(),
        at_time: None,
        limits: RecallLimits {
            max_seeds: 8,
            max_neighbors: 8,
            max_hops: 2,
            max_nodes: 8,
            timeout_ms: 100,
        },
    }
}

fn packet(point_ids: &[PointId], evidence_ids: &[EvidenceId]) -> MemoryPacket {
    let mut packet = MemoryPacket::empty(RecallId::new(), RecallMode::Semantic);
    packet.points = point_ids
        .iter()
        .copied()
        .map(|point_id| PointView {
            point_id,
            kind: PointKind::Fact,
            confidence: 0.9,
            validity: ValidityStatus::Verified,
            source_fragments: vec![FragmentId::new()],
        })
        .collect();
    packet.evidence = evidence_ids
        .iter()
        .copied()
        .map(|evidence_ref_id| EvidenceView {
            fragment_id: FragmentId::new(),
            event_id: None,
            evidence_ref_id: Some(evidence_ref_id),
        })
        .collect();
    packet.coverage = Coverage {
        seed_count: point_ids.len(),
        visited_nodes: point_ids.len(),
        max_depth: 1,
        evidence_count: evidence_ids.len(),
        missing_evidence_count: 0,
        truncated: false,
    };
    packet
}

fn case(expected: ExpectedAnswer, scope: MemoryScope) -> BenchmarkCase {
    BenchmarkCase {
        id: "smoke-1".to_owned(),
        setup: Vec::new(),
        query: query(scope),
        expected,
        tags: BTreeSet::from(["smoke".to_owned()]),
    }
}

fn result(case_id: &str, packet: MemoryPacket, latency_ms: u64) -> CaseResult {
    CaseResult {
        case_id: case_id.to_owned(),
        packet,
        latency: Duration::from_millis(latency_ms),
        database_bytes: 120,
        process_peak_bytes: 240,
        written_fragments: 2,
        write_duration: Some(Duration::from_millis(20)),
        consolidation_duration: Some(Duration::from_millis(4)),
        model_swap_consistent: Some(true),
    }
}

#[test]
fn recall_at_k_and_evidence_coverage_use_known_denominators() {
    let scope = MemoryScope::new(NamespaceId::new());
    let relevant = PointId::new();
    let missing = PointId::new();
    let evidence = EvidenceId::new();
    let missing_evidence = EvidenceId::new();
    let expected = ExpectedAnswer {
        point_ids: BTreeSet::from([relevant, missing]),
        evidence_ids: BTreeSet::from([evidence, missing_evidence]),
        ..ExpectedAnswer::default()
    };
    let benchmark_case = case(expected, scope);
    let benchmark_result = result(&benchmark_case.id, packet(&[relevant], &[evidence]), 10);

    let recall = RecallAtK::new(2).evaluate(
        std::slice::from_ref(&benchmark_case),
        std::slice::from_ref(&benchmark_result),
    );
    assert_eq!(recall.name, "critical_memory_recall_at_5");
    assert!((recall.value - 0.5).abs() < f64::EPSILON);
    assert_eq!(recall.sample_count, 1);

    let coverage = jc_memory_benchmark_harness::EvidenceCoverage.evaluate(
        std::slice::from_ref(&benchmark_case),
        std::slice::from_ref(&benchmark_result),
    );
    assert!((coverage.value - 0.5).abs() < f64::EPSILON);
}

#[test]
fn nearest_rank_percentile_is_deterministic_and_records_samples() {
    let value = percentile_nearest_rank(&[30, 10, 20], 95).expect("非空样本");
    assert_eq!(value, 30);
    assert_eq!(percentile_nearest_rank(&[], 50), None);
}

#[test]
fn report_exposes_all_nineteen_required_metric_names() {
    let scope = MemoryScope::new(NamespaceId::new());
    let point = PointId::new();
    let evidence = EvidenceId::new();
    let expected = ExpectedAnswer {
        point_ids: BTreeSet::from([point]),
        evidence_ids: BTreeSet::from([evidence]),
        answer_correct: Some(true),
        ..ExpectedAnswer::default()
    };
    let benchmark_case = case(expected, scope);
    let benchmark_result = result(&benchmark_case.id, packet(&[point], &[evidence]), 10);
    let report = BenchmarkReport::build(
        std::slice::from_ref(&benchmark_case),
        std::slice::from_ref(&benchmark_result),
    );
    let names: BTreeSet<_> = report
        .metrics
        .iter()
        .map(|metric| metric.name.as_str())
        .collect();
    let expected_names = BTreeSet::from([
        "critical_memory_recall_at_5",
        "evidence_coverage",
        "current_state_accuracy",
        "historical_state_accuracy",
        "temporal_accuracy",
        "causal_path_accuracy",
        "conflict_detection_rate",
        "stale_memory_rate",
        "false_memory_rate",
        "cross_tenant_leakage_rate",
        "source_traceability",
        "abstention_accuracy",
        "model_swap_consistency",
        "write_throughput",
        "recall_p50",
        "recall_p95",
        "recall_p99",
        "storage_amplification",
        "consolidation_cost",
    ]);
    assert_eq!(names, expected_names);
}

#[test]
fn report_json_and_markdown_share_schema_and_percentile_samples() {
    let scope = MemoryScope::new(NamespaceId::new());
    let point = PointId::new();
    let evidence = EvidenceId::new();
    let expected = ExpectedAnswer {
        point_ids: BTreeSet::from([point]),
        evidence_ids: BTreeSet::from([evidence]),
        ..ExpectedAnswer::default()
    };
    let benchmark_case = case(expected, scope);
    let benchmark_result = result(&benchmark_case.id, packet(&[point], &[evidence]), 30);
    let report = BenchmarkReport::build(
        std::slice::from_ref(&benchmark_case),
        std::slice::from_ref(&benchmark_result),
    );
    let json = report.to_json().expect("报告可序列化");
    let decoded: BenchmarkReport = serde_json::from_str(&json).expect("报告可反序列化");
    assert_eq!(decoded, report);
    assert_eq!(decoded.schema_version, "jcmem.benchmark.v1");
    for name in ["recall_p50", "recall_p95", "recall_p99"] {
        let metric = decoded
            .metrics
            .iter()
            .find(|metric| metric.name == name)
            .expect("百分位指标存在");
        assert_eq!(metric.sample_count, 1);
        assert!((metric.value - 30.0).abs() < f64::EPSILON);
    }
    let markdown = report.to_markdown();
    assert!(markdown.contains("# Benchmark 报告"));
    assert!(markdown.contains("`jcmem.benchmark.v1`"));
    assert!(markdown.contains("`recall_p95`"));
}

#[test]
fn runner_preserves_case_order_and_stops_on_executor_error() {
    let scope = MemoryScope::new(NamespaceId::new());
    let first = case(ExpectedAnswer::default(), scope.clone());
    let mut second = case(ExpectedAnswer::default(), scope);
    second.id = "smoke-2".to_owned();
    let cases = vec![first.clone(), second];
    let results = run_cases(&cases, &|benchmark_case: &BenchmarkCase| {
        if benchmark_case.id == first.id {
            Ok(result(&benchmark_case.id, packet(&[], &[]), 1))
        } else {
            Err(RunnerError::Rejected(benchmark_case.id.clone()))
        }
    });
    assert_eq!(results, Err(RunnerError::Rejected("smoke-2".to_owned())));
}
