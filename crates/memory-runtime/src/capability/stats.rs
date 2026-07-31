//! Capability 的可审计运行统计。

use std::{collections::BTreeMap, sync::RwLock};

use jc_memory_protocol::EvidenceId;
use semver::Version;

/// 一个 Capability 版本的运行统计。
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CapabilityStats {
    /// 总运行次数。
    pub runs: u64,
    /// 成功次数。
    pub successes: u64,
    /// 失败次数。
    pub failures: u64,
    /// 按稳定错误模式统计的失败次数。
    pub failure_modes: BTreeMap<String, u64>,
    /// 支撑统计的权威 Evidence 引用。
    pub evidence: std::collections::BTreeSet<EvidenceId>,
}

/// 用于排序的 Capability 统计结果。
#[derive(Clone, Debug, PartialEq)]
pub struct CapabilityRanking {
    /// Capability 标识。
    pub id: String,
    /// Capability 版本。
    pub version: Version,
    /// 达到样本门槛的统计。
    pub stats: CapabilityStats,
    /// 使用 Beta(1,1) 先验平滑后的成功率。
    pub smoothed_success_rate: f64,
}

/// 只保存运行统计和 Evidence 引用的内存存储。
pub struct CapabilityStatsStore {
    stats: RwLock<BTreeMap<(String, Version), CapabilityStats>>,
}

impl CapabilityStatsStore {
    /// 创建空统计存储。
    #[must_use]
    pub fn new() -> Self {
        Self {
            stats: RwLock::new(BTreeMap::new()),
        }
    }

    /// 追加一次结果，不会创建或改写任何事实记录。
    pub fn record<I>(
        &self,
        id: &str,
        version: &Version,
        success: bool,
        failure_mode: Option<&str>,
        evidence: I,
    ) where
        I: IntoIterator<Item = EvidenceId>,
    {
        let Ok(mut stats) = self.stats.write() else {
            return;
        };
        let item = stats.entry((id.to_owned(), version.clone())).or_default();
        item.runs = item.runs.saturating_add(1);
        if success {
            item.successes = item.successes.saturating_add(1);
        } else {
            item.failures = item.failures.saturating_add(1);
            if let Some(mode) = failure_mode.filter(|mode| !mode.trim().is_empty()) {
                let count = item.failure_modes.entry(mode.to_owned()).or_default();
                *count = count.saturating_add(1);
            }
        }
        item.evidence.extend(evidence);
    }

    /// 读取一个版本的当前统计。
    #[must_use]
    pub fn get(&self, id: &str, version: &Version) -> Option<CapabilityStats> {
        self.stats
            .read()
            .ok()
            .and_then(|stats| stats.get(&(id.to_owned(), version.clone())).cloned())
    }

    /// 按平滑成功率降序返回达到最小样本数的版本。
    #[must_use]
    pub fn rank(&self, id: &str, minimum_samples: u64) -> Vec<CapabilityRanking> {
        let Ok(stats) = self.stats.read() else {
            return Vec::new();
        };
        let mut ranked: Vec<_> = stats
            .iter()
            .filter(|((candidate_id, _), item)| candidate_id == id && item.runs >= minimum_samples)
            .map(|((candidate_id, version), item)| CapabilityRanking {
                id: candidate_id.clone(),
                version: version.clone(),
                stats: item.clone(),
                smoothed_success_rate: (bounded_f64(item.successes) + 1.0)
                    / (bounded_f64(item.runs) + 2.0),
            })
            .collect();
        ranked.sort_by(|left, right| {
            right
                .smoothed_success_rate
                .total_cmp(&left.smoothed_success_rate)
                .then_with(|| right.version.cmp(&left.version))
        });
        ranked
    }
}

fn bounded_f64(value: u64) -> f64 {
    f64::from(u32::try_from(value).unwrap_or(u32::MAX))
}

impl Default for CapabilityStatsStore {
    fn default() -> Self {
        Self::new()
    }
}
