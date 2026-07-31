#![allow(missing_docs)]

use std::collections::BTreeSet;

use jc_memory_benchmark_harness::{cross_year, memory_poisoning, model_swap};
use jc_memory_protocol::{FragmentId, MemoryPacket, RecallId, RecallMode};

#[test]
fn cross_year_generator_is_seed_reproducible_and_manifested() {
    let first = cross_year::generate(7, 32).expect("生成 Cross-Year Fixture");
    let second = cross_year::generate(7, 32).expect("重复生成 Cross-Year Fixture");
    let other = cross_year::generate(8, 32).expect("不同 Seed 生成 Cross-Year Fixture");
    assert_eq!(first.manifest, second.manifest);
    assert_eq!(first.digest(), second.digest());
    assert_ne!(first.digest(), other.digest());
    assert_eq!(first.manifest.seed, 7);
    assert_eq!(first.manifest.case_count, 32);
    let tags: BTreeSet<_> = first
        .cases
        .iter()
        .flat_map(|case| case.tags.iter().cloned())
        .collect();
    for tag in [
        "one_day",
        "one_month",
        "one_year",
        "state_update",
        "ambiguous",
        "same_name",
        "similar_event",
        "rare_fact",
    ] {
        assert!(tags.contains(tag), "缺少 Cross-Year 标签 {tag}");
    }
}

#[test]
fn model_swap_ignores_final_response_but_requires_structured_hash_match() {
    let packet = MemoryPacket::empty(RecallId::new(), RecallMode::Semantic);
    let fragment_ids = vec![FragmentId::new()];
    let first = model_swap::DeterministicModel::new("alpha");
    let second = model_swap::DeterministicModel::new("beta");
    let left = first.infer(&fragment_ids, &packet);
    let right = second.infer(&fragment_ids, &packet);
    assert_ne!(left.final_response, right.final_response);
    assert_eq!(left.canonical_hash, right.canonical_hash);
    assert!(model_swap::compare(&left, &right));
}

#[test]
fn poisoning_fixture_covers_all_required_fail_closed_scenarios() {
    let suite = memory_poisoning::load_fixture(format!(
        "{}/../memory-poisoning/fixtures/smoke.json",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("读取污染 Fixture");
    let kinds: BTreeSet<_> = suite.cases.iter().map(|case| case.kind.as_str()).collect();
    for kind in [
        "injection",
        "cross_tenant",
        "unverified_inference",
        "repeated_error",
        "malicious_tool_result",
        "permission_bypass",
    ] {
        assert!(kinds.contains(kind), "缺少污染场景 {kind}");
    }
    for case in &suite.cases {
        let outcome = memory_poisoning::evaluate(case);
        assert_eq!(outcome, case.expected, "场景 {} 未 fail closed", case.id);
    }
}
