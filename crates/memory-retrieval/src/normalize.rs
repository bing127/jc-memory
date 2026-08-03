use std::collections::{BTreeMap, BTreeSet};

/// 将查询规范为小写、单空格分隔的字母数字文本。
#[must_use]
pub fn normalize_text(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut needs_separator = false;
    for character in input.chars().flat_map(char::to_lowercase) {
        if character.is_alphanumeric() {
            if needs_separator && !output.is_empty() {
                output.push(' ');
            }
            output.push(character);
            needs_separator = false;
        } else if !output.is_empty() {
            needs_separator = true;
        }
    }
    output
}

/// 返回稳定去重的空白词项。
#[must_use]
pub fn lexical_terms(input: &str) -> Vec<String> {
    let mut seen = BTreeSet::new();
    normalize_text(input)
        .split_whitespace()
        .filter_map(|term| {
            let term = term.to_owned();
            seen.insert(term.clone()).then_some(term)
        })
        .collect()
}

/// 从字母数字字符生成按出现顺序稳定去重的三字符特征。
#[must_use]
pub fn character_trigrams(input: &str, limit: usize) -> Vec<String> {
    if limit == 0 {
        return Vec::new();
    }
    let compact = normalize_text(input)
        .chars()
        .filter(|character| character.is_alphanumeric())
        .collect::<Vec<_>>();
    if compact.len() < 3 {
        return Vec::new();
    }
    let mut seen = BTreeSet::new();
    compact
        .windows(3)
        .map(|window| window.iter().collect::<String>())
        .filter(|feature| seen.insert(feature.clone()))
        .take(limit)
        .collect()
}

/// 仅为包含非 ASCII 字母数字的业务别名生成字符 trigram。
///
/// 纯 ASCII 别名继续由 unicode61 词法检索处理，避免短 trigram 扩大误召回集合。
#[must_use]
pub fn alias_character_trigrams(
    aliases: &[String],
    per_alias_limit: usize,
    total_limit: usize,
) -> Vec<String> {
    aliases
        .iter()
        .filter(|alias| {
            alias
                .chars()
                .any(|character| character.is_alphanumeric() && !character.is_ascii())
        })
        .flat_map(|alias| character_trigrams(alias, per_alias_limit))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .take(total_limit)
        .collect()
}

/// 计算两个文本的确定性字符 trigram 余弦相似度。
///
/// 该分数只表达共享字符特征，不推断语义，也不改变任何权威记录。
#[must_use]
pub fn trigram_similarity(left: &str, right: &str) -> f32 {
    let left = trigram_counts(left);
    let right = trigram_counts(right);
    let dot = left
        .iter()
        .filter_map(|(feature, left_count)| {
            right
                .get(feature)
                .map(|right_count| left_count * right_count)
        })
        .sum::<f32>();
    let left_norm = left.values().map(|value| value * value).sum::<f32>().sqrt();
    let right_norm = right
        .values()
        .map(|value| value * value)
        .sum::<f32>()
        .sqrt();
    if left_norm == 0.0 || right_norm == 0.0 {
        0.0
    } else {
        dot / (left_norm * right_norm)
    }
}

fn trigram_counts(input: &str) -> BTreeMap<String, f32> {
    let compact = normalize_text(input)
        .chars()
        .filter(|character| character.is_alphanumeric())
        .collect::<Vec<_>>();
    if compact.len() < 3 {
        return BTreeMap::new();
    }
    let mut counts = BTreeMap::new();
    for window in compact.windows(3) {
        let feature = window.iter().collect::<String>();
        *counts.entry(feature).or_insert(0.0) += 1.0;
    }
    counts
}
