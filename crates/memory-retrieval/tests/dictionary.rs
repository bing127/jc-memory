//! 版本化业务别名词典契约。

use jc_memory_retrieval::AliasDictionary;

const DICTIONARY: &str = r"
schema_version: jcmem.retrieval-aliases.v1
dictionary_version: 2026-08-03.test
language: zh-CN
groups:
  - concept: project_codename
    terms:
      - project codename
      - codename
      - 验收代号
      - 代号
  - concept: preferred_color
    terms:
      - preferred color
      - 首选颜色
      - 喜欢什么颜色
";

#[test]
fn dictionary_expands_bilingual_concepts_in_stable_order() {
    let dictionary = AliasDictionary::from_yaml_str(DICTIONARY).expect("加载测试词典");

    assert_eq!(dictionary.version(), "2026-08-03.test");
    assert_eq!(dictionary.sha256().len(), 64);
    assert_eq!(
        dictionary.expand("我的验收代号是什么", 8),
        vec!["codename", "project codename", "代号", "验收代号"]
    );
    assert_eq!(
        dictionary.expand("preferred color", 8),
        vec!["preferred color", "喜欢什么颜色", "首选颜色"]
    );
}

#[test]
fn dictionary_rejects_unknown_fields_duplicates_and_empty_terms() {
    let unknown = DICTIONARY.replace("language: zh-CN", "language: zh-CN\nunknown: true");
    assert!(AliasDictionary::from_yaml_str(&unknown).is_err());

    let duplicate = DICTIONARY.replace(
        "  - concept: preferred_color",
        "  - concept: project_codename",
    );
    assert!(AliasDictionary::from_yaml_str(&duplicate).is_err());

    let empty = DICTIONARY.replace("      - codename", "      - \"\"");
    assert!(AliasDictionary::from_yaml_str(&empty).is_err());
}

#[test]
fn dictionary_digest_is_reproducible() {
    let first = AliasDictionary::from_yaml_str(DICTIONARY).expect("第一次加载词典");
    let second = AliasDictionary::from_yaml_str(DICTIONARY).expect("第二次加载词典");
    assert_eq!(first.sha256(), second.sha256());
}
