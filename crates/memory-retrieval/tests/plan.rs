//! Recall 查询计划契约。

use jc_memory_protocol::RecallMode;
use jc_memory_retrieval::{AliasDictionary, QueryPlanner, RetrievalLimits};

const DICTIONARY: &str = r"
schema_version: jcmem.retrieval-aliases.v1
dictionary_version: 2026-08-03.test
language: zh-CN
groups:
  - concept: project_codename
    terms: [project codename, codename, 验收代号, 代号]
";

#[test]
fn exact_plan_keeps_one_phrase_without_expansion() {
    let planner = QueryPlanner::new(
        AliasDictionary::from_yaml_str(DICTIONARY).expect("加载词典"),
        RetrievalLimits::default(),
    )
    .expect("创建规划器");

    let plan = planner
        .plan(RecallMode::Exact, " Project Codename ")
        .expect("规划 Exact 查询");

    assert_eq!(plan.exact_phrase.as_deref(), Some("project codename"));
    assert!(plan.alias_terms.is_empty());
    assert!(plan.trigrams.is_empty());
}

#[test]
fn associative_plan_adds_bounded_aliases_and_chinese_trigrams() {
    let limits = RetrievalLimits {
        max_expanded_terms: 3,
        max_trigrams: 5,
    };
    let planner = QueryPlanner::new(
        AliasDictionary::from_yaml_str(DICTIONARY).expect("加载词典"),
        limits,
    )
    .expect("创建规划器");

    let plan = planner
        .plan(RecallMode::Associative, "我的验收代号是什么")
        .expect("规划 Associative 查询");

    assert!(plan.exact_phrase.is_none());
    assert_eq!(plan.alias_terms.len(), 3);
    assert_eq!(plan.trigrams.len(), 5);
    assert!(plan.trigrams.contains(&"验收代".to_owned()));
}
