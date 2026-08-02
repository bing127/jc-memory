//! `SQLite` and content-addressed storage for Jiangcheng Memory.

mod blob;
mod config;
mod error;
mod export;
mod projection;
mod projection_derivations;
mod recovery;
mod repository;
mod snapshot;
mod sqlite;

pub use blob::{BlobError, BlobOptions, BlobReceipt, BlobStore, Compression, Encryption};
pub use config::SqliteConfig;
pub use error::StorageError;
pub use export::{SnapshotBlob, SnapshotError, SnapshotFile, SnapshotManifest};
pub use projection::{
    ProjectedGraph, ProjectedPoint, ProjectedStateSet, ProjectionCheckpoint, ProjectionFailpoint,
    ProjectionTarget, ProjectorKind, SqliteProjectionStore, StateSelector,
};
pub use projection_derivations::ProjectedRecallDerivations;
pub use recovery::{
    DerivedRebuildReport, GcPolicy, GcReport, IntegrityReport, RebuildReport, RebuildStage,
    RecoveryError, RecoveryFailpoint, RecoveryManager, verify_authority_hash,
};
pub use repository::{
    AppendReceipt, FragmentSearchHit, MemoryRepository, QuarantineDisposition, RepositoryFailpoint,
    RepositoryStats, SqliteMemoryRepository,
};
pub use snapshot::{RestoreReceipt, SnapshotManager, SnapshotReceipt};
pub use sqlite::SqliteStore;
