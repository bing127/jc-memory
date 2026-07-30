use std::{
    path::{Path, PathBuf},
    str::FromStr as _,
    time::{SystemTime, UNIX_EPOCH},
};

use chrono::Utc;
use jc_memory_protocol::{BlobId, MemoryScope};
use sha2::{Digest as _, Sha256};
use sqlx::{Connection as _, Row as _, SqliteConnection, SqlitePool, sqlite::SqliteConnectOptions};
use tokio::io::AsyncReadExt as _;

use crate::{
    BlobOptions, BlobStore, SnapshotBlob, SnapshotError, SnapshotFile, SnapshotManifest,
    export::{SNAPSHOT_MANIFEST_VERSION, validate_relative_path},
};

const DATABASE_FILE: &str = "memory.sqlite3";
const MANIFEST_FILE: &str = "manifest.json";

/// Snapshot 创建结果。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SnapshotReceipt {
    /// 原子发布后的 Snapshot 目录。
    pub path: PathBuf,
    /// 已写入该目录的 Manifest。
    pub manifest: SnapshotManifest,
}

/// Restore 原子切换结果。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RestoreReceipt {
    /// 当前生效的数据目录。
    pub data_dir: PathBuf,
    /// 被替换数据目录的可恢复备份；首次安装时为空。
    pub backup_dir: Option<PathBuf>,
    /// 已校验并恢复的 Manifest。
    pub manifest: SnapshotManifest,
}

/// 基于现有 `SQLite` 连接池和 Blob 目录的 Snapshot 管理器。
#[derive(Clone, Debug)]
pub struct SnapshotManager {
    pool: SqlitePool,
    blob_root: PathBuf,
}

impl SnapshotManager {
    /// 复用活动 `SQLite` 连接池，并从 Blob 根目录确定数据目录。
    #[must_use]
    pub fn new(pool: SqlitePool, blob_root: impl Into<PathBuf>) -> Self {
        let blob_root = blob_root.into();
        Self { pool, blob_root }
    }

    /// 使用 `SQLite` `VACUUM INTO` 一致镜像和同文件系统 rename 原子发布 Snapshot。
    ///
    /// # Errors
    ///
    /// 名称非法、目标已存在、SQLite 备份失败、引用 Blob 缺失或文件发布失败时返回错误。
    pub async fn create(
        &self,
        snapshot_root: impl AsRef<Path>,
        name: &str,
        scope: MemoryScope,
    ) -> Result<SnapshotReceipt, SnapshotError> {
        validate_snapshot_name(name)?;
        let snapshot_root = snapshot_root.as_ref();
        tokio::fs::create_dir_all(snapshot_root).await?;
        let target = snapshot_root.join(name);
        if tokio::fs::try_exists(&target).await? {
            return Err(SnapshotError::AlreadyExists { path: target });
        }
        let staging = StagingDirectory::create(snapshot_root, name).await?;
        let database_path = staging.path().join(DATABASE_FILE);
        let destination = database_path.to_string_lossy().into_owned();
        sqlx::query("VACUUM INTO ?1")
            .bind(destination)
            .execute(&self.pool)
            .await?;
        database_integrity(&database_path).await?;
        validate_database_scope(&database_path, &scope).await?;
        let schema_version = schema_version(&database_path).await?;
        let sqlite = snapshot_file(&database_path, DATABASE_FILE).await?;
        let blob_ids = referenced_blobs(&database_path).await?;
        let mut blobs = Vec::with_capacity(blob_ids.len());
        for id in blob_ids {
            BlobId::from_str(&id).map_err(|_| SnapshotError::InvalidBlob { id: id.clone() })?;
            let source = blob_path(&self.blob_root, &id);
            if !tokio::fs::try_exists(&source).await? {
                return Err(SnapshotError::MissingBlob { id });
            }
            let relative = blob_relative_path(&id);
            let destination = staging.path().join(&relative);
            if let Some(parent) = destination.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            tokio::fs::copy(&source, &destination).await?;
            let file = snapshot_file(&destination, &relative).await?;
            blobs.push(SnapshotBlob {
                id,
                path: file.path,
                sha256: file.sha256,
                byte_len: file.byte_len,
            });
        }
        let manifest = SnapshotManifest {
            manifest_version: SNAPSHOT_MANIFEST_VERSION,
            schema_version,
            scope,
            sqlite,
            blobs,
            created_at: Utc::now(),
        };
        let manifest_bytes = serde_json::to_vec_pretty(&manifest)?;
        write_synced(&staging.path().join(MANIFEST_FILE), &manifest_bytes).await?;
        sync_directory(staging.path()).await?;
        let staging_path = staging.keep();
        if let Err(error) = tokio::fs::rename(&staging_path, &target).await {
            let _ = tokio::fs::remove_dir_all(&staging_path).await;
            return Err(error.into());
        }
        sync_directory(snapshot_root).await?;
        Ok(SnapshotReceipt {
            path: target,
            manifest,
        })
    }

    /// 在数据目录同级隔离复制并完整校验后，原子切换当前数据目录。
    ///
    /// 调用方必须先关闭目标目录上的活动 `SQLite` 连接。
    ///
    /// # Errors
    ///
    /// Manifest、Scope、Schema、SQLite 或 Blob 校验失败，或目录切换失败时返回错误。
    pub async fn restore(
        snapshot_dir: impl AsRef<Path>,
        data_dir: impl AsRef<Path>,
        expected_scope: &MemoryScope,
    ) -> Result<RestoreReceipt, SnapshotError> {
        let snapshot_dir = snapshot_dir.as_ref();
        let data_dir = data_dir.as_ref();
        let manifest = validate_snapshot(snapshot_dir, Some(expected_scope)).await?;
        let parent = data_dir
            .parent()
            .ok_or_else(|| SnapshotError::InvalidName {
                name: data_dir.display().to_string(),
            })?;
        tokio::fs::create_dir_all(parent).await?;
        let name = data_dir
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| SnapshotError::InvalidName {
                name: data_dir.display().to_string(),
            })?;
        let staging = StagingDirectory::create(parent, &format!("restore-{name}")).await?;
        copy_manifest_files(snapshot_dir, staging.path(), &manifest).await?;
        validate_snapshot(staging.path(), Some(expected_scope)).await?;
        sync_directory(staging.path()).await?;
        let staging_path = staging.keep();
        let backup = if tokio::fs::try_exists(data_dir).await? {
            let backup = unique_sibling(parent, &format!("{name}.restore-backup"));
            tokio::fs::rename(data_dir, &backup).await?;
            Some(backup)
        } else {
            None
        };
        if let Err(switch) = tokio::fs::rename(&staging_path, data_dir).await {
            let _ = tokio::fs::remove_dir_all(&staging_path).await;
            if let Some(backup) = &backup {
                if let Err(rollback) = tokio::fs::rename(backup, data_dir).await {
                    return Err(SnapshotError::RollbackFailed { switch, rollback });
                }
            }
            return Err(switch.into());
        }
        sync_directory(parent).await?;
        Ok(RestoreReceipt {
            data_dir: data_dir.to_path_buf(),
            backup_dir: backup,
            manifest,
        })
    }
}

pub(crate) async fn validate_snapshot(
    snapshot_dir: &Path,
    expected_scope: Option<&MemoryScope>,
) -> Result<SnapshotManifest, SnapshotError> {
    let manifest = SnapshotManifest::read(snapshot_dir).await?;
    manifest.validate_contract()?;
    if expected_scope.is_some_and(|expected| expected != &manifest.scope) {
        return Err(SnapshotError::ScopeMismatch);
    }
    verify_file(snapshot_dir, &manifest.sqlite).await?;
    let database_path = snapshot_dir.join(&manifest.sqlite.path);
    let actual_schema = schema_version(&database_path).await?;
    if actual_schema != manifest.schema_version {
        return Err(SnapshotError::SchemaMismatch {
            manifest: manifest.schema_version,
            database: actual_schema,
        });
    }
    database_integrity(&database_path).await?;
    validate_database_scope(&database_path, &manifest.scope).await?;
    let referenced: std::collections::BTreeSet<_> = referenced_blobs(&database_path)
        .await?
        .into_iter()
        .collect();
    let listed: std::collections::BTreeSet<_> =
        manifest.blobs.iter().map(|blob| blob.id.clone()).collect();
    if listed.len() != manifest.blobs.len() {
        return Err(SnapshotError::InvalidBlob {
            id: "duplicate manifest entry".to_owned(),
        });
    }
    if let Some(id) = referenced.difference(&listed).next() {
        return Err(SnapshotError::MissingBlob { id: id.clone() });
    }
    if let Some(id) = listed.difference(&referenced).next() {
        return Err(SnapshotError::InvalidBlob { id: id.clone() });
    }
    let blob_store = if manifest.blobs.is_empty() {
        None
    } else {
        Some(BlobStore::open(snapshot_dir.join("blobs"), BlobOptions::plain()).await?)
    };
    for blob in &manifest.blobs {
        let id = BlobId::from_str(&blob.id).map_err(|_| SnapshotError::InvalidBlob {
            id: blob.id.clone(),
        })?;
        let expected = blob_relative_path(&blob.id);
        if blob.path != expected {
            return Err(SnapshotError::InvalidBlob {
                id: blob.id.clone(),
            });
        }
        verify_file(
            snapshot_dir,
            &SnapshotFile {
                path: blob.path.clone(),
                sha256: blob.sha256.clone(),
                byte_len: blob.byte_len,
            },
        )
        .await?;
        blob_store
            .as_ref()
            .expect("非空 Blob Manifest 已打开 BlobStore")
            .verify(&id)
            .await
            .map_err(|_| SnapshotError::ChecksumMismatch {
                path: blob.path.clone(),
            })?;
    }
    Ok(manifest)
}

async fn copy_manifest_files(
    source: &Path,
    destination: &Path,
    manifest: &SnapshotManifest,
) -> Result<(), SnapshotError> {
    tokio::fs::copy(source.join(MANIFEST_FILE), destination.join(MANIFEST_FILE)).await?;
    copy_file(source, destination, &manifest.sqlite.path).await?;
    for blob in &manifest.blobs {
        copy_file(source, destination, &blob.path).await?;
    }
    Ok(())
}

async fn copy_file(source: &Path, destination: &Path, relative: &str) -> Result<(), SnapshotError> {
    validate_relative_path(relative)?;
    let target = destination.join(relative);
    if let Some(parent) = target.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::copy(source.join(relative), target).await?;
    Ok(())
}

async fn verify_file(root: &Path, expected: &SnapshotFile) -> Result<(), SnapshotError> {
    validate_relative_path(&expected.path)?;
    let root = tokio::fs::canonicalize(root).await?;
    let path = tokio::fs::canonicalize(root.join(&expected.path)).await?;
    if !path.starts_with(&root) || !tokio::fs::metadata(&path).await?.is_file() {
        return Err(SnapshotError::InvalidName {
            name: expected.path.clone(),
        });
    }
    let actual = snapshot_file(&path, &expected.path).await?;
    if actual.byte_len != expected.byte_len || actual.sha256 != expected.sha256 {
        return Err(SnapshotError::ChecksumMismatch {
            path: expected.path.clone(),
        });
    }
    Ok(())
}

async fn snapshot_file(path: &Path, relative: &str) -> Result<SnapshotFile, SnapshotError> {
    let mut file = tokio::fs::File::open(path).await?;
    let mut digest = Sha256::new();
    let mut byte_len = 0_u64;
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
        byte_len = byte_len.saturating_add(u64::try_from(read).unwrap_or(u64::MAX));
    }
    Ok(SnapshotFile {
        path: relative.to_owned(),
        sha256: hex::encode(digest.finalize()),
        byte_len,
    })
}

async fn referenced_blobs(database: &Path) -> Result<Vec<String>, SnapshotError> {
    let mut connection = read_only_connection(database).await?;
    let rows = sqlx::query(
        "SELECT DISTINCT c.blob_id
         FROM fragment_contents AS c
         JOIN fragments AS f ON f.content_id = c.id
         WHERE c.storage_kind = 'blob' AND c.blob_id IS NOT NULL
         ORDER BY c.blob_id",
    )
    .fetch_all(&mut connection)
    .await?;
    Ok(rows
        .iter()
        .map(|row| row.get::<String, _>("blob_id"))
        .collect())
}

async fn schema_version(database: &Path) -> Result<i64, SnapshotError> {
    let mut connection = read_only_connection(database).await?;
    Ok(sqlx::query_scalar(
        "SELECT COALESCE(MAX(version), 0) FROM _sqlx_migrations WHERE success = 1",
    )
    .fetch_one(&mut connection)
    .await?)
}

async fn database_integrity(database: &Path) -> Result<(), SnapshotError> {
    let mut connection = read_only_connection(database).await?;
    for statement in ["PRAGMA quick_check", "PRAGMA integrity_check"] {
        let rows: Vec<String> = sqlx::query_scalar(statement)
            .fetch_all(&mut connection)
            .await?;
        if rows.len() != 1 || !rows[0].eq_ignore_ascii_case("ok") {
            return Err(SnapshotError::IntegrityCheck {
                details: rows.join("; "),
            });
        }
    }
    Ok(())
}

async fn validate_database_scope(
    database: &Path,
    scope: &MemoryScope,
) -> Result<(), SnapshotError> {
    let scope_key = canonical_scope_key(scope)?;
    let mut connection = read_only_connection(database).await?;
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT schema.name
         FROM sqlite_schema AS schema
         JOIN pragma_table_info(schema.name) AS column
         WHERE schema.type = 'table' AND column.name = 'scope_key'
         ORDER BY schema.name",
    )
    .fetch_all(&mut connection)
    .await?;
    for table in tables {
        // 表名来自 SQLite Schema，仍需引用转义以避免损坏快照改变语句结构。
        let quoted_table = table.replace('"', "\"\"");
        let statement = format!("SELECT COUNT(*) FROM \"{quoted_table}\" WHERE scope_key != ?1");
        let foreign_rows: i64 = sqlx::query_scalar(&statement)
            .bind(&scope_key)
            .fetch_one(&mut connection)
            .await?;
        if foreign_rows != 0 {
            return Err(SnapshotError::ScopeContentsMismatch);
        }
    }
    Ok(())
}

fn canonical_scope_key(scope: &MemoryScope) -> Result<String, SnapshotError> {
    let value = serde_json::to_value(scope)?;
    let bytes = serde_json::to_vec(&canonicalize(value))?;
    Ok(hex::encode(Sha256::digest(bytes)))
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

async fn read_only_connection(database: &Path) -> Result<SqliteConnection, SnapshotError> {
    let options = SqliteConnectOptions::new()
        .filename(database)
        .read_only(true)
        .foreign_keys(true);
    Ok(SqliteConnection::connect_with(&options).await?)
}

fn validate_snapshot_name(name: &str) -> Result<(), SnapshotError> {
    validate_relative_path(name)?;
    if name.contains(std::path::MAIN_SEPARATOR) {
        return Err(SnapshotError::InvalidName {
            name: name.to_owned(),
        });
    }
    Ok(())
}

fn blob_relative_path(id: &str) -> String {
    format!("blobs/{}/{}/{id}.jcb", &id[..2], &id[2..4])
}

fn blob_path(root: &Path, id: &str) -> PathBuf {
    root.join(&id[..2])
        .join(&id[2..4])
        .join(format!("{id}.jcb"))
}

async fn write_synced(path: &Path, bytes: &[u8]) -> Result<(), SnapshotError> {
    let mut file = tokio::fs::File::create(path).await?;
    tokio::io::AsyncWriteExt::write_all(&mut file, bytes).await?;
    file.sync_all().await?;
    Ok(())
}

#[cfg(unix)]
async fn sync_directory(path: &Path) -> Result<(), SnapshotError> {
    tokio::fs::File::open(path).await?.sync_all().await?;
    Ok(())
}

#[cfg(not(unix))]
async fn sync_directory(_path: &Path) -> Result<(), SnapshotError> {
    Ok(())
}

fn unique_sibling(parent: &Path, prefix: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    parent.join(format!(".{prefix}-{nonce}-{}", std::process::id()))
}

struct StagingDirectory {
    path: PathBuf,
    retained: bool,
}

impl StagingDirectory {
    async fn create(parent: &Path, prefix: &str) -> Result<Self, SnapshotError> {
        let path = unique_sibling(parent, &format!("{prefix}.staging"));
        tokio::fs::create_dir(&path).await?;
        Ok(Self {
            path,
            retained: false,
        })
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn keep(mut self) -> PathBuf {
        self.retained = true;
        self.path.clone()
    }
}

impl Drop for StagingDirectory {
    fn drop(&mut self) {
        if !self.retained {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}
