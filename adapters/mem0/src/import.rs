//! Mem0 历史记录迁移。

use chrono::{DateTime, Utc};
use jc_memory_protocol::FragmentId;
use serde::{Deserialize, Serialize};

/// ID mapping manifest 的稳定版本。
pub const MANIFEST_VERSION: &str = "jc-memory.mem0-id-map.v1";

/// 单条 Mem0 外部 ID 到权威 Fragment ID 的映射。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Mem0IdMapping {
    /// 原 Mem0 记录 ID。
    pub mem0_id: String,
    /// 迁移后的权威 Fragment ID。
    pub fragment_id: FragmentId,
    /// 原 memory 文本的 SHA-256。
    pub content_sha256: String,
    /// 原记录创建时间。
    pub created_at: DateTime<Utc>,
    /// 原记录更新时间，用于区分历史版本。
    pub updated_at: DateTime<Utc>,
    /// 本次导入是否创建了新的权威 Event。
    pub created: bool,
}

/// 可独立保存的 Mem0 ID mapping manifest。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Mem0IdMappingManifest {
    /// Manifest 协议版本。
    pub manifest_version: String,
    /// 按输入顺序保存的 ID 映射。
    pub entries: Vec<Mem0IdMapping>,
}

/// 一次 Add 或历史导入的结果。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Mem0ImportReceipt {
    /// 需要随迁移结果一起保存的 ID mapping manifest。
    pub manifest: Mem0IdMappingManifest,
}
