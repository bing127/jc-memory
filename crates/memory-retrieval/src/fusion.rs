use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};
use jc_memory_protocol::{EventId, FragmentId};
use serde::{Deserialize, Serialize};

/// 产生候选的固定本地检索通道。
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateSource {
    /// `SQLite` FTS5 unicode61 字面查询。
    Lexical,
    /// `SQLite` FTS5 trigram 字符查询。
    Trigram,
    /// 版本化业务别名扩展查询。
    Alias,
    /// Scope 内显式实体或实体别名查询。
    Entity,
    /// 只作为低优先级安全网的近期候选。
    Recent,
}

/// 一个来源内有稳定名次的候选。
#[derive(Clone, Debug, PartialEq)]
pub struct RetrievalCandidate {
    /// 候选权威 Fragment。
    pub fragment_id: FragmentId,
    /// 承载 Fragment 的权威 Event。
    pub event_id: EventId,
    /// 产生候选的检索通道。
    pub source: CandidateSource,
    /// 从 1 开始的来源内名次。
    pub source_rank: u32,
    /// 来源自己的有限相关性分数。
    pub source_score: f64,
    /// 权威 Fragment 的业务发生时间。
    pub occurred_at: DateTime<Utc>,
}

/// RRF 合并后的稳定候选。
#[derive(Clone, Debug, PartialEq)]
pub struct FusedCandidate {
    /// 候选权威 Fragment。
    pub fragment_id: FragmentId,
    /// 选定的权威 Event。
    pub event_id: EventId,
    /// 参与合并的去重来源。
    pub sources: BTreeSet<CandidateSource>,
    /// Reciprocal Rank Fusion 总分。
    pub rrf_score: f64,
    /// 各来源中的最佳原始分数。
    pub best_source_score: f64,
    /// 权威 Fragment 的业务发生时间。
    pub occurred_at: DateTime<Utc>,
}

/// 以固定 `k` 合并多个有序候选列表。
///
/// # Errors
///
/// `k`、limit、来源名次或来源分数无效时返回错误。
pub fn fuse_rrf(
    lists: &[Vec<RetrievalCandidate>],
    k: u32,
    limit: usize,
) -> Result<Vec<FusedCandidate>, FusionError> {
    if k == 0 {
        return Err(FusionError::InvalidK);
    }
    if limit == 0 {
        return Err(FusionError::InvalidLimit);
    }
    let mut per_source = BTreeMap::<(FragmentId, CandidateSource), RetrievalCandidate>::new();
    for candidate in lists.iter().flatten() {
        validate(candidate)?;
        let key = (candidate.fragment_id, candidate.source);
        match per_source.get(&key) {
            Some(current) if !is_better(candidate, current) => {}
            _ => {
                per_source.insert(key, candidate.clone());
            }
        }
    }

    let mut fused = BTreeMap::<FragmentId, FusedCandidate>::new();
    for candidate in per_source.into_values() {
        let contribution = 1.0 / f64::from(k + candidate.source_rank);
        let entry = fused
            .entry(candidate.fragment_id)
            .or_insert_with(|| FusedCandidate {
                fragment_id: candidate.fragment_id,
                event_id: candidate.event_id,
                sources: BTreeSet::new(),
                rrf_score: 0.0,
                best_source_score: candidate.source_score,
                occurred_at: candidate.occurred_at,
            });
        entry.sources.insert(candidate.source);
        entry.rrf_score += contribution;
        if candidate.source_score > entry.best_source_score
            || (candidate
                .source_score
                .total_cmp(&entry.best_source_score)
                .is_eq()
                && (candidate.occurred_at, candidate.event_id)
                    > (entry.occurred_at, entry.event_id))
        {
            entry.event_id = candidate.event_id;
            entry.best_source_score = candidate.source_score;
            entry.occurred_at = candidate.occurred_at;
        }
    }

    let mut output = fused.into_values().collect::<Vec<_>>();
    output.sort_by(|left, right| {
        right
            .rrf_score
            .total_cmp(&left.rrf_score)
            .then_with(|| right.best_source_score.total_cmp(&left.best_source_score))
            .then_with(|| right.occurred_at.cmp(&left.occurred_at))
            .then_with(|| left.fragment_id.cmp(&right.fragment_id))
    });
    output.truncate(limit);
    Ok(output)
}

fn validate(candidate: &RetrievalCandidate) -> Result<(), FusionError> {
    if candidate.source_rank == 0 {
        return Err(FusionError::InvalidRank);
    }
    if !candidate.source_score.is_finite() {
        return Err(FusionError::InvalidScore);
    }
    Ok(())
}

fn is_better(candidate: &RetrievalCandidate, current: &RetrievalCandidate) -> bool {
    candidate.source_rank < current.source_rank
        || (candidate.source_rank == current.source_rank
            && (
                candidate.source_score,
                candidate.occurred_at,
                candidate.event_id,
            ) > (current.source_score, current.occurred_at, current.event_id))
}

/// 候选融合输入错误。
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum FusionError {
    /// RRF `k` 必须大于零。
    #[error("RRF k must be greater than zero")]
    InvalidK,
    /// 输出上限必须大于零。
    #[error("RRF output limit must be greater than zero")]
    InvalidLimit,
    /// 来源名次必须从 1 开始。
    #[error("candidate source rank must start at one")]
    InvalidRank,
    /// 来源分数必须是有限值。
    #[error("candidate source score must be finite")]
    InvalidScore,
}
