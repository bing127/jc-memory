//! 受 Core 授权保护的持久化 Consolidation Job 门面。

use std::{sync::Arc, time::Duration};

use jc_memory_consolidation::{
    ColdTierMovement, ConflictDetection, ConsolidationTask, Deduplication, EdgeReinforcement,
    EdgeWeakening, JobId, JobKind, JobRecord, JobRunner, JobSpec, Rebuild, Replay,
    SemanticPromotion, StaleStateDetection, SurfaceDiscovery, SurfaceMerge, SurfaceSplit,
};
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
        self.authorized_consolidation_job(context, id).await
    }

    /// 运行一个已授权 Job，直到完成、暂停、取消或失败。
    ///
    /// # Errors
    ///
    /// 授权、Lease、预算、Handler 或持久化状态失败时返回错误。
    pub async fn run_consolidation(
        &self,
        context: &AccessContext,
        id: JobId,
    ) -> Result<JobRecord, MemoryError> {
        let record = self.authorized_consolidation_job(context, id).await?;
        match record.spec.kind {
            JobKind::Replay => self.run_consolidation_task(id, Replay).await,
            JobKind::Deduplication => self.run_consolidation_task(id, Deduplication).await,
            JobKind::EdgeReinforcement => self.run_consolidation_task(id, EdgeReinforcement).await,
            JobKind::EdgeWeakening => self.run_consolidation_task(id, EdgeWeakening).await,
            JobKind::SurfaceDiscovery => self.run_consolidation_task(id, SurfaceDiscovery).await,
            JobKind::SurfaceMerge => self.run_consolidation_task(id, SurfaceMerge).await,
            JobKind::SurfaceSplit => self.run_consolidation_task(id, SurfaceSplit).await,
            JobKind::SemanticPromotion => self.run_consolidation_task(id, SemanticPromotion).await,
            JobKind::ConflictDetection => self.run_consolidation_task(id, ConflictDetection).await,
            JobKind::StaleStateDetection => {
                self.run_consolidation_task(id, StaleStateDetection).await
            }
            JobKind::ColdTierMovement => self.run_consolidation_task(id, ColdTierMovement).await,
            JobKind::Rebuild => self.run_consolidation_task(id, Rebuild).await,
        }
    }

    /// 暂停一个已授权且尚未结束的 Job。
    ///
    /// # Errors
    ///
    /// 授权、状态转换或持久化失败时返回错误。
    pub async fn pause_consolidation(
        &self,
        context: &AccessContext,
        id: JobId,
    ) -> Result<JobRecord, MemoryError> {
        self.authorized_consolidation_job(context, id).await?;
        Ok(self.consolidation_jobs.pause(id).await?)
    }

    /// 把已授权的 Paused Job 恢复为 Pending。
    ///
    /// # Errors
    ///
    /// 授权、状态转换或持久化失败时返回错误。
    pub async fn resume_consolidation(
        &self,
        context: &AccessContext,
        id: JobId,
    ) -> Result<JobRecord, MemoryError> {
        self.authorized_consolidation_job(context, id).await?;
        Ok(self.consolidation_jobs.resume(id).await?)
    }

    /// 显式重试一个已授权且仍可重试的 Job。
    ///
    /// # Errors
    ///
    /// 授权、状态转换、重试耗尽或持久化失败时返回错误。
    pub async fn retry_consolidation(
        &self,
        context: &AccessContext,
        id: JobId,
    ) -> Result<JobRecord, MemoryError> {
        self.authorized_consolidation_job(context, id).await?;
        Ok(self.consolidation_jobs.retry(id).await?)
    }

    /// 取消一个已授权且尚未结束的 Job。
    ///
    /// # Errors
    ///
    /// 授权、状态转换或持久化失败时返回错误。
    pub async fn cancel_consolidation(
        &self,
        context: &AccessContext,
        id: JobId,
    ) -> Result<JobRecord, MemoryError> {
        self.authorized_consolidation_job(context, id).await?;
        Ok(self.consolidation_jobs.cancel(id).await?)
    }

    async fn authorized_consolidation_job(
        &self,
        context: &AccessContext,
        id: JobId,
    ) -> Result<JobRecord, MemoryError> {
        let record = self.consolidation_jobs.get(id).await?;
        crate::security::authorize(context, Operation::Consolidate, &record.spec.scope)?;
        Ok(record)
    }

    async fn run_consolidation_task<Task>(
        &self,
        id: JobId,
        task: Task,
    ) -> Result<JobRecord, MemoryError>
    where
        Task: ConsolidationTask + 'static,
    {
        Ok(JobRunner::new(
            self.consolidation_jobs.clone(),
            Arc::new(task),
            self.consolidation_worker_id.clone(),
            Duration::from_secs(30),
            Duration::from_millis(10),
        )
        .run(id)
        .await?)
    }
}
