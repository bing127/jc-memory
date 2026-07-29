use std::path::{Path, PathBuf};

/// Local embedded-engine configuration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemoryConfig {
    data_dir: PathBuf,
}

impl MemoryConfig {
    /// Creates a local-only configuration rooted at `data_dir`.
    #[must_use]
    pub fn local(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: data_dir.into(),
        }
    }

    /// Returns the root directory containing all durable engine state.
    #[must_use]
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub(crate) fn database_path(&self) -> PathBuf {
        self.data_dir.join("memory.sqlite3")
    }

    pub(crate) fn blob_root(&self) -> PathBuf {
        self.data_dir.join("blobs")
    }
}
