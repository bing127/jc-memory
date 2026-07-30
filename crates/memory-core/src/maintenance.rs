use std::path::Path;

use jc_memory_protocol::{AccessContext, Operation};
use jc_memory_storage::{
    DerivedRebuildReport, RecoveryError, RestoreReceipt, SnapshotManager, SnapshotReceipt,
};

use crate::{MemoryConfig, MemoryEngine, MemoryError};

impl MemoryEngine {
    /// 为调用者 Scope 创建经过校验并原子发布的 Snapshot。
    ///
    /// # Errors
    ///
    /// 调用者缺少 Export 权限，或一致备份、Blob 校验和目录发布失败时返回错误。
    pub async fn create_snapshot(
        &self,
        context: &AccessContext,
        snapshot_root: impl AsRef<Path>,
        name: &str,
    ) -> Result<SnapshotReceipt, MemoryError> {
        crate::security::authorize(context, Operation::Export, &context.scope)?;
        Ok(self
            .recovery
            .create_snapshot(snapshot_root, name, context.scope.clone())
            .await?)
    }

    /// 在隔离目录完整校验 Snapshot 后原子切换目标数据目录。
    ///
    /// 调用前必须关闭目标目录上的所有 `MemoryEngine` 与 `SQLite` 连接。
    ///
    /// # Errors
    ///
    /// 调用者缺少 Admin 权限，或 Manifest、Scope、Schema、SQLite、Blob、目录切换校验失败时
    /// 返回错误。切换前失败不会改动当前数据目录。
    pub async fn restore_snapshot(
        context: &AccessContext,
        config: MemoryConfig,
        snapshot_dir: impl AsRef<Path>,
    ) -> Result<RestoreReceipt, MemoryError> {
        crate::security::authorize(context, Operation::Admin, &context.scope)?;
        Ok(
            SnapshotManager::restore(snapshot_dir, config.data_dir(), &context.scope)
                .await
                .map_err(RecoveryError::from)?,
        )
    }

    /// 按 FTS、Embedding、Edge Candidate、Surface Member、Summary/Cache 顺序重建派生层。
    ///
    /// # Errors
    ///
    /// 调用者缺少 Admin 权限，或阶段执行与 Checkpoint 持久化失败时返回错误。
    pub async fn rebuild_derived(
        &self,
        context: &AccessContext,
    ) -> Result<DerivedRebuildReport, MemoryError> {
        crate::security::authorize(context, Operation::Admin, &context.scope)?;
        Ok(self.recovery.rebuild_derived(&context.scope).await?)
    }
}
