//! Tool 审批记录。

use std::{
    collections::BTreeMap,
    sync::{Arc, RwLock},
};

use jc_memory_protocol::MemoryScope;
use semver::VersionReq;
use uuid::Uuid;

/// 一条审批请求的稳定标识。
pub type ApprovalId = Uuid;

/// 审批结果。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApprovalDecision {
    /// 允许执行。
    Approved,
    /// 拒绝执行。
    Rejected,
}

/// 持久化边界使用的审批状态。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApprovalStatus {
    /// 等待审批。
    Pending,
    /// 已批准。
    Approved,
    /// 已拒绝。
    Rejected,
}

/// 一条待审批的 Tool 请求。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApprovalRequest {
    /// 审批标识。
    pub id: ApprovalId,
    /// 请求的 Tool 标识。
    pub tool_id: String,
    /// 请求的版本范围。
    pub version: VersionReq,
    /// 请求所在的精确 Scope。
    pub scope: MemoryScope,
    /// 发起审批的主体。
    pub principal_id: String,
    /// 当前审批状态。
    pub status: ApprovalStatus,
}

/// 线程安全的审批存储。
#[derive(Clone, Default)]
pub struct ApprovalStore {
    entries: Arc<RwLock<BTreeMap<ApprovalId, ApprovalRequest>>>,
}

impl ApprovalStore {
    /// 创建空审批存储。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 追加一条待审批请求。
    #[must_use]
    pub fn request(
        &self,
        tool_id: String,
        version: VersionReq,
        scope: MemoryScope,
        principal_id: String,
    ) -> ApprovalRequest {
        let request = ApprovalRequest {
            id: Uuid::now_v7(),
            tool_id,
            version,
            scope,
            principal_id,
            status: ApprovalStatus::Pending,
        };
        if let Ok(mut entries) = self.entries.write() {
            entries.insert(request.id, request.clone());
        }
        request
    }

    /// 更新审批状态。
    ///
    /// # Errors
    ///
    /// 审批标识不存在或存储锁不可用时返回错误。
    pub fn decide(&self, id: ApprovalId, decision: ApprovalDecision) -> Result<(), ApprovalId> {
        let Ok(mut entries) = self.entries.write() else {
            return Err(id);
        };
        let Some(request) = entries.get_mut(&id) else {
            return Err(id);
        };
        request.status = match decision {
            ApprovalDecision::Approved => ApprovalStatus::Approved,
            ApprovalDecision::Rejected => ApprovalStatus::Rejected,
        };
        Ok(())
    }

    /// 找到同一主体、Scope、Tool 和版本范围下最新的一条审批。
    #[must_use]
    pub fn find(
        &self,
        tool_id: &str,
        version: &VersionReq,
        scope: &MemoryScope,
        principal_id: &str,
    ) -> Option<ApprovalRequest> {
        self.entries
            .read()
            .ok()?
            .values()
            .filter(|request| {
                request.tool_id == tool_id
                    && request.version == *version
                    && request.scope == *scope
                    && request.principal_id == principal_id
            })
            .max_by_key(|request| request.id)
            .cloned()
    }

    /// 读取审批记录。
    #[must_use]
    pub fn get(&self, id: ApprovalId) -> Option<ApprovalRequest> {
        self.entries.read().ok()?.get(&id).cloned()
    }
}
