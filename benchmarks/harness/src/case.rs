//! Benchmark 输入、预期答案和单 Case 结果。

use std::{collections::BTreeSet, time::Duration};

use jc_memory_activation::RecallQuery;
use jc_memory_protocol::{CognitiveFragment, EvidenceId, MemoryPacket, PointId, StateId};

/// 一个可重复执行的 Benchmark Case。
#[derive(Clone, Debug)]
pub struct BenchmarkCase {
    /// 稳定的原始 Case 标识。
    pub id: String,
    /// 写入或准备阶段使用的权威 Fragment。
    pub setup: Vec<CognitiveFragment>,
    /// 发送给 Recall 引擎的查询。
    pub query: RecallQuery,
    /// 与模型措辞无关的结构化预期答案。
    pub expected: ExpectedAnswer,
    /// 用于筛选 Benchmark 子集的标签。
    pub tags: BTreeSet<String>,
}

/// 一个 Case 的结构化答案预期。
#[derive(Clone, Debug, Default)]
pub struct ExpectedAnswer {
    /// 预期召回的 Point 标识。
    pub point_ids: BTreeSet<PointId>,
    /// 预期返回的 Evidence 标识。
    pub evidence_ids: BTreeSet<EvidenceId>,
    /// 预期的当前状态标识。
    pub current_state_ids: BTreeSet<StateId>,
    /// 预期的历史状态标识。
    pub historical_state_ids: BTreeSet<StateId>,
    /// 预期时间路径中的 Point 标识。
    pub temporal_point_ids: BTreeSet<PointId>,
    /// 预期因果路径中的 Point 标识。
    pub causal_point_ids: BTreeSet<PointId>,
    /// 是否预期检测到冲突。
    pub conflict_detected: Option<bool>,
    /// 是否预期拒答；仅比较结构化 Packet 信号。
    pub abstention_expected: Option<bool>,
    /// 答案整体是否正确，供当前/历史状态等 Accuracy 使用。
    pub answer_correct: Option<bool>,
}

/// 一个 Case 的实际执行结果和资源计量。
#[derive(Clone, Debug, PartialEq)]
pub struct CaseResult {
    /// 对应的 Benchmark Case 标识。
    pub case_id: String,
    /// Recall 产生的模型无关 Packet。
    pub packet: MemoryPacket,
    /// 从执行器开始到 Packet 完成的墙钟延迟。
    pub latency: Duration,
    /// 执行后数据库和 Blob 的总字节数。
    pub database_bytes: u64,
    /// 进程峰值常驻内存字节数。
    pub process_peak_bytes: u64,
    /// 本次写入的 Fragment 数量。
    pub written_fragments: u64,
    /// 写入阶段耗时。
    pub write_duration: Option<Duration>,
    /// 巩固阶段耗时。
    pub consolidation_duration: Option<Duration>,
    /// 两个确定性模型的结构化输出是否一致。
    pub model_swap_consistent: Option<bool>,
}
