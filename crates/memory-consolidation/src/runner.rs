//! Cancellation、批循环、墙钟预算和重试协调。

use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use jc_memory_protocol::MemoryScope;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{Row as _, Sqlite, Transaction};
use tokio::time::{Instant, timeout_at};
use tokio_util::sync::CancellationToken;

use crate::{
    BatchOutcome, ConsolidationJobStore, JobCheckpoint, JobError, JobId, JobRecord, JobStatus,
    ResourceLimits,
    checkpoint::BatchUsage,
    job::{load_for_update, scope_key, stage_committed_batch, verify_worker_in_write_transaction},
};

enum BatchAttempt {
    Cancelled,
    TimedOut(&'static str),
    Finished(Result<BatchOutcome, JobError>),
}

/// 受巩固边界保护的 `SQLite` 表类别。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConsolidationTable {
    /// 派生 Point 表。
    Points,
    /// 派生 Edge 表。
    Edges,
    /// 派生 Surface 表。
    Surfaces,
    /// 派生 Entity 表。
    Entities,
    /// 派生 State 表。
    States,
    /// 派生巩固产物表。
    Artifacts,
    /// 派生冷层位置索引。
    ColdTier,
    /// 可重建全文索引。
    FragmentFts,
    /// 权威 Fragment 表；巩固任务不得修改。
    Fragments,
    /// 权威 Event 表；巩固任务不得修改。
    Events,
    /// 权威 `EvidenceRef` 表；巩固任务不得修改。
    EvidenceRefs,
}

impl ConsolidationTable {
    const fn name(self) -> &'static str {
        match self {
            Self::Points => "points",
            Self::Edges => "edges",
            Self::Surfaces => "surfaces",
            Self::Entities => "entities",
            Self::States => "states",
            Self::Artifacts => "consolidation_artifacts",
            Self::ColdTier => "cold_tier_entries",
            Self::FragmentFts => "fragment_fts",
            Self::Fragments => "fragments",
            Self::Events => "events",
            Self::EvidenceRefs => "evidence_refs",
        }
    }

    const fn delete_scope_sql(self) -> Option<&'static str> {
        match self {
            Self::Points => Some("DELETE FROM points WHERE scope_key = ?1"),
            Self::Edges => Some("DELETE FROM edges WHERE scope_key = ?1"),
            Self::Surfaces => Some("DELETE FROM surfaces WHERE scope_key = ?1"),
            Self::Entities => Some("DELETE FROM entities WHERE scope_key = ?1"),
            Self::States => Some("DELETE FROM states WHERE scope_key = ?1"),
            Self::Artifacts => Some("DELETE FROM consolidation_artifacts WHERE scope_key = ?1"),
            Self::ColdTier => Some("DELETE FROM cold_tier_entries WHERE scope_key = ?1"),
            Self::FragmentFts => Some("DELETE FROM fragment_fts WHERE scope_key = ?1"),
            Self::Fragments | Self::Events | Self::EvidenceRefs => None,
        }
    }
}

/// Handler 从权威 Event 流读取的 Scope 绑定视图。
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct AuthorityEvent {
    /// Scope 内单调递增的权威游标。
    pub sequence: u64,
    /// 权威 Event 标识。
    pub event_id: String,
    /// Event 携带的权威 Fragment 标识。
    pub fragment_id: String,
    /// Fragment 类别。
    pub fragment_kind: String,
    /// 原始来源标识。
    pub source_id: String,
    /// 权威内容散列。
    pub content_hash: String,
    /// 归一化原始内容标识，不包含 Event/来源差异。
    pub content_id: String,
    /// 原始 Fragment JSON，仅供派生计算读取。
    pub payload: Value,
    /// Event 记录时间。
    pub recorded_at_us: i64,
    /// Fragment 置信度。
    pub confidence: f64,
    /// Fragment 有效性。
    pub validity: String,
}

/// 派生 Point 的静态白名单写入参数。
#[derive(Clone, Debug, PartialEq)]
pub struct PointWrite {
    /// 派生 Point 标识。
    pub id: String,
    /// Point 类别。
    pub kind: String,
    /// 权威来源 Fragment。
    pub source_fragment_id: String,
    /// 有效期起点。
    pub valid_from_us: i64,
    /// 派生置信度。
    pub confidence: f64,
    /// 创建时间。
    pub created_at_us: i64,
}

/// 派生 Edge 的静态白名单写入参数。
#[derive(Clone, Debug, PartialEq)]
pub struct EdgeWrite {
    /// 派生 Edge 标识。
    pub id: String,
    /// 来源 Point。
    pub source_point_id: String,
    /// 目标 Point。
    pub target_point_id: String,
    /// 白名单 Edge 类别。
    pub kind: String,
    /// 初始权重。
    pub weight: f64,
    /// 派生置信度。
    pub confidence: f64,
    /// 权威来源 Fragment。
    pub source_fragment_id: String,
    /// 有效期起点。
    pub valid_from_us: i64,
    /// 创建时间。
    pub created_at_us: i64,
}

/// 一个不可覆盖旧版本的 Surface 派生写入。
#[derive(Clone, Debug, PartialEq)]
pub struct SurfaceVersionWrite {
    /// 跨版本稳定的 Surface 标识。
    pub id: String,
    /// Surface 类别。
    pub kind: String,
    /// 当前版本状态。
    pub status: String,
    /// 语义中心 JSON。
    pub semantic_center: Value,
    /// 激活阈值。
    pub activation_threshold: f64,
    /// 派生置信度。
    pub confidence: f64,
    /// 单调版本号。
    pub version: u64,
    /// 有效期起点。
    pub valid_from_us: i64,
    /// 权威 Evidence/provenance JSON。
    pub evidence: Value,
    /// 当前版本成员及其权重。
    pub members: Vec<(String, f64)>,
}

/// 可重建巩固产物的静态白名单写入参数。
#[derive(Clone, Debug, PartialEq)]
pub struct ArtifactWrite {
    /// 幂等派生产物标识。
    pub id: String,
    /// 产物类别。
    pub kind: String,
    /// Scope 内主题键。
    pub subject_key: String,
    /// 产生该产物的 Event cursor。
    pub source_cursor: u64,
    /// 派生载荷。
    pub payload: Value,
    /// 权威 Evidence/provenance。
    pub evidence: Value,
    /// 创建时间。
    pub created_at_us: i64,
}

/// 一个由 crate 白名单解释的派生写请求。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DerivedMutation {
    table: ConsolidationTable,
    scope: MemoryScope,
}

impl DerivedMutation {
    /// 创建一个只删除精确 Scope 派生行的请求。
    #[must_use]
    pub const fn delete_scope(table: ConsolidationTable, scope: MemoryScope) -> Self {
        Self { table, scope }
    }
}

/// Handler 可见的受限批次上下文。
///
/// 上下文不公开底层 `SQLx` Transaction。所有数据库变更必须经过
/// [`Self::apply`]，因此只能修改白名单派生表，并自动计入 IO 预算。
///
/// ```compile_fail
/// use jc_memory_consolidation::BatchContext;
///
/// fn raw_transaction(context: &mut BatchContext<'_>) {
///     let _ = &mut context.transaction;
/// }
/// ```
pub struct BatchContext<'transaction> {
    transaction: &'transaction mut Transaction<'static, Sqlite>,
    checkpoint: JobCheckpoint,
    scope_key: String,
    limits: ResourceLimits,
    cancellation: CancellationToken,
    processed_items: u32,
    io_ops: u64,
}

impl<'transaction> BatchContext<'transaction> {
    fn new(
        transaction: &'transaction mut Transaction<'static, Sqlite>,
        checkpoint: &JobCheckpoint,
        limits: ResourceLimits,
        cancellation: &CancellationToken,
    ) -> Result<Self, JobError> {
        Ok(Self {
            transaction,
            checkpoint: checkpoint.clone(),
            scope_key: scope_key(&checkpoint.scope)?,
            limits,
            cancellation: cancellation.clone(),
            processed_items: 0,
            io_ops: 0,
        })
    }

    /// 返回当前批次绑定的不可变 Checkpoint。
    #[must_use]
    pub const fn checkpoint(&self) -> &JobCheckpoint {
        &self.checkpoint
    }

    /// 返回当前批次绑定的精确 Scope。
    #[must_use]
    pub const fn scope(&self) -> &MemoryScope {
        &self.checkpoint.scope
    }

    /// 返回 Runner 传入的协作取消 Token。
    #[must_use]
    pub const fn cancellation(&self) -> &CancellationToken {
        &self.cancellation
    }

    /// 返回单批允许处理的最大条目数。
    #[must_use]
    pub const fn batch_size(&self) -> u32 {
        self.limits.max_batch_size
    }

    /// 在处理条目前消费批量预算令牌。
    ///
    /// # Errors
    ///
    /// 累计条目超过单批上限时返回预算错误。
    pub fn consume_items(&mut self, count: u32) -> Result<(), JobError> {
        let processed_items = self
            .processed_items
            .checked_add(count)
            .ok_or(JobError::BudgetExceeded("batch_size"))?;
        if processed_items > self.limits.max_batch_size {
            return Err(JobError::BudgetExceeded("batch_size"));
        }
        self.processed_items = processed_items;
        Ok(())
    }

    fn charge_io(&mut self) -> Result<(), JobError> {
        let io_ops = self
            .io_ops
            .checked_add(1)
            .ok_or(JobError::BudgetExceeded("io"))?;
        let total_io_ops = self
            .checkpoint
            .io_ops
            .checked_add(io_ops)
            .ok_or(JobError::BudgetExceeded("io"))?;
        if total_io_ops > self.limits.max_io_ops {
            return Err(JobError::BudgetExceeded("io"));
        }
        self.io_ops = io_ops;
        Ok(())
    }

    /// 从 Checkpoint cursor 开始读取一个受批量上限约束的权威 Event 批次。
    ///
    /// # Errors
    ///
    /// 取消、预算超限、数据损坏或数据库失败时返回错误。
    pub async fn read_authority_events(&mut self) -> Result<Vec<AuthorityEvent>, JobError> {
        if self.cancellation.is_cancelled() {
            return Err(JobError::Cancelled);
        }
        self.charge_io()?;
        let cursor = i64::try_from(self.checkpoint.cursor)
            .map_err(|_| JobError::Corrupt("event cursor exceeds SQLite INTEGER".into()))?;
        let limit = i64::from(self.limits.max_batch_size);
        let rows = sqlx::query(
            "SELECT e.sequence, e.id AS event_id, e.fragment_id, e.content_hash,
                    e.payload_json, e.recorded_at_us, f.kind, f.content_id, f.source_json,
                    f.confidence, f.validity
             FROM events e
             JOIN fragments f ON f.id = e.fragment_id AND f.scope_key = e.scope_key
             WHERE e.scope_key = ?1 AND e.sequence >= ?2
             ORDER BY e.sequence ASC
             LIMIT ?3",
        )
        .bind(&self.scope_key)
        .bind(cursor)
        .bind(limit)
        .fetch_all(&mut **self.transaction)
        .await?;
        self.consume_items(
            u32::try_from(rows.len())
                .map_err(|_| JobError::Corrupt("event batch exceeds u32".into()))?,
        )?;
        rows.iter().map(decode_authority_event).collect()
    }

    /// 读取同一原始内容的有限权威支持集，不把摘要或 Embedding 当作事实来源。
    ///
    /// # Errors
    ///
    /// IO 预算超限、权威行损坏或数据库失败时返回错误。
    pub async fn authority_events_by_content(
        &mut self,
        event: &AuthorityEvent,
    ) -> Result<Vec<AuthorityEvent>, JobError> {
        self.charge_io()?;
        let rows = sqlx::query(
            "SELECT e.sequence, e.id AS event_id, e.fragment_id, e.content_hash,
                    e.payload_json, e.recorded_at_us, f.kind, f.content_id, f.source_json,
                    f.confidence, f.validity
             FROM events e
             JOIN fragments f ON f.id = e.fragment_id AND f.scope_key = e.scope_key
             WHERE e.scope_key = ?1
               AND f.content_id = (SELECT content_id FROM fragments WHERE id = ?2)
             ORDER BY e.sequence ASC LIMIT ?3",
        )
        .bind(&self.scope_key)
        .bind(&event.fragment_id)
        .bind(i64::from(self.limits.max_batch_size))
        .fetch_all(&mut **self.transaction)
        .await?;
        rows.iter().map(decode_authority_event).collect()
    }

    /// 幂等写入一个 Scope 绑定派生 Point。
    ///
    /// # Errors
    ///
    /// IO 预算超限、外键不成立或数据库失败时返回错误。
    pub async fn write_point(&mut self, point: &PointWrite) -> Result<(), JobError> {
        if !self
            .fragment_belongs_to_scope(&point.source_fragment_id)
            .await?
        {
            return Err(JobError::ReferenceScopeMismatch("source_fragment_id"));
        }
        self.charge_io()?;
        sqlx::query(
            "INSERT INTO points (
                id, scope_key, kind, source_fragment_id, valid_from_us,
                confidence, created_at_us
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(id) DO NOTHING",
        )
        .bind(&point.id)
        .bind(&self.scope_key)
        .bind(&point.kind)
        .bind(&point.source_fragment_id)
        .bind(point.valid_from_us)
        .bind(point.confidence)
        .bind(point.created_at_us)
        .execute(&mut **self.transaction)
        .await?;
        Ok(())
    }

    /// 查找当前 Scope 内一个 Fragment 对应的派生 Point。
    ///
    /// # Errors
    ///
    /// IO 预算超限或数据库失败时返回错误。
    pub async fn point_for_fragment(
        &mut self,
        fragment_id: &str,
    ) -> Result<Option<String>, JobError> {
        self.charge_io()?;
        Ok(sqlx::query_scalar(
            "SELECT id FROM points WHERE scope_key = ?1 AND source_fragment_id = ?2 ORDER BY created_at_us ASC LIMIT 1",
        )
        .bind(&self.scope_key)
        .bind(fragment_id)
        .fetch_optional(&mut **self.transaction)
        .await?)
    }

    /// 查找同内容或同一次 trace/session 的早期 Point，并返回保守关系类别。
    ///
    /// # Errors
    ///
    /// IO 预算超限、权威内容损坏或数据库失败时返回错误。
    pub async fn dedup_candidate(
        &mut self,
        event: &AuthorityEvent,
    ) -> Result<Option<(String, String)>, JobError> {
        self.charge_io()?;
        let trace_id = event.payload.get("trace_id").and_then(Value::as_str);
        let session_id = event.payload.get("session_id").and_then(Value::as_str);
        let row = sqlx::query(
            "SELECT p.id,
                    CASE
                        WHEN e.content_hash = ?3
                          OR f.content_id = (SELECT content_id FROM fragments WHERE id = ?4)
                        THEN 'same_entity'
                        ELSE 'co_activated'
                    END AS edge_kind
             FROM events e
             JOIN fragments f ON f.id = e.fragment_id AND f.scope_key = e.scope_key
             JOIN points p ON p.source_fragment_id = e.fragment_id AND p.scope_key = e.scope_key
             WHERE e.scope_key = ?1 AND e.sequence < ?2
               AND (e.content_hash = ?3
                    OR f.content_id = (SELECT content_id FROM fragments WHERE id = ?4)
                    OR (?5 IS NOT NULL AND json_extract(e.payload_json, '$.trace_id') = ?5)
                    OR (?6 IS NOT NULL AND json_extract(e.payload_json, '$.session_id') = ?6))
             ORDER BY e.sequence DESC LIMIT 1",
        )
        .bind(&self.scope_key)
        .bind(
            i64::try_from(event.sequence)
                .map_err(|_| JobError::Corrupt("event sequence exceeds SQLite INTEGER".into()))?,
        )
        .bind(&event.content_hash)
        .bind(&event.fragment_id)
        .bind(trace_id)
        .bind(session_id)
        .fetch_optional(&mut **self.transaction)
        .await?;
        row.map(|row| Ok((row.try_get("id")?, row.try_get("edge_kind")?)))
            .transpose()
    }

    /// 幂等写入一个只允许保守关系类型的派生 Edge。
    ///
    /// # Errors
    ///
    /// Edge 类别不在静态白名单、IO 预算超限或数据库失败时返回错误。
    pub async fn write_edge(&mut self, edge: &EdgeWrite) -> Result<(), JobError> {
        if !matches!(
            edge.kind.as_str(),
            "same_entity" | "co_activated" | "supports" | "contradicts"
        ) {
            return Err(JobError::Task(format!(
                "edge kind is not consolidation-whitelisted: {}",
                edge.kind
            )));
        }
        if !self.edge_references_belong_to_scope(edge).await? {
            return Err(JobError::ReferenceScopeMismatch("edge_references"));
        }
        self.charge_io()?;
        sqlx::query(
            "INSERT INTO edges (
                id, scope_key, source_point_id, target_point_id, kind, weight,
                confidence, source_fragment_id, valid_from_us, created_at_us
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(id) DO NOTHING",
        )
        .bind(&edge.id)
        .bind(&self.scope_key)
        .bind(&edge.source_point_id)
        .bind(&edge.target_point_id)
        .bind(&edge.kind)
        .bind(edge.weight.clamp(0.0, 1.0))
        .bind(edge.confidence.clamp(0.0, 1.0))
        .bind(&edge.source_fragment_id)
        .bind(edge.valid_from_us)
        .bind(edge.created_at_us)
        .execute(&mut **self.transaction)
        .await?;
        Ok(())
    }

    /// 按权威 Evidence 指向的 Edge 标识有界调整权重。
    ///
    /// # Errors
    ///
    /// IO 预算超限或数据库失败时返回错误。
    pub async fn adjust_edge_weight(
        &mut self,
        edge_id: &str,
        delta: f64,
    ) -> Result<bool, JobError> {
        self.charge_io()?;
        let result = sqlx::query(
            "UPDATE edges SET weight = min(1.0, max(0.0, weight + ?1))
             WHERE id = ?2 AND scope_key = ?3",
        )
        .bind(delta)
        .bind(edge_id)
        .bind(&self.scope_key)
        .execute(&mut **self.transaction)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    /// 插入一个新 Surface 版本；相同 `(id, version)` 只允许幂等重放。
    ///
    /// # Errors
    ///
    /// 版本溢出、IO 预算超限、成员外键无效或数据库失败时返回错误。
    pub async fn write_surface_version(
        &mut self,
        surface: &SurfaceVersionWrite,
    ) -> Result<(), JobError> {
        if !self
            .surface_id_belongs_to_scope_or_is_new(&surface.id)
            .await?
        {
            return Err(JobError::ReferenceScopeMismatch("surface_id"));
        }
        for (point_id, _) in &surface.members {
            if !self.point_belongs_to_scope(point_id).await? {
                return Err(JobError::ReferenceScopeMismatch("surface_member"));
            }
        }
        let version = i64::try_from(surface.version)
            .map_err(|_| JobError::Corrupt("surface version exceeds SQLite INTEGER".into()))?;
        if version > 1 {
            self.charge_io()?;
            let previous = sqlx::query(
                "UPDATE surfaces SET valid_to_us = ?1
                 WHERE id = ?2 AND scope_key = ?3 AND version = ?4
                   AND (valid_to_us IS NULL OR valid_to_us = ?1)",
            )
            .bind(surface.valid_from_us)
            .bind(&surface.id)
            .bind(&self.scope_key)
            .bind(version - 1)
            .execute(&mut **self.transaction)
            .await?;
            if previous.rows_affected() != 1 {
                return Err(JobError::Task(
                    "surface version must append to the preceding scoped version".into(),
                ));
            }
        }
        self.charge_io()?;
        let evidence = serde_json::to_string(&surface.evidence)?;
        let center = serde_json::to_string(&surface.semantic_center)?;
        sqlx::query(
            "INSERT INTO surfaces (
                id, scope_key, kind, status, semantic_center_json,
                activation_threshold, confidence, version, valid_from_us,
                source_evidence_json, created_at_us
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?9)
             ON CONFLICT(id, version) DO NOTHING",
        )
        .bind(&surface.id)
        .bind(&self.scope_key)
        .bind(&surface.kind)
        .bind(&surface.status)
        .bind(center)
        .bind(surface.activation_threshold.clamp(0.0, 1.0))
        .bind(surface.confidence.clamp(0.0, 1.0))
        .bind(version)
        .bind(surface.valid_from_us)
        .bind(evidence)
        .execute(&mut **self.transaction)
        .await?;
        for (point_id, weight) in &surface.members {
            self.charge_io()?;
            sqlx::query(
                "INSERT INTO surface_members (
                    surface_id, point_id, member_weight, version, created_at_us
                 ) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(surface_id, point_id, version) DO NOTHING",
            )
            .bind(&surface.id)
            .bind(point_id)
            .bind(weight.clamp(0.0, 1.0))
            .bind(version)
            .bind(surface.valid_from_us)
            .execute(&mut **self.transaction)
            .await?;
        }
        Ok(())
    }

    async fn fragment_belongs_to_scope(&mut self, fragment_id: &str) -> Result<bool, JobError> {
        self.charge_io()?;
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM fragments WHERE id = ?1 AND scope_key = ?2")
                .bind(fragment_id)
                .bind(&self.scope_key)
                .fetch_one(&mut **self.transaction)
                .await?;
        Ok(count == 1)
    }

    async fn point_belongs_to_scope(&mut self, point_id: &str) -> Result<bool, JobError> {
        self.charge_io()?;
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM points WHERE id = ?1 AND scope_key = ?2")
                .bind(point_id)
                .bind(&self.scope_key)
                .fetch_one(&mut **self.transaction)
                .await?;
        Ok(count == 1)
    }

    async fn surface_id_belongs_to_scope_or_is_new(
        &mut self,
        surface_id: &str,
    ) -> Result<bool, JobError> {
        self.charge_io()?;
        let (total, scoped): (i64, i64) = sqlx::query_as(
            "SELECT COUNT(*), COUNT(*) FILTER (WHERE scope_key = ?2)
             FROM surfaces WHERE id = ?1",
        )
        .bind(surface_id)
        .bind(&self.scope_key)
        .fetch_one(&mut **self.transaction)
        .await?;
        Ok(total == 0 || total == scoped)
    }

    async fn edge_references_belong_to_scope(
        &mut self,
        edge: &EdgeWrite,
    ) -> Result<bool, JobError> {
        self.charge_io()?;
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)
             FROM points source
             JOIN points target ON target.id = ?2 AND target.scope_key = ?4
             JOIN fragments fragment ON fragment.id = ?3 AND fragment.scope_key = ?4
             WHERE source.id = ?1 AND source.scope_key = ?4",
        )
        .bind(&edge.source_point_id)
        .bind(&edge.target_point_id)
        .bind(&edge.source_fragment_id)
        .bind(&self.scope_key)
        .fetch_one(&mut **self.transaction)
        .await?;
        Ok(count == 1)
    }

    /// 读取当前 Scope 的最新 Surface 版本及其成员。
    ///
    /// # Errors
    ///
    /// IO 预算超限、派生 JSON 损坏或数据库失败时返回错误。
    pub async fn latest_surfaces(&mut self) -> Result<Vec<SurfaceVersionWrite>, JobError> {
        self.charge_io()?;
        let rows = sqlx::query(
            "SELECT s.id, s.kind, s.status, s.semantic_center_json,
                    s.activation_threshold, s.confidence, s.version,
                    s.valid_from_us, s.source_evidence_json
             FROM surfaces s
             WHERE s.scope_key = ?1
               AND s.version = (
                   SELECT max(v.version) FROM surfaces v
                   WHERE v.id = s.id AND v.scope_key = s.scope_key
               )
             ORDER BY s.id ASC LIMIT ?2",
        )
        .bind(&self.scope_key)
        .bind(i64::from(self.limits.max_batch_size))
        .fetch_all(&mut **self.transaction)
        .await?;
        let mut surfaces = Vec::with_capacity(rows.len());
        for row in rows {
            self.charge_io()?;
            let id: String = row.try_get("id")?;
            let version: i64 = row.try_get("version")?;
            let members = sqlx::query("SELECT point_id, member_weight FROM surface_members WHERE surface_id = ?1 AND version = ?2 ORDER BY point_id")
                .bind(&id)
                .bind(version)
                .fetch_all(&mut **self.transaction)
                .await?
                .into_iter()
                .map(|member| Ok((member.try_get("point_id")?, member.try_get("member_weight")?)))
                .collect::<Result<Vec<_>, sqlx::Error>>()?;
            surfaces.push(SurfaceVersionWrite {
                id,
                kind: row.try_get("kind")?,
                status: row.try_get("status")?,
                semantic_center: serde_json::from_str(
                    &row.try_get::<String, _>("semantic_center_json")?,
                )?,
                activation_threshold: row.try_get("activation_threshold")?,
                confidence: row.try_get("confidence")?,
                version: u64::try_from(version)
                    .map_err(|_| JobError::Corrupt("negative surface version".into()))?,
                valid_from_us: row.try_get("valid_from_us")?,
                evidence: serde_json::from_str(&row.try_get::<String, _>("source_evidence_json")?)?,
                members,
            });
        }
        Ok(surfaces)
    }

    /// 幂等保存一个保留 Evidence 的可重建巩固产物。
    ///
    /// # Errors
    ///
    /// cursor 溢出、序列化失败、IO 预算超限或数据库失败时返回错误。
    pub async fn write_artifact(&mut self, artifact: &ArtifactWrite) -> Result<(), JobError> {
        self.charge_io()?;
        sqlx::query(
            "INSERT INTO consolidation_artifacts (
                id, scope_key, kind, subject_key, source_cursor,
                payload_json, evidence_json, created_at_us
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(id) DO UPDATE SET
                source_cursor = excluded.source_cursor,
                payload_json = excluded.payload_json,
                evidence_json = excluded.evidence_json,
                created_at_us = excluded.created_at_us
             WHERE consolidation_artifacts.scope_key = excluded.scope_key
               AND consolidation_artifacts.kind = excluded.kind
               AND consolidation_artifacts.subject_key = excluded.subject_key",
        )
        .bind(&artifact.id)
        .bind(&self.scope_key)
        .bind(&artifact.kind)
        .bind(&artifact.subject_key)
        .bind(
            i64::try_from(artifact.source_cursor)
                .map_err(|_| JobError::Corrupt("artifact cursor exceeds SQLite INTEGER".into()))?,
        )
        .bind(serde_json::to_string(&artifact.payload)?)
        .bind(serde_json::to_string(&artifact.evidence)?)
        .bind(artifact.created_at_us)
        .execute(&mut **self.transaction)
        .await?;
        Ok(())
    }

    /// 读取当前 Scope 内一个已持久化派生产物的 Evidence。
    ///
    /// # Errors
    ///
    /// IO 预算超限、派生 JSON 损坏或数据库失败时返回错误。
    pub async fn artifact_evidence(
        &mut self,
        kind: &str,
        subject_key: &str,
    ) -> Result<Option<Value>, JobError> {
        self.charge_io()?;
        let evidence: Option<String> = sqlx::query_scalar(
            "SELECT evidence_json FROM consolidation_artifacts
             WHERE scope_key = ?1 AND kind = ?2 AND subject_key = ?3
             ORDER BY source_cursor DESC LIMIT 1",
        )
        .bind(&self.scope_key)
        .bind(kind)
        .bind(subject_key)
        .fetch_optional(&mut **self.transaction)
        .await?;
        evidence
            .map(|value| serde_json::from_str(&value).map_err(JobError::from))
            .transpose()
    }

    /// 幂等记录权威 Fragment 的派生冷层位置，不修改 Fragment 本身。
    ///
    /// # Errors
    ///
    /// Evidence 序列化失败、IO 预算超限或数据库失败时返回错误。
    pub async fn write_cold_tier(
        &mut self,
        event: &AuthorityEvent,
        evidence: &Value,
    ) -> Result<(), JobError> {
        if !self.authority_event_belongs_to_scope(event).await? {
            return Err(JobError::ReferenceScopeMismatch("authority_event"));
        }
        self.charge_io()?;
        sqlx::query(
            "INSERT INTO cold_tier_entries (
                fragment_id, scope_key, tier, source_event_id, evidence_json, moved_at_us
             ) VALUES (?1, ?2, 'cold', ?3, ?4, ?5)
             ON CONFLICT(fragment_id) DO UPDATE SET
                tier = excluded.tier,
                source_event_id = excluded.source_event_id,
                evidence_json = excluded.evidence_json,
                moved_at_us = excluded.moved_at_us
             WHERE cold_tier_entries.scope_key = excluded.scope_key",
        )
        .bind(&event.fragment_id)
        .bind(&self.scope_key)
        .bind(&event.event_id)
        .bind(serde_json::to_string(evidence)?)
        .bind(event.recorded_at_us)
        .execute(&mut **self.transaction)
        .await?;
        Ok(())
    }

    /// 幂等重建一个权威 Fragment 的可搜索文本派生项。
    ///
    /// # Errors
    ///
    /// IO 预算超限或数据库失败时返回错误。
    pub async fn write_fts(&mut self, event: &AuthorityEvent, body: &str) -> Result<(), JobError> {
        if !self.authority_event_belongs_to_scope(event).await? {
            return Err(JobError::ReferenceScopeMismatch("authority_event"));
        }
        self.charge_io()?;
        sqlx::query("DELETE FROM fragment_fts WHERE fragment_id = ?1 AND scope_key = ?2")
            .bind(&event.fragment_id)
            .bind(&self.scope_key)
            .execute(&mut **self.transaction)
            .await?;
        self.charge_io()?;
        sqlx::query("INSERT INTO fragment_fts (fragment_id, scope_key, body) VALUES (?1, ?2, ?3)")
            .bind(&event.fragment_id)
            .bind(&self.scope_key)
            .bind(body)
            .execute(&mut **self.transaction)
            .await?;
        Ok(())
    }

    async fn authority_event_belongs_to_scope(
        &mut self,
        event: &AuthorityEvent,
    ) -> Result<bool, JobError> {
        self.charge_io()?;
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM events
             WHERE id = ?1 AND fragment_id = ?2 AND scope_key = ?3",
        )
        .bind(&event.event_id)
        .bind(&event.fragment_id)
        .bind(&self.scope_key)
        .fetch_one(&mut **self.transaction)
        .await?;
        Ok(count == 1)
    }

    /// 应用一个绑定 Scope、白名单化且自动计量 IO 的派生变更。
    ///
    /// # Errors
    ///
    /// 权威表、跨 Scope、取消、IO 超限或数据库失败时返回错误。
    pub async fn apply(&mut self, mutation: DerivedMutation) -> Result<(), JobError> {
        if self.cancellation.is_cancelled() {
            return Err(JobError::Cancelled);
        }
        let Some(statement) = mutation.table.delete_scope_sql() else {
            return Err(JobError::AuthoritativeMutationDenied(mutation.table.name()));
        };
        if mutation.scope != self.checkpoint.scope {
            return Err(JobError::MutationScopeMismatch);
        }
        self.charge_io()?;
        sqlx::query(statement)
            .bind(&self.scope_key)
            .execute(&mut **self.transaction)
            .await?;
        Ok(())
    }

    const fn usage(&self, cpu_ms: u64) -> BatchUsage {
        BatchUsage {
            processed_items: self.processed_items,
            cpu_ms,
            io_ops: self.io_ops,
        }
    }
}

fn decode_authority_event(row: &sqlx::sqlite::SqliteRow) -> Result<AuthorityEvent, JobError> {
    let sequence = u64::try_from(row.try_get::<i64, _>("sequence")?)
        .map_err(|_| JobError::Corrupt("negative event sequence".into()))?;
    let payload: Value = serde_json::from_str(&row.try_get::<String, _>("payload_json")?)?;
    let source: Value = serde_json::from_str(&row.try_get::<String, _>("source_json")?)?;
    let source_id = source
        .get("source_id")
        .and_then(Value::as_str)
        .ok_or_else(|| JobError::Corrupt("fragment source_id is missing".into()))?
        .to_owned();
    Ok(AuthorityEvent {
        sequence,
        event_id: row.try_get("event_id")?,
        fragment_id: row.try_get("fragment_id")?,
        fragment_kind: row.try_get("kind")?,
        source_id,
        content_hash: row.try_get("content_hash")?,
        content_id: row.try_get("content_id")?,
        payload,
        recorded_at_us: row.try_get("recorded_at_us")?,
        confidence: row.try_get("confidence")?,
        validity: row.try_get("validity")?,
    })
}

/// 一个具体巩固 Handler 必须实现的批处理边界。
#[async_trait]
pub trait ConsolidationTask: Send + Sync {
    /// 通过受限上下文处理一个批次，但不得自行推进持久化 Checkpoint。
    async fn process_batch(&self, context: &mut BatchContext<'_>)
    -> Result<BatchOutcome, JobError>;
}

/// 使用持久化 Lease 驱动单个 Job 的 Runner。
#[derive(Clone)]
pub struct JobRunner<Task> {
    store: ConsolidationJobStore,
    task: Arc<Task>,
    worker_id: String,
    lease_duration: Duration,
    retry_base: Duration,
    cancellation: CancellationToken,
}

impl<Task> JobRunner<Task>
where
    Task: ConsolidationTask + 'static,
{
    /// 创建 Runner；默认使用新的未取消 Token。
    #[must_use]
    pub fn new(
        store: ConsolidationJobStore,
        task: Arc<Task>,
        worker_id: impl Into<String>,
        lease_duration: Duration,
        retry_base: Duration,
    ) -> Self {
        Self {
            store,
            task,
            worker_id: worker_id.into(),
            lease_duration,
            retry_base,
            cancellation: CancellationToken::new(),
        }
    }

    /// 替换 Runner 使用的协作取消 Token。
    #[must_use]
    pub fn with_cancellation(mut self, cancellation: CancellationToken) -> Self {
        self.cancellation = cancellation;
        self
    }

    async fn process_one_batch(
        &self,
        transaction: &mut Transaction<'static, Sqlite>,
        record: &JobRecord,
        deadline: Instant,
        timeout_budget: &'static str,
    ) -> Result<(BatchAttempt, BatchUsage), JobError> {
        let batch_started = Instant::now();
        let mut context = BatchContext::new(
            transaction,
            &record.checkpoint,
            record.spec.limits,
            &self.cancellation,
        )?;
        let process = timeout_at(deadline, self.task.process_batch(&mut context));
        let attempt = tokio::select! {
            () = self.cancellation.cancelled() => BatchAttempt::Cancelled,
            result = process => match result {
                Ok(result) => BatchAttempt::Finished(result),
                Err(_) => BatchAttempt::TimedOut(timeout_budget),
            },
        };
        let usage = context.usage(elapsed_ms_ceil(batch_started.elapsed()));
        Ok((attempt, usage))
    }

    /// 从最近提交的 Checkpoint 运行到完成、暂停、取消或失败。
    ///
    /// # Errors
    ///
    /// Lease 不可用、资源超限、取消或 Handler 失败时返回错误。
    pub async fn run(&self, id: JobId) -> Result<JobRecord, JobError> {
        if self.cancellation.is_cancelled() {
            self.store.cancel(id).await?;
            return Err(JobError::Cancelled);
        }
        let claimed = self
            .store
            .claim(id, &self.worker_id, self.lease_duration)
            .await?;
        let started = Instant::now();
        let wall_deadline = started + Duration::from_millis(claimed.spec.limits.max_runtime_ms);
        loop {
            if self.cancellation.is_cancelled() {
                self.store.cancel(id).await?;
                return Err(JobError::Cancelled);
            }
            let mut transaction = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
            let record = load_for_update(&mut transaction, id).await?;
            match record.status {
                JobStatus::Running => {}
                JobStatus::Paused => return Err(JobError::Paused),
                JobStatus::Cancelled => return Err(JobError::Cancelled),
                status => {
                    return Err(JobError::InvalidState {
                        status,
                        operation: "run",
                    });
                }
            }
            verify_worker_in_write_transaction(&record, &self.worker_id)?;

            let remaining_cpu_ms = record
                .spec
                .limits
                .max_cpu_ms
                .saturating_sub(record.checkpoint.cpu_ms);
            if remaining_cpu_ms == 0 {
                transaction.rollback().await?;
                self.store
                    .record_failure(id, &self.worker_id, self.retry_base)
                    .await?;
                return Err(JobError::BudgetExceeded("cpu"));
            }
            let batch_started = Instant::now();
            let cpu_deadline = batch_started + Duration::from_millis(remaining_cpu_ms);
            let (batch_deadline, timeout_budget) = if cpu_deadline <= wall_deadline {
                (cpu_deadline, "cpu")
            } else {
                (wall_deadline, "wall_clock")
            };
            let (attempt, usage) = self
                .process_one_batch(&mut transaction, &record, batch_deadline, timeout_budget)
                .await?;
            let outcome = match attempt {
                BatchAttempt::Cancelled => {
                    transaction.rollback().await?;
                    self.store.cancel(id).await?;
                    return Err(JobError::Cancelled);
                }
                BatchAttempt::TimedOut(budget) => {
                    transaction.rollback().await?;
                    self.store
                        .record_failure(id, &self.worker_id, self.retry_base)
                        .await?;
                    return Err(JobError::BudgetExceeded(budget));
                }
                BatchAttempt::Finished(Ok(outcome)) => outcome,
                BatchAttempt::Finished(Err(error)) => {
                    transaction.rollback().await?;
                    self.store
                        .record_failure(id, &self.worker_id, self.retry_base)
                        .await?;
                    return Err(error);
                }
            };
            if let Err(error) = record
                .checkpoint
                .advanced(outcome, usage, &record.spec.limits)
            {
                transaction.rollback().await?;
                self.store
                    .record_failure(id, &self.worker_id, self.retry_base)
                    .await?;
                return Err(error);
            }
            let record = stage_committed_batch(
                &mut transaction,
                record,
                &self.worker_id,
                outcome,
                usage,
                self.lease_duration,
            )
            .await?;
            transaction.commit().await?;
            if record.status == JobStatus::Completed {
                return Ok(record);
            }
            tokio::task::yield_now().await;
        }
    }
}

fn elapsed_ms_ceil(elapsed: Duration) -> u64 {
    let micros = elapsed.as_micros();
    let rounded = micros.saturating_add(999) / 1_000;
    u64::try_from(rounded).unwrap_or(u64::MAX)
}
