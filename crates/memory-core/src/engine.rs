use std::{sync::Arc, time::Instant};

use jc_memory_events::factory::{EventFactory, EventFactoryError};
use jc_memory_protocol::{BlobId, CognitiveFragment, EventId, FragmentId, ValidityStatus};
use jc_memory_storage::{
    BlobError, BlobOptions, BlobStore, MemoryRepository, RecoveryError, RecoveryManager,
    RepositoryFailpoint, SqliteConfig, SqliteMemoryRepository, SqliteStore, StorageError,
};
use sha2::{Digest as _, Sha256};
use tracing::Instrument as _;

use crate::{HealthReport, HealthStatus, MemoryConfig};

/// Result of persisting one authoritative cognitive fragment and event.
#[derive(Clone, Debug, Eq, PartialEq)]
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
    /// Whether the fragment was explicitly submitted with quarantined validity.
    pub quarantined: bool,
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
            source => Self::Storage(source),
        }
    }
}

/// High-level embedded API for durable Jiangcheng Memory operations.
#[derive(Clone)]
pub struct MemoryEngine {
    repository: Arc<dyn MemoryRepository>,
    recovery: Arc<RecoveryManager>,
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
        let recovery = Arc::new(RecoveryManager::from_stores(&store, blobs));
        Ok(Self {
            repository,
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
        fragment: CognitiveFragment,
    ) -> Result<ObserveReceipt, MemoryError> {
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
        let result = self.observe_inner(fragment).instrument(span.clone()).await;
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

    /// Reads one authoritative fragment by identifier.
    ///
    /// # Errors
    ///
    /// Returns [`MemoryError::NotFound`] when the fragment does not exist, or another storage
    /// error when its durable rows cannot be decoded.
    pub async fn fragment(&self, id: FragmentId) -> Result<CognitiveFragment, MemoryError> {
        Ok(self.repository.get_fragment(id).await?)
    }

    /// Audits current authoritative storage and derived-index health.
    ///
    /// # Errors
    ///
    /// Returns [`MemoryError`] when the integrity audit cannot complete.
    pub async fn health(&self) -> Result<HealthReport, MemoryError> {
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

    async fn observe_inner(
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
        })
    }
}

fn short_hash(value: &str) -> String {
    let digest = format!("{:x}", Sha256::digest(value.as_bytes()));
    digest[..12].to_owned()
}

fn elapsed_millis(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}
