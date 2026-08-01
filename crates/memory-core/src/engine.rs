use std::{sync::Arc, time::Instant};

use jc_memory_activation::RecallEngineError;
use jc_memory_events::factory::{EventFactory, EventFactoryError};
use jc_memory_protocol::{
    AccessContext, BlobId, CognitiveFragment, DenyReason, EventId, FragmentId, Operation,
    ValidityStatus,
};
use jc_memory_security::{Admission, RiskAssessment, SecurityInvariantViolation};
use jc_memory_storage::{
    BlobError, BlobOptions, BlobStore, MemoryRepository, RecoveryError, RecoveryManager,
    RepositoryFailpoint, SqliteConfig, SqliteMemoryRepository, SqliteProjectionStore, SqliteStore,
    StorageError,
};
use sha2::{Digest as _, Sha256};
use tracing::Instrument as _;

use crate::{HealthReport, HealthStatus, MemoryConfig};

/// Result of persisting one authoritative cognitive fragment and event.
#[derive(Clone, Debug, PartialEq)]
pub struct ObserveReceipt {
    /// Persisted fragment identifier.
    pub fragment_id: FragmentId,
    /// Persisted immutable event identifier.
    pub event_id: EventId,
    /// Monotonic sequence within the exact memory scope.
    pub sequence: u64,
    /// Canonical SHA-256 of the complete fragment payload.
    pub content_hash: BlobId,
    /// Whether this call created the authoritative event.
    pub created: bool,
    /// 检测和准入规则执行后，持久化 Fragment 是否处于 Quarantined 状态。
    pub quarantined: bool,
    /// 确定性检测器对原始 Fragment 的风险评估。
    pub risk_assessment: RiskAssessment,
}

/// Errors exposed by the embedded memory facade.
#[derive(Debug, thiserror::Error)]
pub enum MemoryError {
    /// The local data directory could not be prepared.
    #[error("failed to prepare the local memory directory: {0}")]
    DataDirectory(#[source] std::io::Error),
    /// The cognitive fragment could not form a valid immutable event.
    #[error("failed to construct memory event: {0}")]
    Event(#[from] EventFactoryError),
    /// The content-addressed blob store could not be opened or accessed.
    #[error("memory blob operation failed: {0}")]
    Blob(#[from] BlobError),
    /// A requested durable entity does not exist.
    #[error("{entity} {id} was not found")]
    NotFound {
        /// Logical entity type.
        entity: &'static str,
        /// Missing durable identifier.
        id: String,
    },
    /// `SQLite` remained busy or locked after bounded retries.
    #[error("memory database remained busy after bounded retries: {source}")]
    Busy {
        /// Original storage failure with the `SQLite` diagnostic chain.
        #[source]
        source: StorageError,
    },
    /// An immutable identifier already belongs to different content.
    #[error("{entity} {id} already exists with different content")]
    Conflict {
        /// Logical entity type whose identifier conflicts.
        entity: &'static str,
        /// Conflicting immutable identifier.
        id: String,
    },
    /// `SQLite` rejected a relational or uniqueness constraint.
    #[error("memory database constraint failed: {source}")]
    Constraint {
        /// Original `SQLite` constraint diagnostic.
        #[source]
        source: StorageError,
    },
    /// Authoritative durable rows could not be decoded safely.
    #[error("memory contains corrupt authoritative data: {source}")]
    Corrupt {
        /// Original structured corrupt-row diagnostic.
        #[source]
        source: StorageError,
    },
    /// The storage layer rejected or could not complete an operation.
    #[error("memory storage operation failed: {0}")]
    Storage(#[source] StorageError),
    /// Integrity audit or recovery failed.
    #[error("memory recovery operation failed: {0}")]
    Recovery(#[from] RecoveryError),
    /// 有界 Recall 流水线拒绝了派生图输入或被取消、超时。
    #[error("memory recall failed: {0}")]
    Recall(#[from] RecallEngineError),
    /// State 或 Graph 投影未能在 Recall deadline 内追平。
    #[error("memory projection timed out")]
    ProjectionTimedOut,
    /// 持久化投影 Checkpoint 无法安全解码。
    #[error("memory projection checkpoint is corrupt")]
    ProjectionCheckpointCorrupt {
        /// 不向公开正文暴露 Scope 细节的底层错误。
        #[source]
        source: StorageError,
    },
    /// 投影行或权威来源越过精确 Scope。
    #[error("memory projection scope mismatch")]
    ProjectionScopeMismatch {
        /// 不向公开正文暴露 Scope 细节的底层错误。
        #[source]
        source: StorageError,
    },
    /// `StateVersion` 链不满足唯一当前版本等不变量。
    #[error("memory state chain is corrupt")]
    StateChainCorrupt {
        /// 不向公开正文暴露状态内容的底层错误。
        #[source]
        source: StorageError,
    },
    /// 显式 State Mutation 的目标不存在可投影 State 或 key 不一致。
    #[error("invalid state mutation: {reason}")]
    InvalidStateMutation {
        /// 不包含 State value 或权威正文的稳定拒绝原因。
        reason: &'static str,
    },
    /// Memory Packet 无法渲染为模型无关 JSON 上下文。
    #[error("memory packet serialization failed: {0}")]
    Packet(#[from] serde_json::Error),
    /// 强类型标识之间的确定性 UUID 转换失败。
    #[error("memory identifier conversion failed: {0}")]
    Identifier(#[from] jc_memory_protocol::IdParseError),
    /// 派生关系的 Point 端点违反图不变量。
    #[error("memory edge creation failed: {0}")]
    Edge(#[from] jc_memory_graph::EdgeCreationError),
    /// 调用者缺少目标操作和 Scope 的显式 Grant。
    #[error("memory access denied for {operation:?}: {reason:?}")]
    AccessDenied {
        /// 被拒绝的核心操作。
        operation: Operation,
        /// 稳定的默认拒绝原因。
        reason: DenyReason,
    },
    /// 返回数据违反了独立的 Scope 或来源不变量。
    #[error("memory output security invariant failed: {0}")]
    SecurityInvariantViolation(#[source] SecurityInvariantViolation),
    /// Quarantine 审查目标不是仍处于隔离状态的权威 Fragment。
    #[error("fragment {fragment_id} is not quarantined")]
    ReviewTargetNotQuarantined {
        /// 无法审查的目标 Fragment。
        fragment_id: FragmentId,
    },
    /// Quarantine 审查理由为空。
    #[error("quarantine review reason cannot be empty")]
    EmptyReviewReason,
}

impl From<StorageError> for MemoryError {
    fn from(error: StorageError) -> Self {
        if error.is_busy_or_locked() {
            return Self::Busy { source: error };
        }
        if error.is_constraint() {
            return Self::Constraint { source: error };
        }
        match error {
            StorageError::NotFound { entity, id } => Self::NotFound { entity, id },
            StorageError::IdentityConflict { entity, id } => Self::Conflict { entity, id },
            source @ StorageError::CorruptRow { .. } => Self::Corrupt { source },
            source @ StorageError::ProjectionCheckpointCorrupt { .. } => {
                Self::ProjectionCheckpointCorrupt { source }
            }
            source @ StorageError::ProjectionScopeMismatch { .. } => {
                Self::ProjectionScopeMismatch { source }
            }
            source @ StorageError::StateChainCorrupt { .. } => Self::StateChainCorrupt { source },
            source => Self::Storage(source),
        }
    }
}

/// High-level embedded API for durable Jiangcheng Memory operations.
#[derive(Clone)]
pub struct MemoryEngine {
    pub(crate) repository: Arc<dyn MemoryRepository>,
    pub(crate) projection: SqliteProjectionStore,
    pub(crate) recovery: Arc<RecoveryManager>,
    event_factory: EventFactory,
    schema_version: i64,
}

impl MemoryEngine {
    /// Opens or creates a local embedded memory engine.
    ///
    /// # Errors
    ///
    /// Returns [`MemoryError`] when the data directory, database, migrations, or blob store cannot
    /// be opened.
    pub async fn open(config: MemoryConfig) -> Result<Self, MemoryError> {
        Self::open_inner(config, None).await
    }

    /// 显式关闭底层共享连接池。
    pub async fn close(&self) {
        self.repository.close().await;
    }

    /// Opens a local engine with a deterministic repository failure hook.
    ///
    /// This constructor exists for crash-recovery acceptance tests and fault-injection harnesses.
    ///
    /// # Errors
    ///
    /// Returns the same failures as [`Self::open`].
    pub async fn open_with_repository_failpoint(
        config: MemoryConfig,
        failpoint: Arc<dyn RepositoryFailpoint>,
    ) -> Result<Self, MemoryError> {
        Self::open_inner(config, Some(failpoint)).await
    }

    async fn open_inner(
        config: MemoryConfig,
        failpoint: Option<Arc<dyn RepositoryFailpoint>>,
    ) -> Result<Self, MemoryError> {
        tokio::fs::create_dir_all(config.data_dir())
            .await
            .map_err(MemoryError::DataDirectory)?;
        let sqlite_config =
            SqliteConfig::new(config.database_path()).with_busy_timeout(config.busy_timeout());
        let store = SqliteStore::open(&sqlite_config).await?;
        let schema_version = store.schema_version().await?;
        let blobs = BlobStore::open(config.blob_root(), BlobOptions::plain()).await?;
        let mut repository = SqliteMemoryRepository::from_store(&store);
        if let Some(failpoint) = failpoint {
            repository = repository.with_failpoint(failpoint);
        }
        let repository: Arc<dyn MemoryRepository> = Arc::new(repository);
        let projection = SqliteProjectionStore::from_store(&store);
        let recovery = Arc::new(RecoveryManager::from_stores(&store, blobs));
        Ok(Self {
            repository,
            projection,
            recovery,
            event_factory: EventFactory::new(),
            schema_version,
        })
    }

    /// Validates and transactionally persists one cognitive fragment as an immutable event.
    ///
    /// # Errors
    ///
    /// Returns [`MemoryError`] when fragment validation, event construction, or transactional
    /// persistence fails.
    pub async fn observe(
        &self,
        context: &AccessContext,
        fragment: CognitiveFragment,
    ) -> Result<ObserveReceipt, MemoryError> {
        crate::security::authorize(context, Operation::Observe, &fragment.scope)?;
        let fragment_id = fragment.id;
        let namespace_hash = short_hash(&fragment.scope.namespace_id.to_string());
        let tenant_hash = fragment.scope.tenant_id.map_or_else(
            || "none".to_owned(),
            |tenant_id| short_hash(&tenant_id.to_string()),
        );
        let span = tracing::info_span!(
            "memory.observe",
            event_id = tracing::field::Empty,
            fragment_id = %fragment_id,
            namespace_hash = %namespace_hash,
            tenant_hash = %tenant_hash,
            elapsed_ms = tracing::field::Empty,
            result = tracing::field::Empty,
        );
        let started = Instant::now();
        let result = self
            .observe_admitted(fragment)
            .instrument(span.clone())
            .await;
        span.record("elapsed_ms", elapsed_millis(started));
        match &result {
            Ok(receipt) => {
                span.record("event_id", tracing::field::display(receipt.event_id));
                span.record("result", "ok");
            }
            Err(_) => {
                span.record("result", "error");
            }
        }
        result
    }

    /// 追加一次 Runtime Commit 产生的完整 Fragment 集合。
    ///
    /// Commit 使用 `Mutate` Grant 进入核心，并复用同一准入和权威追加路径；
    /// 每个 Fragment 仍保持独立的原始 Kind，不会被提升为 Verified Fact。
    ///
    /// # Errors
    ///
    /// 任一 Fragment 无法通过准入、事件校验或持久化时返回错误。
    pub async fn commit(
        &self,
        context: &AccessContext,
        fragments: Vec<CognitiveFragment>,
    ) -> Result<Vec<ObserveReceipt>, MemoryError> {
        crate::security::authorize(context, Operation::Mutate, &context.scope)?;
        if fragments
            .iter()
            .any(|fragment| fragment.scope != context.scope)
        {
            return Err(MemoryError::AccessDenied {
                operation: Operation::Mutate,
                reason: DenyReason::SubjectScopeMismatch,
            });
        }
        let mut admitted = Vec::with_capacity(fragments.len());
        for mut fragment in fragments {
            jc_memory_events::validation::validate_fragment(&fragment)
                .map_err(EventFactoryError::from)?;
            let risk_context = self.risk_context(&fragment).await?;
            let risk_assessment =
                jc_memory_security::QuarantinePipeline.assess(&fragment, risk_context);
            let admission =
                jc_memory_security::QuarantinePipeline.admission(&fragment, &risk_assessment);
            match admission {
                Admission::Accept => {}
                Admission::AcceptUnverified => fragment.validity = ValidityStatus::Unverified,
                Admission::Quarantine { .. } => fragment.validity = ValidityStatus::Quarantined,
            }
            let governance_fragment = fragment.clone();
            let draft = self.event_factory.from_fragment(fragment)?;
            admitted.push((draft, governance_fragment, risk_assessment, admission));
        }
        let drafts = admitted
            .iter()
            .map(|(draft, _, _, _)| draft.clone())
            .collect::<Vec<_>>();
        let persisted = self.repository.append_batch(&drafts).await?;
        let mut receipts = Vec::with_capacity(persisted.len());
        for ((_, governance_fragment, risk_assessment, admission), receipt) in
            admitted.into_iter().zip(persisted)
        {
            let result = ObserveReceipt {
                fragment_id: receipt.event.draft.fragment_id,
                event_id: receipt.event.draft.id,
                sequence: receipt.event.sequence,
                content_hash: receipt.event.draft.content_hash,
                created: receipt.created,
                quarantined: receipt.event.draft.payload.validity == ValidityStatus::Quarantined,
                risk_assessment: risk_assessment.clone(),
            };
            if result.created && matches!(admission, Admission::Quarantine { .. }) {
                self.append_automatic_quarantine(&governance_fragment, &risk_assessment, admission)
                    .await?;
            }
            receipts.push(result);
        }
        Ok(receipts)
    }

    pub(crate) async fn observe_admitted(
        &self,
        mut fragment: CognitiveFragment,
    ) -> Result<ObserveReceipt, MemoryError> {
        jc_memory_events::validation::validate_fragment(&fragment)
            .map_err(EventFactoryError::from)?;
        let risk_context = self.risk_context(&fragment).await?;
        let risk_assessment =
            jc_memory_security::QuarantinePipeline.assess(&fragment, risk_context);
        let admission =
            jc_memory_security::QuarantinePipeline.admission(&fragment, &risk_assessment);
        match admission {
            Admission::Accept => {}
            Admission::AcceptUnverified => fragment.validity = ValidityStatus::Unverified,
            Admission::Quarantine { .. } => fragment.validity = ValidityStatus::Quarantined,
        }
        let governance_fragment = fragment.clone();
        let mut receipt = self.observe_inner(fragment).await?;
        receipt.risk_assessment.clone_from(&risk_assessment);
        if receipt.created && matches!(admission, Admission::Quarantine { .. }) {
            self.append_automatic_quarantine(&governance_fragment, &risk_assessment, admission)
                .await?;
        }
        Ok(receipt)
    }

    /// Reads one authoritative fragment by identifier.
    ///
    /// # Errors
    ///
    /// Returns [`MemoryError::NotFound`] when the fragment does not exist, or another storage
    /// error when its durable rows cannot be decoded.
    pub async fn fragment(
        &self,
        context: &AccessContext,
        id: FragmentId,
    ) -> Result<CognitiveFragment, MemoryError> {
        crate::security::authorize(context, Operation::Recall, &context.scope)?;
        Ok(self
            .repository
            .get_fragment_in_scope(&context.scope, id)
            .await?)
    }

    /// Audits current authoritative storage and derived-index health.
    ///
    /// # Errors
    ///
    /// Returns [`MemoryError`] when the integrity audit cannot complete.
    pub async fn health(&self, context: &AccessContext) -> Result<HealthReport, MemoryError> {
        crate::security::authorize(context, Operation::Admin, &context.scope)?;
        let report = self.recovery.audit().await?;
        let blob_store_ok = report.missing_blobs.is_empty() && report.corrupt_blobs.is_empty();
        let status = if !report.database_ok || !blob_store_ok {
            HealthStatus::Unhealthy
        } else if !report.orphan_blobs.is_empty() || report.fts_mismatches > 0 {
            HealthStatus::Degraded
        } else {
            HealthStatus::Healthy
        };
        Ok(HealthReport {
            status,
            database_ok: report.database_ok,
            blob_store_ok,
            schema_version: self.schema_version,
        })
    }

    pub(crate) async fn observe_inner(
        &self,
        fragment: CognitiveFragment,
    ) -> Result<ObserveReceipt, MemoryError> {
        let quarantined = fragment.validity == ValidityStatus::Quarantined;
        let draft = self.event_factory.from_fragment(fragment)?;
        let receipt = self.repository.append(&draft).await?;
        Ok(ObserveReceipt {
            fragment_id: receipt.event.draft.fragment_id,
            event_id: receipt.event.draft.id,
            sequence: receipt.event.sequence,
            content_hash: receipt.event.draft.content_hash,
            created: receipt.created,
            quarantined,
            risk_assessment: RiskAssessment::default(),
        })
    }
}

pub(crate) fn short_hash(value: &str) -> String {
    let digest = hex::encode(Sha256::digest(value.as_bytes()));
    digest[..12].to_owned()
}

fn elapsed_millis(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}
