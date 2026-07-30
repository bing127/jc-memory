//! `SQLite` and content-addressed storage for Jiangcheng Memory.

mod blob;
mod config;
mod error;
mod recovery;
mod repository;
mod sqlite;

pub use blob::{BlobError, BlobOptions, BlobReceipt, BlobStore, Compression, Encryption};
pub use config::SqliteConfig;
pub use error::StorageError;
pub use recovery::{
    GcPolicy, GcReport, IntegrityReport, RebuildReport, RecoveryError, RecoveryFailpoint,
    RecoveryManager,
};
pub use repository::{
    AppendReceipt, FragmentSearchHit, MemoryRepository, QuarantineDisposition, RepositoryFailpoint,
    RepositoryStats, SqliteMemoryRepository,
};
pub use sqlite::SqliteStore;
