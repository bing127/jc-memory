use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{File, OpenOptions},
    path::{Path, PathBuf},
    str::FromStr as _,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use fs2::FileExt;
use jc_memory_protocol::BlobId;
use sqlx::{Row as _, SqlitePool};

use crate::{BlobError, BlobStore, SqliteStore};

const FTS_REBUILD_TABLE: &str = "fragment_fts_rebuild";

/// Result of a read-only storage integrity audit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IntegrityReport {
    /// Whether `SQLite`'s quick consistency check passed.
    pub database_ok: bool,
    /// Number of unique referenced or on-disk blob identifiers inspected.
    pub checked_blobs: u64,
    /// Referenced blobs whose files are absent.
    pub missing_blobs: Vec<BlobId>,
    /// On-disk blobs that fail envelope or checksum verification.
    pub corrupt_blobs: Vec<BlobId>,
    /// On-disk blobs with no positive database reference count.
    pub orphan_blobs: Vec<BlobId>,
    /// Number of expected and actual FTS rows that do not match.
    pub fts_mismatches: u64,
}

/// Garbage-collection policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GcPolicy {
    grace_period: Duration,
    delete: bool,
}

impl GcPolicy {
    /// Creates an explicit deletion policy with the supplied minimum file age.
    #[must_use]
    pub const fn delete_after(grace_period: Duration) -> Self {
        Self {
            grace_period,
            delete: true,
        }
    }
}

impl Default for GcPolicy {
    fn default() -> Self {
        Self {
            grace_period: Duration::ZERO,
            delete: false,
        }
    }
}

/// Result of one garbage-collection pass.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GcReport {
    /// Unreferenced blobs old enough to delete under the supplied policy.
    pub candidates: Vec<BlobId>,
    /// Candidate blobs atomically moved to trash and deleted.
    pub removed: Vec<BlobId>,
    /// Blobs retained because they are referenced, too young, or the pass was a dry run.
    pub retained: Vec<BlobId>,
}

/// Result of rebuilding the derived FTS5 index.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RebuildReport {
    /// Number of authoritative inline fragments inserted into the rebuilt index.
    pub indexed_fragments: u64,
    /// Number of rows present in the old index before replacement.
    pub replaced_rows: u64,
}

/// Injectable recovery boundary used to prove transaction rollback behavior.
pub trait RecoveryFailpoint: Send + Sync {
    /// Runs after the replacement index is complete but before the authoritative FTS swap.
    ///
    /// # Errors
    ///
    /// Returning an error aborts and rolls back the rebuild transaction.
    fn before_fts_swap(&self) -> Result<(), RecoveryError>;
}

#[derive(Debug)]
struct NoRecoveryFailpoint;

impl RecoveryFailpoint for NoRecoveryFailpoint {
    fn before_fts_swap(&self) -> Result<(), RecoveryError> {
        Ok(())
    }
}

/// Failures produced by integrity audit and recovery operations.
#[derive(Debug, thiserror::Error)]
pub enum RecoveryError {
    /// A `SQLite` operation failed.
    #[error("SQLite recovery operation failed: {0}")]
    Database(#[from] sqlx::Error),
    /// A blob operation failed.
    #[error("blob recovery operation failed: {0}")]
    Blob(#[from] BlobError),
    /// A filesystem operation failed.
    #[error("recovery filesystem operation failed: {0}")]
    Io(#[from] std::io::Error),
    /// Another process currently owns the garbage-collection lock.
    #[error("garbage collection lock is unavailable")]
    LockUnavailable,
    /// A durable database blob identifier is malformed.
    #[error("invalid blob identifier in durable storage: {value}")]
    InvalidBlobId {
        /// Malformed identifier read from the database.
        value: String,
    },
    /// A replacement FTS index contained an unexpected row count.
    #[error("FTS rebuild count mismatch: expected {expected}, indexed {indexed}")]
    FtsCountMismatch {
        /// Number of authoritative indexable fragments.
        expected: u64,
        /// Number of rows written to the temporary index.
        indexed: u64,
    },
    /// A test failpoint deliberately interrupted recovery.
    #[error("recovery failpoint injected at {point}")]
    InjectedFailure {
        /// Stable failpoint identifier.
        point: &'static str,
    },
}

/// Coordinates read-only audits, conservative blob GC, and derived-index recovery.
#[derive(Clone)]
pub struct RecoveryManager {
    pool: SqlitePool,
    blobs: BlobStore,
    failpoint: Arc<dyn RecoveryFailpoint>,
}

impl RecoveryManager {
    /// Creates a manager sharing an open `SQLite` store and blob store.
    #[must_use]
    pub fn from_stores(store: &SqliteStore, blobs: BlobStore) -> Self {
        Self {
            pool: store.pool().clone(),
            blobs,
            failpoint: Arc::new(NoRecoveryFailpoint),
        }
    }

    /// Replaces the recovery failpoint implementation.
    #[must_use]
    pub fn with_failpoint(mut self, failpoint: Arc<dyn RecoveryFailpoint>) -> Self {
        self.failpoint = failpoint;
        self
    }

    /// Audits database consistency, blob references, checksums, and FTS derivation.
    ///
    /// This method never deletes or rewrites authoritative evidence.
    ///
    /// # Errors
    ///
    /// Returns [`RecoveryError`] when the database, filesystem, or blob store cannot be read.
    pub async fn audit(&self) -> Result<IntegrityReport, RecoveryError> {
        let database_ok = quick_check(&self.pool).await?;
        let referenced = referenced_blobs(&self.pool).await?;
        let on_disk = enumerate_blobs(self.blobs.root()).await?;
        let all: BTreeSet<_> = referenced
            .iter()
            .cloned()
            .chain(on_disk.keys().cloned())
            .collect();

        let mut missing_blobs = Vec::new();
        let mut corrupt_blobs = Vec::new();
        let mut orphan_blobs = Vec::new();
        for id in &all {
            let is_referenced = referenced.contains(id);
            if !on_disk.contains_key(id) {
                if is_referenced {
                    missing_blobs.push(id.clone());
                }
                continue;
            }
            if self.blobs.verify(id).await.is_err() {
                corrupt_blobs.push(id.clone());
            }
            if !is_referenced {
                orphan_blobs.push(id.clone());
            }
        }

        Ok(IntegrityReport {
            database_ok,
            checked_blobs: usize_to_u64(all.len()),
            missing_blobs,
            corrupt_blobs,
            orphan_blobs,
            fts_mismatches: fts_mismatches(&self.pool).await?,
        })
    }

    /// Reports or explicitly removes unreferenced blobs after a grace period.
    ///
    /// [`GcPolicy::default`] is always a dry run. Deletion requires
    /// [`GcPolicy::delete_after`].
    ///
    /// # Errors
    ///
    /// Returns [`RecoveryError::LockUnavailable`] when another process holds the GC lock, or a
    /// database/filesystem error when candidates cannot be checked safely.
    pub async fn collect_garbage(&self, policy: GcPolicy) -> Result<GcReport, RecoveryError> {
        let lock = acquire_gc_lock(self.blobs.root())?;
        let referenced = referenced_blobs(&self.pool).await?;
        let on_disk = enumerate_blobs(self.blobs.root()).await?;
        let now = SystemTime::now();
        let mut candidates = Vec::new();
        let mut removed = Vec::new();
        let mut retained = Vec::new();

        for (id, path) in on_disk {
            if referenced.contains(&id) {
                retained.push(id);
                continue;
            }
            let modified = tokio::fs::metadata(&path).await?.modified()?;
            let age = now.duration_since(modified).unwrap_or(Duration::ZERO);
            if age < policy.grace_period {
                retained.push(id);
                continue;
            }

            candidates.push(id.clone());
            if policy.delete {
                move_to_trash_and_delete(self.blobs.root(), &path, &id).await?;
                removed.push(id);
            } else {
                retained.push(id);
            }
        }

        FileExt::unlock(&lock)?;
        Ok(GcReport {
            candidates,
            removed,
            retained,
        })
    }

    /// Rebuilds the derived FTS5 index from authoritative inline fragment content.
    ///
    /// # Errors
    ///
    /// Returns [`RecoveryError`] when index construction, validation, the failpoint, or the
    /// atomic replacement fails. Any failure rolls back and preserves the previous index.
    pub async fn rebuild_fts(&self) -> Result<RebuildReport, RecoveryError> {
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let result = async {
            sqlx::query(&format!("DROP TABLE IF EXISTS {FTS_REBUILD_TABLE}"))
                .execute(&mut *transaction)
                .await?;
            sqlx::query(&format!(
                "CREATE VIRTUAL TABLE {FTS_REBUILD_TABLE} USING fts5(
                    fragment_id UNINDEXED,
                    scope_key UNINDEXED,
                    body,
                    tokenize = 'unicode61 remove_diacritics 2'
                 )"
            ))
            .execute(&mut *transaction)
            .await?;
            sqlx::query(&format!(
                "INSERT INTO {FTS_REBUILD_TABLE} (fragment_id, scope_key, body)
                 SELECT f.id, f.scope_key, CAST(c.inline_value AS TEXT)
                 FROM fragments AS f
                 JOIN fragment_contents AS c ON c.id = f.content_id
                 WHERE c.storage_kind IN ('inline_text', 'inline_json')"
            ))
            .execute(&mut *transaction)
            .await?;

            let expected = count_indexable_fragments(&mut transaction).await?;
            let indexed = query_count(
                &mut transaction,
                &format!("SELECT COUNT(*) FROM {FTS_REBUILD_TABLE}"),
            )
            .await?;
            if expected != indexed {
                return Err(RecoveryError::FtsCountMismatch { expected, indexed });
            }
            let replaced_rows =
                query_count(&mut transaction, "SELECT COUNT(*) FROM fragment_fts").await?;
            self.failpoint.before_fts_swap()?;

            sqlx::query("DELETE FROM fragment_fts")
                .execute(&mut *transaction)
                .await?;
            sqlx::query(&format!(
                "INSERT INTO fragment_fts (fragment_id, scope_key, body)
                 SELECT fragment_id, scope_key, body FROM {FTS_REBUILD_TABLE}"
            ))
            .execute(&mut *transaction)
            .await?;
            sqlx::query(&format!("DROP TABLE {FTS_REBUILD_TABLE}"))
                .execute(&mut *transaction)
                .await?;
            Ok(RebuildReport {
                indexed_fragments: indexed,
                replaced_rows,
            })
        }
        .await;

        match result {
            Ok(report) => {
                transaction.commit().await?;
                Ok(report)
            }
            Err(error) => {
                transaction.rollback().await?;
                Err(error)
            }
        }
    }
}

async fn quick_check(pool: &SqlitePool) -> Result<bool, RecoveryError> {
    let rows: Vec<String> = sqlx::query_scalar("PRAGMA quick_check")
        .fetch_all(pool)
        .await?;
    Ok(rows.len() == 1 && rows[0].eq_ignore_ascii_case("ok"))
}

async fn referenced_blobs(pool: &SqlitePool) -> Result<BTreeSet<BlobId>, RecoveryError> {
    let values: Vec<String> = sqlx::query_scalar(
        "SELECT blob_id FROM blob_refs GROUP BY blob_id HAVING SUM(ref_count) > 0 ORDER BY blob_id",
    )
    .fetch_all(pool)
    .await?;
    values
        .into_iter()
        .map(|value| BlobId::from_str(&value).map_err(|_| RecoveryError::InvalidBlobId { value }))
        .collect()
}

async fn enumerate_blobs(root: &Path) -> Result<BTreeMap<BlobId, PathBuf>, RecoveryError> {
    let mut blobs = BTreeMap::new();
    let mut first_level = tokio::fs::read_dir(root).await?;
    while let Some(first) = first_level.next_entry().await? {
        if !first.file_type().await?.is_dir() || first.file_name() == ".trash" {
            continue;
        }
        let mut second_level = tokio::fs::read_dir(first.path()).await?;
        while let Some(second) = second_level.next_entry().await? {
            if !second.file_type().await?.is_dir() {
                continue;
            }
            let mut files = tokio::fs::read_dir(second.path()).await?;
            while let Some(file) = files.next_entry().await? {
                if !file.file_type().await?.is_file()
                    || file.path().extension().and_then(|value| value.to_str()) != Some("jcb")
                {
                    continue;
                }
                let path = file.path();
                let Some(stem) = path.file_stem().and_then(|value| value.to_str()) else {
                    continue;
                };
                if let Ok(id) = BlobId::from_str(stem) {
                    blobs.insert(id, path);
                }
            }
        }
    }
    Ok(blobs)
}

async fn fts_mismatches(pool: &SqlitePool) -> Result<u64, RecoveryError> {
    let expected_rows = sqlx::query(
        "SELECT f.id AS fragment_id, f.scope_key, CAST(c.inline_value AS TEXT) AS body
         FROM fragments AS f
         JOIN fragment_contents AS c ON c.id = f.content_id
         WHERE c.storage_kind IN ('inline_text', 'inline_json')",
    )
    .fetch_all(pool)
    .await?;
    let actual_rows = sqlx::query("SELECT fragment_id, scope_key, body FROM fragment_fts")
        .fetch_all(pool)
        .await?;
    let expected = row_multiset(expected_rows)?;
    let actual = row_multiset(actual_rows)?;
    let keys: BTreeSet<_> = expected.keys().chain(actual.keys()).cloned().collect();
    Ok(keys
        .into_iter()
        .map(|key| {
            expected
                .get(&key)
                .unwrap_or(&0)
                .abs_diff(*actual.get(&key).unwrap_or(&0))
        })
        .map(usize_to_u64)
        .sum())
}

fn row_multiset(
    rows: Vec<sqlx::sqlite::SqliteRow>,
) -> Result<BTreeMap<(String, String, String), usize>, RecoveryError> {
    let mut values = BTreeMap::new();
    for row in rows {
        let key = (
            row.try_get("fragment_id")?,
            row.try_get("scope_key")?,
            row.try_get("body")?,
        );
        *values.entry(key).or_insert(0) += 1;
    }
    Ok(values)
}

fn acquire_gc_lock(root: &Path) -> Result<File, RecoveryError> {
    let lock = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(root.join(".gc.lock"))?;
    match lock.try_lock_exclusive() {
        Ok(()) => Ok(lock),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
            Err(RecoveryError::LockUnavailable)
        }
        Err(error) => Err(error.into()),
    }
}

async fn move_to_trash_and_delete(
    root: &Path,
    source: &Path,
    id: &BlobId,
) -> Result<(), RecoveryError> {
    let trash = root.join(".trash");
    tokio::fs::create_dir_all(&trash).await?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_nanos();
    let target = trash.join(format!(
        "{}.{}.{}.jcb",
        id.as_str(),
        std::process::id(),
        nonce
    ));
    tokio::fs::rename(source, &target).await?;
    tokio::fs::remove_file(target).await?;
    Ok(())
}

async fn count_indexable_fragments(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
) -> Result<u64, RecoveryError> {
    query_count(
        transaction,
        "SELECT COUNT(*)
         FROM fragments AS f
         JOIN fragment_contents AS c ON c.id = f.content_id
         WHERE c.storage_kind IN ('inline_text', 'inline_json')",
    )
    .await
}

async fn query_count(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    query: &str,
) -> Result<u64, RecoveryError> {
    let count: i64 = sqlx::query_scalar(query)
        .fetch_one(&mut **transaction)
        .await?;
    Ok(u64::try_from(count).unwrap_or(0))
}

fn usize_to_u64(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}
