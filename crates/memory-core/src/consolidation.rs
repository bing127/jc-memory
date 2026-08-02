//! 受 Core 授权保护的持久化 Consolidation Job 门面。

use jc_memory_consolidation::{JobId, JobRecord, JobSpec};
use jc_memory_protocol::{AccessContext, Operation};

use crate::{MemoryEngine, MemoryError};

impl MemoryEngine {
    /// 在精确 Scope 授权后提交一个持久化 Consolidation Job。
    ///
    /// # Errors
    ///
    /// 调用者缺少 `Consolidate` Grant、规格无效或 `SQLite` 写入失败时返回错误。
    pub async fn submit_consolidation(
        &self,
        context: &AccessContext,
        spec: JobSpec,
    ) -> Result<JobRecord, MemoryError> {
        crate::security::authorize(context, Operation::Consolidate, &spec.scope)?;
        Ok(self.consolidation_jobs.submit(spec).await?)
    }

    /// 按 Job 自身持久化 Scope 授权并读取当前状态。
    ///
    /// # Errors
    ///
    /// Job 不存在、记录损坏或调用者无权访问其 Scope 时返回错误。
    pub async fn consolidation_job(
        &self,
        context: &AccessContext,
        id: JobId,
    ) -> Result<JobRecord, MemoryError> {
        let record = self.consolidation_jobs.get(id).await?;
        crate::security::authorize(context, Operation::Consolidate, &record.spec.scope)?;
        Ok(record)
    }
}
