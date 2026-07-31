//! Cross-Year 确定性数据生成器。

use chrono::{Duration, TimeZone, Utc};
use serde::Serialize;
use sha2::{Digest, Sha256};
use thiserror::Error;

/// Cross-Year 生成摘要。
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Manifest {
    /// 生成器 Seed。
    pub seed: u64,
    /// Case 总数。
    pub case_count: usize,
    /// 生成数据的 SHA-256。
    pub digest: String,
}

/// 一个跨时间的确定性 Case。
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Case {
    /// 稳定 Case ID。
    pub id: String,
    /// 固定发生时间。
    pub occurred_at: String,
    /// 确定性文本内容。
    pub text: String,
    /// 状态键。
    pub state_key: String,
    /// 状态值。
    pub state_value: String,
    /// 时间、实体和困难样本标签。
    pub tags: Vec<String>,
}

/// 生成结果。
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Dataset {
    /// 生成摘要。
    pub manifest: Manifest,
    /// 生成的 Case。
    pub cases: Vec<Case>,
}

/// 生成失败。
#[derive(Debug, Error)]
pub enum Error {
    /// Case 数量必须大于零。
    #[error("case_count 必须大于 0")]
    Empty,
    /// Canonical JSON 序列化失败。
    #[error("生成数据无法序列化: {0}")]
    Serialize(#[from] serde_json::Error),
    /// 固定起始日期无效。
    #[error("固定起始日期无效")]
    InvalidDate,
}

impl Dataset {
    /// 返回不包含 Manifest 自引用字段的 canonical Digest。
    #[must_use]
    pub fn digest(&self) -> String {
        digest(self.manifest.seed, &self.cases).unwrap_or_default()
    }
}

/// 使用固定算法生成跨日、跨月和跨年的数据。
///
/// # Errors
///
/// 当 Case 数量为空、固定日期无效或生成数据无法序列化时返回错误。
pub fn generate(seed: u64, case_count: usize) -> Result<Dataset, Error> {
    if case_count == 0 {
        return Err(Error::Empty);
    }
    let mut state = seed ^ 0x9e37_79b9_7f4a_7c15;
    let start = Utc
        .with_ymd_and_hms(2020, 1, 1, 0, 0, 0)
        .single()
        .ok_or(Error::InvalidDate)?;
    let tag_sets = [
        vec!["one_day"],
        vec!["one_month"],
        vec!["one_year"],
        vec!["state_update"],
        vec!["ambiguous", "same_name"],
        vec!["similar_event"],
        vec!["rare_fact"],
        vec!["state_update", "one_year"],
    ];
    let mut cases = Vec::with_capacity(case_count);
    for index in 0..case_count {
        state = next(&mut state);
        let day_offset = match index % 3 {
            0 => 1 + (state % 6),
            1 => 31 + (state % 8),
            _ => 365 + (state % 32),
        };
        let day_offset = i64::try_from(day_offset).map_err(|_| Error::InvalidDate)?;
        let tags = tag_sets[index % tag_sets.len()]
            .iter()
            .map(|tag| (*tag).to_owned())
            .collect();
        cases.push(Case {
            id: format!("cross-year-{index:04}"),
            occurred_at: (start + Duration::days(day_offset)).to_rfc3339(),
            text: format!("确定性事件 {}", state % 10_000),
            state_key: format!("service-{}", index % 4),
            state_value: format!("value-{}", state % 16),
            tags,
        });
    }
    let digest = digest(seed, &cases)?;
    Ok(Dataset {
        manifest: Manifest {
            seed,
            case_count,
            digest,
        },
        cases,
    })
}

fn next(state: &mut u64) -> u64 {
    *state = state
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    *state
}

fn digest(seed: u64, cases: &[Case]) -> Result<String, serde_json::Error> {
    let canonical = serde_json::to_vec(&(seed, cases))?;
    Ok(hex::encode(Sha256::digest(canonical)))
}
