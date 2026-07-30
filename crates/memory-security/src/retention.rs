//! 先计划、后执行的可审计 Retention。

use chrono::{DateTime, Utc};
use jc_memory_protocol::{AccessContext, Decision, FragmentId, Operation};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use uuid::Uuid;

use crate::{AclEngine, GovernanceError, GovernanceService, HardDeleteReceipt, delete::scope_key};

/// 基于观察时间的最小 Retention 策略。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RetentionPolicy {
    cutoff: DateTime<Utc>,
}

impl RetentionPolicy {
    /// 选择截止时间之前的 Fragment。
    #[must_use]
    pub const fn before(cutoff: DateTime<Utc>) -> Self {
        Self { cutoff }
    }
}

/// 已持久化且不含正文的 Retention 执行计划。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RetentionPlan {
    /// 计划标识。
    pub id: Uuid,
    /// 候选 Fragment 标识，按观察时间与 ID 稳定排序。
    pub candidates: Vec<FragmentId>,
    /// 计划内容摘要。
    pub digest: String,
}

impl GovernanceService {
    /// 只生成并持久化候选计划，不删除任何数据。
    ///
    /// # Errors
    ///
    /// 缺少 `Delete` Grant 或计划无法持久化时返回错误。
    pub async fn plan_retention(
        &self,
        context: &AccessContext,
        policy: RetentionPolicy,
    ) -> Result<RetentionPlan, GovernanceError> {
        if let Decision::Deny(reason) =
            AclEngine.authorize(context, Operation::Delete, &context.scope)
        {
            return Err(GovernanceError::AccessDenied { reason });
        }
        let scope_key = scope_key(&context.scope)?;
        let raw_ids: Vec<String> = sqlx::query_scalar(
            "SELECT id FROM fragments
             WHERE scope_key = ?1 AND observed_at_us < ?2
             ORDER BY observed_at_us, id",
        )
        .bind(&scope_key)
        .bind(policy.cutoff.timestamp_micros())
        .fetch_all(&self.pool)
        .await?;
        let candidates = raw_ids
            .into_iter()
            .map(|id| id.parse().map_err(|_| GovernanceError::TargetNotFound))
            .collect::<Result<Vec<_>, _>>()?;
        let id = Uuid::now_v7();
        let plan_bytes = serde_json::to_vec(&serde_json::json!({
            "id": id,
            "cutoff_us": policy.cutoff.timestamp_micros(),
            "candidates": candidates,
        }))
        .map_err(|error| GovernanceError::Database(sqlx::Error::Protocol(error.to_string())))?;
        let digest = hex::encode(Sha256::digest(&plan_bytes));
        let now = Utc::now().timestamp_micros();
        let progress = serde_json::json!({
            "completed_count": 0,
            "next_candidate_index": 0,
            "failed_target_digest": null,
        });
        sqlx::query(
            "INSERT INTO retention_plans
             (id, scope_key, cutoff_us, plan_digest, plan_json, progress_json,
              status, created_at_us, updated_at_us)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'planned', ?7, ?7)",
        )
        .bind(id.to_string())
        .bind(scope_key)
        .bind(policy.cutoff.timestamp_micros())
        .bind(&digest)
        .bind(String::from_utf8_lossy(&plan_bytes).into_owned())
        .bind(progress.to_string())
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(RetentionPlan {
            id,
            candidates,
            digest,
        })
    }

    /// 仅执行仍处于 `planned` 且摘要匹配的持久化计划。
    ///
    /// # Errors
    ///
    /// 缺少 `Delete` Grant，或计划不存在、已执行、被篡改、任一删除失败时返回错误。
    #[allow(clippy::too_many_lines)]
    pub async fn execute_retention(
        &self,
        context: &AccessContext,
        plan: &RetentionPlan,
    ) -> Result<Vec<HardDeleteReceipt>, GovernanceError> {
        if let Decision::Deny(reason) =
            AclEngine.authorize(context, Operation::Delete, &context.scope)
        {
            return Err(GovernanceError::AccessDenied { reason });
        }
        let authorized_scope_key = scope_key(&context.scope)?;
        let stored: Option<(String, String, String, String)> = sqlx::query_as(
            "SELECT plan_digest, plan_json, progress_json, status
             FROM retention_plans WHERE id = ?1 AND scope_key = ?2",
        )
        .bind(plan.id.to_string())
        .bind(authorized_scope_key)
        .fetch_optional(&self.pool)
        .await?;
        let Some((stored_digest, stored_json, progress_json, status)) = stored else {
            return Err(GovernanceError::InvalidRetentionPlan);
        };
        let actual_digest = hex::encode(Sha256::digest(stored_json.as_bytes()));
        let stored_value: serde_json::Value = serde_json::from_str(&stored_json)
            .map_err(|_| GovernanceError::InvalidRetentionPlan)?;
        let stored_candidates = stored_value
            .get("candidates")
            .and_then(serde_json::Value::as_array)
            .ok_or(GovernanceError::InvalidRetentionPlan)?;
        let requested = plan
            .candidates
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        let persisted = stored_candidates
            .iter()
            .filter_map(serde_json::Value::as_str)
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if !matches!(status.as_str(), "planned" | "failed")
            || stored_digest != actual_digest
            || plan.digest != stored_digest
            || requested != persisted
        {
            return Err(GovernanceError::InvalidRetentionPlan);
        }
        let progress: serde_json::Value = serde_json::from_str(&progress_json)
            .map_err(|_| GovernanceError::InvalidRetentionPlan)?;
        let next_candidate_index = progress
            .get("next_candidate_index")
            .and_then(serde_json::Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .ok_or(GovernanceError::InvalidRetentionPlan)?;
        if next_candidate_index > plan.candidates.len()
            || (status == "planned" && next_candidate_index != 0)
        {
            return Err(GovernanceError::InvalidRetentionPlan);
        }
        let changed = sqlx::query(
            "UPDATE retention_plans SET status = 'executing', updated_at_us = ?3
             WHERE id = ?1 AND plan_digest = ?2 AND status = ?4",
        )
        .bind(plan.id.to_string())
        .bind(&plan.digest)
        .bind(Utc::now().timestamp_micros())
        .bind(&status)
        .execute(&self.pool)
        .await?
        .rows_affected();
        if changed != 1 {
            return Err(GovernanceError::InvalidRetentionPlan);
        }
        let mut receipts = Vec::with_capacity(plan.candidates.len() - next_candidate_index);
        for (candidate_index, candidate) in plan
            .candidates
            .iter()
            .enumerate()
            .skip(next_candidate_index)
        {
            let mut candidate_context = context.clone();
            candidate_context.request_id =
                format!("{}:retention:{candidate_index}", context.request_id);
            match self.hard_delete(&candidate_context, *candidate).await {
                Ok(receipt) => {
                    receipts.push(receipt);
                    let completed = candidate_index + 1;
                    let progress = serde_json::json!({
                        "completed_count": completed,
                        "next_candidate_index": completed,
                        "failed_target_digest": null,
                    });
                    sqlx::query(
                        "UPDATE retention_plans SET progress_json = ?2, updated_at_us = ?3
                         WHERE id = ?1 AND status = 'executing'",
                    )
                    .bind(plan.id.to_string())
                    .bind(progress.to_string())
                    .bind(Utc::now().timestamp_micros())
                    .execute(&self.pool)
                    .await?;
                }
                Err(error) => {
                    let failed_target_digest =
                        hex::encode(Sha256::digest(candidate.to_string().as_bytes()));
                    let progress = serde_json::json!({
                        "completed_count": candidate_index,
                        "next_candidate_index": candidate_index,
                        "failed_target_digest": failed_target_digest,
                    });
                    sqlx::query(
                        "UPDATE retention_plans
                         SET status = 'failed', progress_json = ?2, updated_at_us = ?3
                         WHERE id = ?1 AND status = 'executing'",
                    )
                    .bind(plan.id.to_string())
                    .bind(progress.to_string())
                    .bind(Utc::now().timestamp_micros())
                    .execute(&self.pool)
                    .await?;
                    return Err(error);
                }
            }
        }
        let completed_progress = serde_json::json!({
            "completed_count": plan.candidates.len(),
            "next_candidate_index": plan.candidates.len(),
            "failed_target_digest": null,
        });
        sqlx::query(
            "UPDATE retention_plans
             SET status = 'completed', progress_json = ?2, updated_at_us = ?3
             WHERE id = ?1 AND status = 'executing'",
        )
        .bind(plan.id.to_string())
        .bind(completed_progress.to_string())
        .bind(Utc::now().timestamp_micros())
        .execute(&self.pool)
        .await?;
        Ok(receipts)
    }
}
