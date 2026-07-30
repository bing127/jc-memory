use std::path::{Component, Path, PathBuf};

use chrono::{DateTime, Utc};
use jc_memory_protocol::MemoryScope;
use serde::{Deserialize, Serialize};

/// 当前 Snapshot Manifest 格式版本。
pub const SNAPSHOT_MANIFEST_VERSION: u32 = 1;

/// Snapshot 中 `SQLite` 镜像的完整性信息。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SnapshotFile {
    /// Snapshot 内的相对路径。
    pub path: String,
    /// 文件字节的 SHA-256 十六进制摘要。
    pub sha256: String,
    /// 文件长度。
    pub byte_len: u64,
}

/// Snapshot 中一个不可变 Blob 的完整性信息。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SnapshotBlob {
    /// 内容寻址 Blob 标识。
    pub id: String,
    /// Snapshot 内的相对路径。
    pub path: String,
    /// 编码后 Blob 文件的 SHA-256 十六进制摘要。
    pub sha256: String,
    /// 编码后 Blob 文件长度。
    pub byte_len: u64,
}

/// 可 round-trip 且版本化的 Snapshot Manifest。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SnapshotManifest {
    /// Manifest 契约版本。
    pub manifest_version: u32,
    /// `SQLite` Schema 迁移版本。
    pub schema_version: i64,
    /// Snapshot 所属的精确 Scope。
    pub scope: MemoryScope,
    /// `SQLite` 一致镜像信息。
    pub sqlite: SnapshotFile,
    /// `SQLite` 镜像实际引用的 Blob 清单。
    pub blobs: Vec<SnapshotBlob>,
    /// Manifest 创建时间。
    pub created_at: DateTime<Utc>,
}

impl SnapshotManifest {
    /// 从 Snapshot 目录读取并反序列化 Manifest。
    ///
    /// # Errors
    ///
    /// 文件不可读或 JSON 不符合 Manifest 契约时返回错误。
    pub async fn read(snapshot_dir: impl AsRef<Path>) -> Result<Self, SnapshotError> {
        let bytes = tokio::fs::read(snapshot_dir.as_ref().join("manifest.json")).await?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    pub(crate) fn validate_contract(&self) -> Result<(), SnapshotError> {
        if self.manifest_version != SNAPSHOT_MANIFEST_VERSION {
            return Err(SnapshotError::UnsupportedManifestVersion {
                actual: self.manifest_version,
            });
        }
        if self.schema_version < 1 {
            return Err(SnapshotError::InvalidSchemaVersion {
                actual: self.schema_version,
            });
        }
        if self.sqlite.path != "memory.sqlite3" {
            return Err(SnapshotError::InvalidName {
                name: self.sqlite.path.clone(),
            });
        }
        validate_relative_path(&self.sqlite.path)?;
        for blob in &self.blobs {
            validate_relative_path(&blob.path)?;
        }
        Ok(())
    }
}

/// Snapshot 创建、校验或 Restore 失败。
#[derive(Debug, thiserror::Error)]
pub enum SnapshotError {
    /// Snapshot 文件系统操作失败。
    #[error("snapshot filesystem operation failed: {0}")]
    Io(#[from] std::io::Error),
    /// Snapshot JSON 编解码失败。
    #[error("snapshot manifest serialization failed: {0}")]
    Serialization(#[from] serde_json::Error),
    /// `SQLite` 一致备份或隔离校验失败。
    #[error("snapshot SQLite operation failed: {0}")]
    Database(#[from] sqlx::Error),
    /// Blob 内容寻址校验失败。
    #[error("snapshot blob operation failed: {0}")]
    Blob(#[from] crate::BlobError),
    /// Snapshot 名称可能逃逸目标目录。
    #[error("invalid snapshot name: {name}")]
    InvalidName {
        /// 被拒绝的名称。
        name: String,
    },
    /// 目标 Snapshot 已存在。
    #[error("snapshot already exists: {path}")]
    AlreadyExists {
        /// 已存在的路径。
        path: PathBuf,
    },
    /// Manifest 版本不受支持。
    #[error("unsupported snapshot manifest version: {actual}")]
    UnsupportedManifestVersion {
        /// 实际版本。
        actual: u32,
    },
    /// Manifest Schema 版本非法。
    #[error("invalid snapshot schema version: {actual}")]
    InvalidSchemaVersion {
        /// 实际版本。
        actual: i64,
    },
    /// Manifest Scope 与 Restore 请求不一致。
    #[error("snapshot scope does not match requested restore scope")]
    ScopeMismatch,
    /// `SQLite` 镜像包含 Manifest Scope 之外的权威记录。
    #[error("snapshot contains authoritative rows outside the manifest scope")]
    ScopeContentsMismatch,
    /// 文件长度或 SHA-256 与 Manifest 不一致。
    #[error("snapshot checksum mismatch for {path}")]
    ChecksumMismatch {
        /// 校验失败的相对路径。
        path: String,
    },
    /// `SQLite` 镜像的 Schema 与 Manifest 不一致。
    #[error("snapshot schema mismatch: manifest {manifest}, database {database}")]
    SchemaMismatch {
        /// Manifest 声明版本。
        manifest: i64,
        /// `SQLite` 实际版本。
        database: i64,
    },
    /// `SQLite` integrity check 失败。
    #[error("snapshot SQLite integrity check failed: {details}")]
    IntegrityCheck {
        /// `SQLite` 返回的诊断。
        details: String,
    },
    /// `SQLite` 镜像引用的 Blob 不在 Snapshot 中。
    #[error("snapshot is missing referenced blob {id}")]
    MissingBlob {
        /// 缺失的 Blob 标识。
        id: String,
    },
    /// Manifest Blob 标识或路径不符合内容寻址布局。
    #[error("invalid snapshot blob entry: {id}")]
    InvalidBlob {
        /// 非法 Blob 标识。
        id: String,
    },
    /// Restore 原子切换失败，旧目录回滚也失败。
    #[error("restore switch failed and rollback failed: switch={switch}; rollback={rollback}")]
    RollbackFailed {
        /// 发布新目录时的错误。
        switch: std::io::Error,
        /// 恢复旧目录时的错误。
        rollback: std::io::Error,
    },
}

pub(crate) fn validate_relative_path(value: &str) -> Result<(), SnapshotError> {
    let path = Path::new(value);
    if value.is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(SnapshotError::InvalidName {
            name: value.to_owned(),
        });
    }
    Ok(())
}
