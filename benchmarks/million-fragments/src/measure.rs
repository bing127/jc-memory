//! 压力测试报告和测量字段。

use std::time::Duration;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// 可恢复压力测试报告。
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct StressReport {
    /// 报告 Schema。
    pub schema_version: String,
    /// 请求生成的 Fragment 数量。
    pub requested_fragments: u64,
    /// 实际生成并提交的 Fragment 数量。
    pub generated_fragments: u64,
    /// 运行配置 Digest。
    pub config_digest: String,
    /// 是否为仓库内 Smoke。
    pub smoke: bool,
    /// 是否已完成预热阶段。
    pub warmup_complete: bool,
    /// 是否已完成测量阶段。
    pub measurement_complete: bool,
    /// 写入吞吐，未测量时为 null。
    pub write_throughput_per_second: Option<f64>,
    /// 索引耗时毫秒，未测量时为 null。
    pub index_time_ms: Option<u64>,
    /// 数据库字节数，未测量时为 null。
    pub database_bytes: Option<u64>,
    /// Blob 字节数，未测量时为 null。
    pub blob_bytes: Option<u64>,
    /// Recall P50/P95/P99，未测量时为 null。
    pub recall_p50_ms: Option<u64>,
    /// Recall P95，单位毫秒。
    pub recall_p95_ms: Option<u64>,
    /// Recall P99，单位毫秒。
    pub recall_p99_ms: Option<u64>,
    /// 局部图加载耗时毫秒，未测量时为 null。
    pub graph_load_ms: Option<u64>,
    /// 巩固耗时毫秒，未测量时为 null。
    pub consolidation_ms: Option<u64>,
    /// RSS 峰值字节，未测量时为 null。
    pub rss_peak_bytes: Option<u64>,
    /// 崩溃恢复耗时毫秒，未测量时为 null。
    pub crash_recovery_ms: Option<u64>,
}

impl StressReport {
    /// 创建一个只声明计数和阶段状态的 Smoke 报告。
    #[must_use]
    pub fn smoke(fragments: u64, config_digest: String) -> Self {
        Self {
            schema_version: "jcmem.million-fragments.v1".to_owned(),
            requested_fragments: fragments,
            generated_fragments: fragments,
            config_digest,
            smoke: true,
            warmup_complete: true,
            measurement_complete: true,
            write_throughput_per_second: None,
            index_time_ms: None,
            database_bytes: None,
            blob_bytes: None,
            recall_p50_ms: None,
            recall_p95_ms: None,
            recall_p99_ms: None,
            graph_load_ms: None,
            consolidation_ms: None,
            rss_peak_bytes: None,
            crash_recovery_ms: None,
        }
    }

    /// 将实际写入吞吐附加到报告；其他未采集指标保持为空。
    #[must_use]
    pub fn with_write_throughput(mut self, throughput: Option<f64>) -> Self {
        self.write_throughput_per_second = throughput;
        self
    }

    /// 校验报告的原始计数和阶段边界。
    ///
    /// # Errors
    ///
    /// 当 Schema、原始计数、阶段标志或配置 Digest 不满足要求时返回错误。
    pub fn validate(&self) -> Result<(), ReportError> {
        if self.schema_version != "jcmem.million-fragments.v1" {
            return Err(ReportError::InvalidSchema);
        }
        if self.requested_fragments != self.generated_fragments {
            return Err(ReportError::RawCountMismatch {
                requested: self.requested_fragments,
                generated: self.generated_fragments,
            });
        }
        if !self.warmup_complete {
            return Err(ReportError::MissingWarmup);
        }
        if !self.measurement_complete {
            return Err(ReportError::MissingMeasurement);
        }
        if self.config_digest.is_empty() {
            return Err(ReportError::MissingConfigDigest);
        }
        Ok(())
    }
}

/// 根据已提交 Fragment 数量和实际耗时计算写入吞吐。
#[must_use]
// 吞吐报告的公开字段是 f64；百万级计数远低于其常用精确范围。
#[allow(clippy::cast_precision_loss)]
pub fn throughput_per_second(committed_fragments: u64, elapsed: Duration) -> Option<f64> {
    if committed_fragments == 0 || elapsed.is_zero() {
        return None;
    }
    Some(committed_fragments as f64 / elapsed.as_secs_f64())
}

/// 报告验证错误。
#[derive(Debug, Error)]
pub enum ReportError {
    /// Schema 不匹配。
    #[error("报告 Schema 不匹配")]
    InvalidSchema,
    /// 请求数和实际数不一致。
    #[error("报告原始计数不一致: requested {requested}, generated {generated}")]
    RawCountMismatch {
        /// 报告声明的请求数量。
        requested: u64,
        /// 报告声明的实际数量。
        generated: u64,
    },
    /// 缺失预热阶段。
    #[error("报告缺少预热阶段")]
    MissingWarmup,
    /// 缺失测量阶段。
    #[error("报告缺少测量阶段")]
    MissingMeasurement,
    /// 缺失配置 Digest。
    #[error("报告缺少 config digest")]
    MissingConfigDigest,
}
