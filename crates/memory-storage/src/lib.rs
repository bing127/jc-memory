//! `SQLite` and content-addressed storage for Jiangcheng Memory.

mod blob;
mod config;
mod error;
mod export;
mod projection;
mod recovery;
mod repository;
mod snapshot;
mod sqlite;

pub use blob::{BlobError, BlobOptions, BlobReceipt, BlobStore, Compression, Encryption};
pub use config::SqliteConfig;
pub use error::StorageError;
pub use export::{SnapshotBlob, SnapshotError, SnapshotFile, SnapshotManifest};
pub use projection::{
    ProjectedStateSet, ProjectionCheckpoint, ProjectionFailpoint, ProjectionTarget, ProjectorKind,
    SqliteProjectionStore, StateSelector,
};
pub use recovery::{
    DerivedRebuildReport, GcPolicy, GcReport, IntegrityReport, RebuildReport, RebuildStage,
    RecoveryError, RecoveryFailpoint, RecoveryManager,
};
pub use repository::{
    AppendReceipt, FragmentSearchHit, MemoryRepository, QuarantineDisposition, RepositoryFailpoint,
    RepositoryStats, SqliteMemoryRepository,
};
pub use snapshot::{RestoreReceipt, SnapshotManager, SnapshotReceipt};
pub use sqlite::SqliteStore;
