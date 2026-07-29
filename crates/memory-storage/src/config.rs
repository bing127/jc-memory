use std::{
    path::{Path, PathBuf},
    time::Duration,
};

/// Configuration used to open a SQLite-backed memory store.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SqliteConfig {
    path: PathBuf,
    max_connections: u32,
    busy_timeout: Duration,
}

impl SqliteConfig {
    /// Creates a configuration for a database at `path`.
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            max_connections: 4,
            busy_timeout: Duration::from_secs(5),
        }
    }

    /// Sets the maximum number of connections in the pool.
    #[must_use]
    pub const fn with_max_connections(mut self, max_connections: u32) -> Self {
        self.max_connections = max_connections;
        self
    }

    /// Sets how long `SQLite` waits for a locked database before failing.
    #[must_use]
    pub const fn with_busy_timeout(mut self, busy_timeout: Duration) -> Self {
        self.busy_timeout = busy_timeout;
        self
    }

    /// Returns the database path.
    #[must_use]
    pub fn path(&self) -> &Path {
        self.path.as_path()
    }

    /// Returns the configured maximum connection count.
    #[must_use]
    pub const fn max_connections(&self) -> u32 {
        self.max_connections
    }

    /// Returns the configured `SQLite` busy timeout.
    #[must_use]
    pub const fn busy_timeout(&self) -> Duration {
        self.busy_timeout
    }

    pub(crate) fn effective_max_connections(&self) -> u32 {
        if self.path == Path::new(":memory:") {
            1
        } else {
            self.max_connections
        }
    }
}
