//! `OpenClaw` Markdown 导入流程。

use jc_memory_protocol::FragmentId;

/// 单个 Markdown 区段的导入结果。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpenClawImportEntry {
    /// Core 中的权威 Fragment 标识。
    pub fragment_id: FragmentId,
    /// 本次导入是否创建了新的权威 Event。
    pub created: bool,
    /// Markdown heading；文件前言为空。
    pub heading: Option<String>,
    /// 1-based 起始行。
    pub line_start: usize,
    /// 1-based 结束行。
    pub line_end: usize,
}

/// 一次 Memory.md 导入结果。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpenClawImportReceipt {
    /// 完整文件内容的 SHA-256。
    pub document_sha256: String,
    /// 按文件顺序返回的区段映射。
    pub entries: Vec<OpenClawImportEntry>,
}
