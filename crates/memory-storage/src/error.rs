/// Errors produced by the storage layer.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    /// The supplied storage configuration is invalid.
    #[error("invalid SQLite configuration: {reason}")]
    InvalidConfiguration {
        /// Human-readable reason the configuration was rejected.
        reason: &'static str,
    },
    /// A `SQLite` operation failed.
    #[error("SQLite operation failed: {0}")]
    Database(#[from] sqlx::Error),
    /// A schema migration failed.
    #[error("SQLite migration failed: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),
    /// A cognitive fragment violates protocol invariants.
    #[error("invalid cognitive fragment: {0}")]
    FragmentValidation(#[from] jc_memory_events::validation::FragmentValidationError),
    /// Domain data could not be serialized for durable storage.
    #[error("storage serialization failed: {0}")]
    Serialization(#[from] serde_json::Error),
    /// An immutable identifier already belongs to different content.
    #[error("{entity} {id} already exists with different content")]
    IdentityConflict {
        /// Logical entity type whose identifier conflicts.
        entity: &'static str,
        /// Conflicting immutable identifier.
        id: String,
    },
    /// A requested durable entity does not exist.
    #[error("{entity} {id} was not found")]
    NotFound {
        /// Logical entity type that was queried.
        entity: &'static str,
        /// Missing immutable identifier.
        id: String,
    },
    /// An event draft is internally inconsistent before persistence.
    #[error("invalid event draft: {reason}")]
    InvalidDraft {
        /// Static explanation of the violated envelope invariant.
        reason: &'static str,
    },
    /// A per-scope event sequence cannot be represented by the protocol.
    #[error("event sequence overflow for scope {scope_key}")]
    SequenceOverflow {
        /// Canonical exact-scope digest.
        scope_key: String,
    },
    /// A durable row cannot be decoded without inventing fallback data.
    #[error("corrupt row {table}/{id}: {reason}")]
    CorruptRow {
        /// Authoritative table containing the invalid row.
        table: &'static str,
        /// Primary identifier of the invalid row.
        id: String,
        /// Precise decoding or invariant failure.
        reason: String,
    },
    /// `SQLite` reported a database consistency problem.
    #[error("SQLite {check} failed: {details}")]
    IntegrityCheck {
        /// Name of the failed `SQLite` check.
        check: &'static str,
        /// Diagnostic rows returned by `SQLite`.
        details: String,
    },
    /// A deterministic test failpoint interrupted a storage transaction.
    #[error("storage failpoint injected at {point}")]
    InjectedFailure {
        /// Stable failpoint identifier.
        point: &'static str,
    },
}

impl StorageError {
    /// Returns whether the failure is `SQLite` Busy or Locked, including extended result codes.
    #[must_use]
    pub fn is_busy_or_locked(&self) -> bool {
        matches!(sqlite_primary_code(self), Some(5 | 6))
    }

    /// Returns whether the failure is an `SQLite` constraint violation.
    #[must_use]
    pub fn is_constraint(&self) -> bool {
        matches!(sqlite_primary_code(self), Some(19))
    }
}

fn sqlite_primary_code(error: &StorageError) -> Option<i32> {
    let StorageError::Database(sqlx::Error::Database(error)) = error else {
        return None;
    };
    error.code()?.parse::<i32>().ok().map(|code| code & 0xff)
}
