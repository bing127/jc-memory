//! 排他治理删除入口。

use std::{fs::OpenOptions, path::Path};

use chrono::Utc;
use fs2::FileExt as _;
use jc_memory_protocol::{AccessContext, Decision, DenyReason, FragmentId, Operation};
use jc_memory_storage::{BlobStore, SqliteStore};
use sha2::{Digest as _, Sha256};
use sqlx::{Row as _, Sqlite, SqlitePool, Transaction};

use crate::{AclEngine, AuditResult};

/// Hard Delete 完成后留下的无正文凭证。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HardDeleteReceipt {
    /// 被删除目标标识的 SHA-256 摘要。
    pub target_digest: String,
    /// 与审计关联的请求标识。
    pub request_id: String,
    /// 是否已排队重新巩固。
    pub reconsolidation_queued: bool,
}

/// 治理操作失败。
#[derive(Debug, thiserror::Error)]
pub enum GovernanceError {
    /// 调用者没有目标 Scope 的显式权限。
    #[error("governance access denied: {reason:?}")]
    AccessDenied {
        /// ACL 的稳定拒绝原因。
        reason: DenyReason,
    },
    /// 授权目标不存在于调用者的精确 Scope。
    #[error("governance target was not found")]
    TargetNotFound,
    /// Retention 计划不存在、已执行、摘要错误或候选被替换。
    #[error("retention plan is invalid or no longer executable")]
    InvalidRetentionPlan,
    /// 另一个进程正在执行排他治理任务。
    #[error("exclusive governance lock is unavailable")]
    LockUnavailable,
    /// 治理文件操作失败。
    #[error("governance filesystem operation failed: {0}")]
    Io(#[from] std::io::Error),
    /// 持久化治理状态失败。
    #[error("governance database operation failed: {0}")]
    Database(#[from] sqlx::Error),
}

/// 在精确 Scope 内执行审计、Retention、Export 与 Hard Delete 的服务。
#[derive(Clone, Debug)]
pub struct GovernanceService {
    pub(crate) pool: SqlitePool,
    pub(crate) blobs: BlobStore,
}

impl GovernanceService {
    /// 复用活动 `SQLite` 与 Blob 仓储。
    #[must_use]
    pub fn from_stores(store: &SqliteStore, blobs: BlobStore) -> Self {
        Self {
            pool: store.pool().clone(),
            blobs,
        }
    }

    /// 执行受 `Delete` Grant 保护的治理删除。
    ///
    /// # Errors
    ///
    /// 没有授权时先写入无正文拒绝审计，再返回 [`GovernanceError::AccessDenied`]。
    // 删除顺序必须与单个排他事务保持在一起，便于审计外键和提交边界。
    #[allow(clippy::too_many_lines)]
    pub async fn hard_delete(
        &self,
        context: &AccessContext,
        target: FragmentId,
    ) -> Result<HardDeleteReceipt, GovernanceError> {
        let target_digest = digest(target.to_string().as_bytes());
        if let Decision::Deny(reason) =
            AclEngine.authorize(context, Operation::Delete, &context.scope)
        {
            self.write_audit(context, "delete", &target_digest, AuditResult::Denied)
                .await?;
            return Err(GovernanceError::AccessDenied { reason });
        }

        let lock = acquire_governance_lock(self.blobs.root())?;
        let scope_key = scope_key(&context.scope)?;
        let mut transaction = self.pool.begin_with("BEGIN EXCLUSIVE").await?;
        let target_row =
            sqlx::query("SELECT content_id FROM fragments WHERE id = ?1 AND scope_key = ?2")
                .bind(target.to_string())
                .bind(&scope_key)
                .fetch_optional(&mut *transaction)
                .await?;
        let Some(target_row) = target_row else {
            transaction.rollback().await?;
            fs2::FileExt::unlock(&lock)?;
            self.write_audit(context, "delete", &target_digest, AuditResult::Failed)
                .await?;
            return Err(GovernanceError::TargetNotFound);
        };
        let content_id: String = target_row.try_get("content_id")?;
        let blob_ids: Vec<String> = sqlx::query_scalar(
            "SELECT DISTINCT blob_id FROM blob_refs
             WHERE owner_type = 'fragment' AND owner_id = ?1 AND ref_count > 0",
        )
        .bind(target.to_string())
        .fetch_all(&mut *transaction)
        .await?;

        clear_scope_derived(&mut transaction, &scope_key).await?;
        clear_authority_references(&mut transaction, target).await?;
        sqlx::query("DELETE FROM blob_refs WHERE owner_type = 'fragment' AND owner_id = ?1")
            .bind(target.to_string())
            .execute(&mut *transaction)
            .await?;
        sqlx::query("DELETE FROM events WHERE fragment_id = ?1")
            .bind(target.to_string())
            .execute(&mut *transaction)
            .await?;
        sqlx::query("DELETE FROM fragments WHERE id = ?1 AND scope_key = ?2")
            .bind(target.to_string())
            .bind(&scope_key)
            .execute(&mut *transaction)
            .await?;
        sqlx::query(
            "DELETE FROM fragment_contents
             WHERE id = ?1 AND NOT EXISTS (
                 SELECT 1 FROM fragments WHERE content_id = fragment_contents.id
             )",
        )
        .bind(content_id)
        .execute(&mut *transaction)
        .await?;

        let now = Utc::now().timestamp_micros();
        let pending_receipt_json = serde_json::json!({
            "operation": "hard_delete",
            "target_digest": target_digest,
            "request_id": context.request_id,
            "reconsolidation_queued": true,
            "cleanup_pending": true,
        });
        sqlx::query(
            "INSERT INTO governance_receipts
             (id, scope_key, operation, target_digest, request_id, result,
              receipt_json, created_at_us)
             VALUES (?1, ?2, 'hard_delete', ?3, ?4, 'failed', ?5, ?6)",
        )
        .bind(uuid::Uuid::now_v7().to_string())
        .bind(&scope_key)
        .bind(&target_digest)
        .bind(&context.request_id)
        .bind(pending_receipt_json.to_string())
        .bind(now)
        .execute(&mut *transaction)
        .await?;
        enqueue_rebuild(&mut transaction, &context.scope, &scope_key, now).await?;
        write_audit_in_transaction(
            &mut transaction,
            context,
            "delete",
            &target_digest,
            AuditResult::Failed,
        )
        .await?;
        let staging_key = digest(context.request_id.as_bytes());
        let mut staged_blobs = Vec::new();
        for blob_id in blob_ids {
            let remaining: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM blob_refs WHERE blob_id = ?1 AND ref_count > 0",
            )
            .bind(&blob_id)
            .fetch_one(&mut *transaction)
            .await?;
            if remaining == 0 {
                match stage_blob_if_present(self.blobs.root(), &blob_id, &staging_key).await {
                    Ok(Some(staged)) => staged_blobs.push(staged),
                    Ok(None) => {}
                    Err(error) => {
                        transaction.rollback().await?;
                        restore_staged_blobs(&staged_blobs).await?;
                        fs2::FileExt::unlock(&lock)?;
                        self.write_audit(context, "delete", &target_digest, AuditResult::Failed)
                            .await?;
                        return Err(error);
                    }
                }
            }
        }
        if let Err(error) = transaction.commit().await {
            restore_staged_blobs(&staged_blobs).await?;
            fs2::FileExt::unlock(&lock)?;
            self.write_audit(context, "delete", &target_digest, AuditResult::Failed)
                .await?;
            return Err(error.into());
        }
        if let Err(error) = remove_staged_blobs(&staged_blobs).await {
            fs2::FileExt::unlock(&lock)?;
            return Err(error.into());
        }
        finalize_delete_success(&self.pool, context, &target_digest).await?;
        fs2::FileExt::unlock(&lock)?;
        Ok(HardDeleteReceipt {
            target_digest,
            request_id: context.request_id.clone(),
            reconsolidation_queued: true,
        })
    }

    pub(crate) async fn write_audit(
        &self,
        context: &AccessContext,
        operation: &str,
        target_digest: &str,
        result: AuditResult,
    ) -> Result<(), GovernanceError> {
        let scope_key = scope_key(&context.scope)?;
        let detail_digest = digest(
            format!(
                "{operation}\0{target_digest}\0{}\0{}",
                context.request_id,
                result.as_str()
            )
            .as_bytes(),
        );
        sqlx::query(
            "INSERT INTO audit_logs
             (id, scope_key, principal_id, operation, target_type, target_id,
              detail_digest, target_digest, request_id, result, created_at_us)
             VALUES (?1, ?2, ?3, ?4, 'fragment_digest', ?5, ?6, ?5, ?7, ?8, ?9)",
        )
        .bind(uuid::Uuid::now_v7().to_string())
        .bind(scope_key)
        .bind(&context.principal_id)
        .bind(operation)
        .bind(target_digest)
        .bind(detail_digest)
        .bind(&context.request_id)
        .bind(result.as_str())
        .bind(Utc::now().timestamp_micros())
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

async fn clear_scope_derived(
    transaction: &mut Transaction<'_, Sqlite>,
    scope_key: &str,
) -> Result<(), GovernanceError> {
    for statement in [
        "DELETE FROM recall_activations WHERE recall_session_id IN
             (SELECT id FROM recall_sessions WHERE scope_key = ?1)",
        "DELETE FROM recall_sessions WHERE scope_key = ?1",
        "DELETE FROM surface_members WHERE surface_id IN
             (SELECT id FROM surfaces WHERE scope_key = ?1)",
        "DELETE FROM surfaces WHERE scope_key = ?1",
        "DELETE FROM edges WHERE scope_key = ?1",
        "DELETE FROM points WHERE scope_key = ?1",
        "DELETE FROM consolidation_artifacts WHERE scope_key = ?1",
        "DELETE FROM cold_tier_entries WHERE scope_key = ?1",
        "DELETE FROM fragment_fts WHERE scope_key = ?1",
    ] {
        sqlx::query(statement)
            .bind(scope_key)
            .execute(&mut **transaction)
            .await?;
    }
    Ok(())
}

async fn clear_authority_references(
    transaction: &mut Transaction<'_, Sqlite>,
    target: FragmentId,
) -> Result<(), GovernanceError> {
    let target = target.to_string();
    sqlx::query(
        "DELETE FROM evidence_refs
         WHERE owner_fragment_id = ?1 OR referenced_fragment_id = ?1
            OR referenced_event_id IN (SELECT id FROM events WHERE fragment_id = ?1)",
    )
    .bind(&target)
    .execute(&mut **transaction)
    .await?;
    for statement in [
        "DELETE FROM quarantine WHERE fragment_id = ?1",
        "DELETE FROM entity_aliases WHERE source_fragment_id = ?1
             OR entity_id IN (SELECT id FROM entities WHERE source_fragment_id = ?1)",
        "DELETE FROM entities WHERE source_fragment_id = ?1",
        "DELETE FROM decisions WHERE candidate_id IN
             (SELECT id FROM hypotheses WHERE source_fragment_id = ?1)",
        "DELETE FROM hypotheses WHERE source_fragment_id = ?1",
        "DELETE FROM outcomes WHERE source_fragment_id = ?1",
    ] {
        sqlx::query(statement)
            .bind(&target)
            .execute(&mut **transaction)
            .await?;
    }
    let state_versions: Vec<String> =
        sqlx::query_scalar("SELECT id FROM state_versions WHERE source_fragment_id = ?1")
            .bind(&target)
            .fetch_all(&mut **transaction)
            .await?;
    for version_id in state_versions {
        sqlx::query("UPDATE states SET current_version_id = NULL WHERE current_version_id = ?1")
            .bind(&version_id)
            .execute(&mut **transaction)
            .await?;
        sqlx::query(
            "UPDATE state_versions SET previous_version_id = NULL WHERE previous_version_id = ?1",
        )
        .bind(&version_id)
        .execute(&mut **transaction)
        .await?;
    }
    sqlx::query("DELETE FROM state_versions WHERE source_fragment_id = ?1")
        .bind(&target)
        .execute(&mut **transaction)
        .await?;
    sqlx::query(
        "DELETE FROM states WHERE NOT EXISTS (
             SELECT 1 FROM state_versions WHERE state_id = states.id
         )",
    )
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn enqueue_rebuild(
    transaction: &mut Transaction<'_, Sqlite>,
    scope: &jc_memory_protocol::MemoryScope,
    scope_key: &str,
    now: i64,
) -> Result<(), GovernanceError> {
    let checkpoint = serde_json::json!({
        "scope": scope,
        "cursor": 0,
        "committed_batches": 0,
        "processed_items": 0,
        "cpu_ms": 0,
        "io_ops": 0,
        "lease": null,
        "next_retry_at_us": null,
    });
    let limits = serde_json::json!({
        "max_batch_size": 128,
        "max_cpu_ms": 60_000,
        "max_io_ops": 100_000,
        "max_runtime_ms": 60_000,
        "max_attempts": 5,
    });
    sqlx::query(
        "INSERT INTO consolidation_jobs
         (id, scope_key, kind, status, checkpoint_json, attempts,
          resource_limits_json, created_at_us, updated_at_us)
         VALUES (?1, ?2, 'rebuild', 'pending', ?3, 0, ?4, ?5, ?5)",
    )
    .bind(uuid::Uuid::now_v7().to_string())
    .bind(scope_key)
    .bind(checkpoint.to_string())
    .bind(limits.to_string())
    .bind(now)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn write_audit_in_transaction(
    transaction: &mut Transaction<'_, Sqlite>,
    context: &AccessContext,
    operation: &str,
    target_digest: &str,
    result: AuditResult,
) -> Result<(), GovernanceError> {
    let detail_digest = digest(
        format!(
            "{operation}\0{target_digest}\0{}\0{}",
            context.request_id,
            result.as_str()
        )
        .as_bytes(),
    );
    sqlx::query(
        "INSERT INTO audit_logs
         (id, scope_key, principal_id, operation, target_type, target_id,
          detail_digest, target_digest, request_id, result, created_at_us)
         VALUES (?1, ?2, ?3, ?4, 'fragment_digest', ?5, ?6, ?5, ?7, ?8, ?9)",
    )
    .bind(uuid::Uuid::now_v7().to_string())
    .bind(scope_key(&context.scope)?)
    .bind(&context.principal_id)
    .bind(operation)
    .bind(target_digest)
    .bind(detail_digest)
    .bind(&context.request_id)
    .bind(result.as_str())
    .bind(Utc::now().timestamp_micros())
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

fn acquire_governance_lock(blob_root: &Path) -> Result<std::fs::File, GovernanceError> {
    let data_dir = blob_root.parent().ok_or(GovernanceError::LockUnavailable)?;
    let lock = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(data_dir.join(".governance.lock"))?;
    lock.try_lock_exclusive()
        .map_err(|_| GovernanceError::LockUnavailable)?;
    Ok(lock)
}

#[derive(Debug)]
struct StagedBlob {
    original: std::path::PathBuf,
    staged: std::path::PathBuf,
}

async fn stage_blob_if_present(
    root: &Path,
    id: &str,
    staging_key: &str,
) -> Result<Option<StagedBlob>, GovernanceError> {
    if id.len() != 64 || !id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Ok(None);
    }
    let original = root
        .join(&id[..2])
        .join(&id[2..4])
        .join(format!("{id}.jcb"));
    let data_dir = root.parent().ok_or(GovernanceError::LockUnavailable)?;
    let staging_dir = data_dir.join(".governance-trash").join(staging_key);
    let staged = staging_dir.join(format!("{id}.jcb"));
    tokio::fs::create_dir_all(&staging_dir).await?;
    match tokio::fs::rename(&original, &staged).await {
        Ok(()) => Ok(Some(StagedBlob { original, staged })),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

async fn restore_staged_blobs(staged_blobs: &[StagedBlob]) -> Result<(), GovernanceError> {
    for staged in staged_blobs.iter().rev() {
        if let Some(parent) = staged.original.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::rename(&staged.staged, &staged.original).await?;
    }
    Ok(())
}

async fn remove_staged_blobs(staged_blobs: &[StagedBlob]) -> Result<(), std::io::Error> {
    let mut first_error = None;
    for staged in staged_blobs {
        if let Err(error) = tokio::fs::remove_file(&staged.staged).await
            && error.kind() != std::io::ErrorKind::NotFound
            && first_error.is_none()
        {
            first_error = Some(error);
        }
    }
    match first_error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

async fn finalize_delete_success(
    pool: &SqlitePool,
    context: &AccessContext,
    target_digest: &str,
) -> Result<(), GovernanceError> {
    let receipt_json = serde_json::json!({
        "operation": "hard_delete",
        "target_digest": target_digest,
        "request_id": context.request_id,
        "reconsolidation_queued": true,
        "cleanup_pending": false,
    });
    let detail_digest = digest(
        format!(
            "delete\0{target_digest}\0{}\0{}",
            context.request_id,
            AuditResult::Succeeded.as_str()
        )
        .as_bytes(),
    );
    let mut transaction = pool.begin().await?;
    sqlx::query(
        "UPDATE governance_receipts SET result = 'succeeded', receipt_json = ?2
         WHERE request_id = ?1 AND operation = 'hard_delete' AND result = 'failed'",
    )
    .bind(&context.request_id)
    .bind(receipt_json.to_string())
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "UPDATE audit_logs SET result = 'succeeded', detail_digest = ?2
         WHERE request_id = ?1 AND operation = 'delete' AND result = 'failed'",
    )
    .bind(&context.request_id)
    .bind(detail_digest)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(())
}

fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

pub(crate) fn scope_key(
    scope: &jc_memory_protocol::MemoryScope,
) -> Result<String, GovernanceError> {
    let value = serde_json::to_value(scope)
        .map_err(|error| GovernanceError::Database(sqlx::Error::Protocol(error.to_string())))?;
    let bytes = serde_json::to_vec(&canonicalize(value))
        .map_err(|error| GovernanceError::Database(sqlx::Error::Protocol(error.to_string())))?;
    Ok(digest(&bytes))
}

fn canonicalize(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Array(values) => {
            serde_json::Value::Array(values.into_iter().map(canonicalize).collect())
        }
        serde_json::Value::Object(values) => {
            let mut entries: Vec<_> = values.into_iter().collect();
            entries.sort_by(|left, right| left.0.cmp(&right.0));
            serde_json::Value::Object(
                entries
                    .into_iter()
                    .map(|(key, value)| (key, canonicalize(value)))
                    .collect(),
            )
        }
        primitive => primitive,
    }
}
