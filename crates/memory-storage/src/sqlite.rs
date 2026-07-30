use sqlx::{
    SqlitePool,
    migrate::Migrator,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};

use crate::{SqliteConfig, StorageError};

// 编译期嵌入全部版本化 Schema，当前包含治理审计扩展 v3。
static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

/// SQLite-backed storage connection pool with the Jiangcheng Memory schema.
#[derive(Debug)]
pub struct SqliteStore {
    pool: SqlitePool,
}

impl SqliteStore {
    /// Opens the configured database, applies connection pragmas, and runs migrations.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] when the configuration is invalid, the database cannot be
    /// opened, or a schema migration fails.
    pub async fn open(config: &SqliteConfig) -> Result<Self, StorageError> {
        if config.max_connections() == 0 {
            return Err(StorageError::InvalidConfiguration {
                reason: "max_connections must be greater than zero",
            });
        }

        let connect_options = SqliteConnectOptions::new()
            .filename(config.path())
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .busy_timeout(config.busy_timeout());
        let pool = SqlitePoolOptions::new()
            .max_connections(config.effective_max_connections())
            .connect_with(connect_options)
            .await?;

        if let Err(error) = MIGRATOR.run(&pool).await {
            pool.close().await;
            return Err(error.into());
        }

        Ok(Self { pool })
    }

    /// Returns the underlying `SQLx` connection pool.
    #[must_use]
    pub const fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// Returns the highest successfully applied schema migration version.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] when migration metadata cannot be queried.
    pub async fn schema_version(&self) -> Result<i64, StorageError> {
        let version = sqlx::query_scalar(
            "SELECT COALESCE(MAX(version), 0) FROM _sqlx_migrations WHERE success = 1",
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(version)
    }

    /// Runs `SQLite`'s fast and exhaustive database consistency checks.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] when a check cannot be executed or reports corruption.
    pub async fn integrity_check(&self) -> Result<(), StorageError> {
        run_check(&self.pool, "quick_check", "PRAGMA quick_check").await?;
        run_check(&self.pool, "integrity_check", "PRAGMA integrity_check").await
    }
}

async fn run_check(
    pool: &SqlitePool,
    check: &'static str,
    statement: &str,
) -> Result<(), StorageError> {
    let results: Vec<String> = sqlx::query_scalar(statement).fetch_all(pool).await?;
    if results.len() == 1 && results[0].eq_ignore_ascii_case("ok") {
        return Ok(());
    }

    Err(StorageError::IntegrityCheck {
        check,
        details: results.join("; "),
    })
}
