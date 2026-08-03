use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::CandidateSource;

/// 一次混合检索调用的脱敏聚合解释。
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct RetrievalTrace {
    /// 本次查询使用的业务词典版本。
    pub dictionary_version: String,
    /// 本次查询使用的原始词典 SHA-256。
    pub dictionary_sha256: String,
    /// 各固定来源在融合前产生的候选数量。
    pub source_candidate_counts: BTreeMap<CandidateSource, usize>,
    /// RRF 去重并应用内部候选预算后的数量。
    pub fused_candidate_count: usize,
    /// 近期候选因未达到确定性匹配阈值而丢弃的数量。
    pub recent_dropped_count: usize,
    /// 最终种子候选包含的来源聚合数量。
    pub final_seed_source_counts: BTreeMap<CandidateSource, usize>,
}
