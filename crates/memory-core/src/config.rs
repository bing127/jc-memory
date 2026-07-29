use std::{
    path::{Path, PathBuf},
    time::Duration,
};

/// Local embedded-engine configuration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemoryConfig {
    data_dir: PathBuf,
    busy_timeout: Duration,
}

impl MemoryConfig {
    /// Creates a local-only configuration rooted at `data_dir`.
    #[must_use]
    pub fn local(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: data_dir.into(),
            busy_timeout: Duration::from_secs(5),
        }
    }

    /// Sets how long each `SQLite` lock attempt may wait before returning Busy.
    #[must_use]
    pub const fn with_busy_timeout(mut self, busy_timeout: Duration) -> Self {
        self.busy_timeout = busy_timeout;
        self
    }

    /// Returns the root directory containing all durable engine state.
    #[must_use]
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub(crate) const fn busy_timeout(&self) -> Duration {
        self.busy_timeout
    }

    pub(crate) fn database_path(&self) -> PathBuf {
        self.data_dir.join("memory.sqlite3")
    }

    pub(crate) fn blob_root(&self) -> PathBuf {
        self.data_dir.join("blobs")
    }
}
