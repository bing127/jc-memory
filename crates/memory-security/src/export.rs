//! 精确 Scope 的 canonical JSONL 与 Blob 导出。

use std::path::{Path, PathBuf};

use chrono::Utc;
use jc_memory_protocol::{AccessContext, Decision, Operation};
use sha2::{Digest as _, Sha256};
use tokio::io::AsyncWriteExt as _;

use crate::{AclEngine, AuditResult, GovernanceError, GovernanceService, delete::scope_key};

/// 已原子发布的治理导出结果。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GovernanceExportReceipt {
    /// 导出目录。
    pub path: PathBuf,
    /// canonical JSONL 的 SHA-256。
    pub authority_sha256: String,
}

impl GovernanceService {
    /// 输出 Manifest、canonical JSONL 和当前 Scope 实际引用的 Blob。
    ///
    /// # Errors
    ///
    /// 缺少 `Export` Grant、目标已存在或导出文件校验失败时返回错误。
    #[allow(clippy::too_many_lines)]
    pub async fn export_scope(
        &self,
        context: &AccessContext,
        root: impl AsRef<Path>,
        name: &str,
    ) -> Result<GovernanceExportReceipt, GovernanceError> {
        let scope_key = scope_key(&context.scope)?;
        let target_digest = hex::encode(Sha256::digest(format!("{scope_key}\0{name}").as_bytes()));
        if let Decision::Deny(reason) =
            AclEngine.authorize(context, Operation::Export, &context.scope)
        {
            self.write_audit(context, "export", &target_digest, AuditResult::Denied)
                .await?;
            return Err(GovernanceError::AccessDenied { reason });
        }
        if name.is_empty() || name.contains('/') || name.contains("..") {
            return Err(GovernanceError::TargetNotFound);
        }
        let root = root.as_ref();
        tokio::fs::create_dir_all(root).await?;
        let target = root.join(name);
        if tokio::fs::try_exists(&target).await? {
            return Err(GovernanceError::TargetNotFound);
        }
        let staging = root.join(format!(".{name}.{}.staging", uuid::Uuid::now_v7()));
        tokio::fs::create_dir(&staging).await?;
        let export_result: Result<String, GovernanceError> = async {
            let rows: Vec<String> = sqlx::query_scalar(
            "SELECT row_json FROM (
                 SELECT '1' AS order_key, json_object('table','fragment_contents','record',
                    json_object('id',c.id,'storage_kind',c.storage_kind,'inline_hex',
                    CASE WHEN c.inline_value IS NULL THEN NULL ELSE hex(c.inline_value) END,
                    'blob_id',c.blob_id,'media_type',c.media_type,'byte_len',c.byte_len,
                    'sha256',c.sha256,'created_at_us',c.created_at_us)) AS row_json
                 FROM fragment_contents c JOIN fragments f ON f.content_id=c.id WHERE f.scope_key=?1
                 UNION ALL
                 SELECT '2', json_object('table','fragments','record',json_object(
                    'id',f.id,'scope_key',f.scope_key,'namespace_id',f.namespace_id,
                    'tenant_id',f.tenant_id,'user_id',f.user_id,'agent_id',f.agent_id,
                    'project_id',f.project_id,'session_id',f.session_id,'trace_id',f.trace_id,
                    'kind',f.kind,'content_id',f.content_id,'source_json',json(f.source_json),
                    'perspective',f.perspective,'occurred_at_us',f.occurred_at_us,
                    'observed_at_us',f.observed_at_us,'confidence',f.confidence,
                    'importance',f.importance,'validity',f.validity,
                    'security_label',f.security_label,'metadata_json',json(f.metadata_json),
                    'created_at_us',f.created_at_us))
                 FROM fragments f WHERE f.scope_key=?1
                 UNION ALL
                 SELECT '3', json_object('table','events','record',json_object(
                    'id',e.id,'fragment_id',e.fragment_id,'scope_key',e.scope_key,
                    'namespace_id',e.namespace_id,'tenant_id',e.tenant_id,'stage',e.stage,
                    'sequence',e.sequence,'recorded_at_us',e.recorded_at_us,
                    'content_hash',e.content_hash,'payload_json',json(e.payload_json)))
                 FROM events e WHERE e.scope_key=?1
                 UNION ALL
                 SELECT '4', json_object('table','evidence_refs','record',json_object(
                    'id',r.id,'owner_fragment_id',r.owner_fragment_id,
                    'referenced_fragment_id',r.referenced_fragment_id,
                    'referenced_event_id',r.referenced_event_id,
                    'verification_method',r.verification_method,'note',r.note,
                    'created_at_us',r.created_at_us))
                 FROM evidence_refs r JOIN fragments f ON f.id=r.owner_fragment_id WHERE f.scope_key=?1
             ) ORDER BY order_key, row_json",
            )
            .bind(&scope_key)
            .fetch_all(&self.pool)
            .await?;
            let authority_bytes = if rows.is_empty() {
                String::new()
            } else {
                rows.join("\n") + "\n"
            };
            let authority_sha256 = hex::encode(Sha256::digest(authority_bytes.as_bytes()));
            let mut authority = tokio::fs::File::create(staging.join("authority.jsonl")).await?;
            authority.write_all(authority_bytes.as_bytes()).await?;
            authority.sync_all().await?;
            drop(authority);
            let blob_ids: Vec<String> = sqlx::query_scalar(
            "SELECT DISTINCT c.blob_id FROM fragment_contents c
             JOIN fragments f ON f.content_id=c.id
             WHERE f.scope_key=?1 AND c.blob_id IS NOT NULL ORDER BY c.blob_id",
            )
            .bind(&scope_key)
            .fetch_all(&self.pool)
            .await?;
            let mut blob_manifest = Vec::with_capacity(blob_ids.len());
            for id in &blob_ids {
                let relative = blob_relative(id);
                let destination = staging.join(&relative);
                let Some(parent) = destination.parent() else {
                    return Err(GovernanceError::TargetNotFound);
                };
                tokio::fs::create_dir_all(parent).await?;
                tokio::fs::copy(self.blobs.root().join(&relative[6..]), &destination).await?;
                let exported_bytes = tokio::fs::read(&destination).await?;
                blob_manifest.push(serde_json::json!({
                    "id": id,
                    "path": relative,
                    "sha256": hex::encode(Sha256::digest(&exported_bytes)),
                    "byte_len": exported_bytes.len(),
                }));
            }
            let manifest = serde_json::json!({
                "manifest_version": 1,
                "scope": context.scope,
                "created_at": Utc::now(),
                "authority": {"path":"authority.jsonl","sha256":authority_sha256},
                "blobs": blob_manifest,
            });
            let mut file = tokio::fs::File::create(staging.join("manifest.json")).await?;
            file.write_all(
                serde_json::to_vec_pretty(&manifest)
                    .map_err(|error| {
                        GovernanceError::Database(sqlx::Error::Protocol(error.to_string()))
                    })?
                    .as_slice(),
            )
            .await?;
            file.sync_all().await?;
            drop(file);
            tokio::fs::rename(&staging, &target).await?;
            Ok(authority_sha256)
        }
        .await;
        let authority_sha256 = match export_result {
            Ok(authority_sha256) => authority_sha256,
            Err(error) => {
                match tokio::fs::remove_dir_all(&staging).await {
                    Ok(()) => {}
                    Err(cleanup_error) if cleanup_error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(cleanup_error) => return Err(cleanup_error.into()),
                }
                self.write_audit(context, "export", &target_digest, AuditResult::Failed)
                    .await?;
                return Err(error);
            }
        };
        self.write_audit(context, "export", &authority_sha256, AuditResult::Succeeded)
            .await?;
        Ok(GovernanceExportReceipt {
            path: target,
            authority_sha256,
        })
    }
}

fn blob_relative(id: &str) -> String {
    format!("blobs/{}/{}/{id}.jcb", &id[..2], &id[2..4])
}
