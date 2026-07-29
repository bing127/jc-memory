/// Overall embedded-engine health classification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HealthStatus {
    /// Authoritative storage and all inspected derived state are consistent.
    Healthy,
    /// Authoritative storage is usable but derived state or cleanup needs attention.
    Degraded,
    /// Authoritative database or referenced blob evidence is not intact.
    Unhealthy,
}

/// Current health of the embedded engine's durable storage.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HealthReport {
    /// Overall health classification.
    pub status: HealthStatus,
    /// Whether `SQLite` passed its quick consistency check.
    pub database_ok: bool,
    /// Whether every referenced blob exists and every discovered blob verifies successfully.
    pub blob_store_ok: bool,
    /// Highest successfully applied storage migration version.
    pub schema_version: i64,
}
