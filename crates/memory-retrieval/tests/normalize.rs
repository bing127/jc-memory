//! 查询规范化与字符特征契约。

use jc_memory_retrieval::{
    alias_character_trigrams, character_trigrams, lexical_terms, normalize_text, trigram_similarity,
};

#[test]
fn normalization_is_stable_for_chinese_english_and_punctuation() {
    assert_eq!(
        normalize_text("  Project\tCODE Name  "),
        "project code name"
    );
    assert_eq!(normalize_text("我的验收代号是什么？"), "我的验收代号是什么");
    assert_eq!(normalize_text("数据库，Database"), "数据库 database");
    assert_eq!(lexical_terms(" Project  CODE "), vec!["project", "code"]);
}

#[test]
fn chinese_trigrams_are_bounded_and_deterministic() {
    let features = character_trigrams("我的验收代号是什么？", 16);
    assert!(features.contains(&"验收代".to_owned()));
    assert!(features.contains(&"收代号".to_owned()));
    assert_eq!(features, character_trigrams("我的验收代号是什么", 16));
    assert!(character_trigrams("代号", 16).is_empty());
    assert_eq!(character_trigrams("abcdef", 2), vec!["abc", "bcd"]);
}

#[test]
fn ascii_aliases_do_not_generate_character_trigrams() {
    let aliases = vec!["project codename".to_owned(), "codename".to_owned()];

    assert!(alias_character_trigrams(&aliases, 64, 256).is_empty());
}

#[test]
fn chinese_aliases_still_generate_character_trigrams() {
    let aliases = vec!["验收代号".to_owned()];

    assert_eq!(
        alias_character_trigrams(&aliases, 64, 256),
        vec!["收代号", "验收代"]
    );
}

#[test]
fn chinese_trigram_similarity_is_positive_stable_and_excludes_unrelated_text() {
    let query = "我的验收代号是什么";
    let target = "项目验收代号是 JC-DESKTOP-7319";

    let score = trigram_similarity(query, target);
    assert!(score > 0.0);
    assert!(
        score
            .total_cmp(&trigram_similarity("我的验收代号是什么？", target))
            .is_eq()
    );
    assert_eq!(
        trigram_similarity(query, "今天上海天气晴朗")
            .abs()
            .to_bits(),
        0.0_f32.to_bits()
    );
}
