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
    /// `SQLite` reported a database consistency problem.
    #[error("SQLite {check} failed: {details}")]
    IntegrityCheck {
        /// Name of the failed `SQLite` check.
        check: &'static str,
        /// Diagnostic rows returned by `SQLite`.
        details: String,
    },
}
