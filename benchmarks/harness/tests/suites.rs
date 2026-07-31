#![allow(missing_docs)]

use std::{collections::BTreeSet, time::Duration};

use jc_memory_benchmark_harness::{BenchmarkCase, CaseResult, locomo, longmemeval};
use jc_memory_protocol::{MemoryPacket, MissingEvidence, PointId};

fn fixture_path(dataset: &str) -> String {
    format!(
        "{}/../{dataset}/fixtures/smoke.json",
        env!("CARGO_MANIFEST_DIR")
    )
}

#[test]
fn longmemeval_fixture_preserves_case_ids_and_required_smoke_tags() {
    let suite =
        longmemeval::load_fixture(fixture_path("longmemeval")).expect("读取 LongMemEval Fixture");
    assert_eq!(suite.dataset, "longmemeval");
    assert!(suite.smoke);
    assert_eq!(suite.cases.len(), 7);
    assert!(
        suite
            .cases
            .iter()
            .any(|case| case.id == "longmemeval-cross-session")
    );
    let tags: BTreeSet<_> = suite
        .cases
        .iter()
        .flat_map(|case| case.tags.iter().cloned())
        .collect();
    for tag in [
        "information_extraction",
        "cross_session",
        "temporal",
        "update",
        "abstention",
        "single_hop",
        "multi_hop",
        "causal_failure",
    ] {
        assert!(tags.contains(tag), "缺少标签 {tag}");
    }
    let cases = suite.to_benchmark_cases().expect("转换 BenchmarkCase");
    assert_eq!(cases[1].id, "longmemeval-cross-session");
    assert_eq!(cases[1].setup.len(), 2);
}

#[test]
fn locomo_fixture_preserves_sessions_and_structured_abstention_expectation() {
    let suite = locomo::load_fixture(fixture_path("locomo")).expect("读取 LoCoMo Fixture");
    assert_eq!(suite.dataset, "locomo");
    assert!(suite.smoke);
    let cases = suite.to_benchmark_cases().expect("转换 BenchmarkCase");
    let abstention = cases
        .iter()
        .find(|case| case.id == "locomo-abstention")
        .expect("拒答 Case 存在");
    assert_eq!(abstention.expected.abstention_expected, Some(true));
    assert_eq!(abstention.setup.len(), 1);
    assert_eq!(cases[1].setup.len(), 2);
}

#[test]
fn abstention_uses_missing_evidence_and_packet_shape_not_answer_text() {
    let mut packet = MemoryPacket::empty(
        jc_memory_protocol::RecallId::new(),
        jc_memory_protocol::RecallMode::Exact,
    );
    assert!(!longmemeval::is_abstention(&packet));
    packet.missing_evidence.push(MissingEvidence {
        point_id: PointId::new(),
        expected_edge_kinds: Vec::new(),
        description: "Fixture 中没有该字段".to_owned(),
    });
    assert!(longmemeval::is_abstention(&packet));
    assert!(locomo::is_abstention(&packet));
}

#[test]
fn smoke_runner_delegates_execution_and_builds_versioned_report() {
    let suite = longmemeval::load_fixture(fixture_path("longmemeval")).expect("读取 Fixture");
    let report = longmemeval::run_smoke(&suite, &|case: &BenchmarkCase| {
        Ok(CaseResult {
            case_id: case.id.clone(),
            packet: MemoryPacket::empty(jc_memory_protocol::RecallId::new(), case.query.mode),
            latency: Duration::from_millis(1),
            database_bytes: 0,
            process_peak_bytes: 0,
            written_fragments: case.setup.len() as u64,
            write_duration: None,
            consolidation_duration: None,
            model_swap_consistent: None,
        })
    })
    .expect("运行 Smoke");
    assert_eq!(report.schema_version, "jcmem.benchmark.v1");
    assert_eq!(report.case_count, 7);
}
