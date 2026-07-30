//! 不复制正文或密钥的治理审计记录。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// 治理操作的稳定审计结果。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditResult {
    /// 操作已执行。
    Succeeded,
    /// 操作被授权边界拒绝。
    Denied,
    /// 操作在授权后失败。
    Failed,
}

impl AuditResult {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Succeeded => "succeeded",
            Self::Denied => "denied",
            Self::Failed => "failed",
        }
    }
}

/// 不含目标正文和密钥材料的治理审计记录。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AuditRecord {
    /// 发起操作的主体。
    pub principal_id: String,
    /// 稳定操作名。
    pub operation: String,
    /// 目标标识的 SHA-256 摘要。
    pub target_digest: String,
    /// 调用链请求标识。
    pub request_id: String,
    /// 操作结果。
    pub result: AuditResult,
    /// 审计创建时间。
    pub created_at: DateTime<Utc>,
}
