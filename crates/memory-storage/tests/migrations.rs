//! `SQLite` connection and schema migration contract tests.

use std::path::Path;

use jc_memory_storage::{SqliteConfig, SqliteStore};
use sqlx::{Connection, Executor, Row, SqliteConnection};
use tempfile::TempDir;

const REQUIRED_TABLES: &[&str] = &[
    "_sqlx_migrations",
    "actions",
    "approvals",
    "audit_logs",
    "blob_refs",
    "capabilities",
    "capability_runs",
    "consolidation_jobs",
    "consolidation_artifacts",
    "decisions",
    "edges",
    "entities",
    "entity_aliases",
    "events",
    "evidence_refs",
    "fragment_contents",
    "fragment_fts",
    "fragments",
    "governance_receipts",
    "hypotheses",
    "cold_tier_entries",
    "memory_models",
    "model_versions",
    "outcomes",
    "points",
    "quarantine",
    "recall_activations",
    "recall_sessions",
    "retention_plans",
    "scope_sequences",
    "snapshots",
    "state_versions",
    "states",
    "surface_members",
    "surfaces",
    "transitions",
];

const REQUIRED_INDEXES: &[&str] = &[
    "idx_audit_logs_scope_created",
    "idx_blob_refs_blob",
    "idx_blob_refs_owner",
    "idx_capabilities_scope_kind_version",
    "idx_cold_tier_entries_scope_tier",
    "idx_consolidation_artifacts_scope_cursor",
    "idx_edges_source_kind_validity",
    "idx_edges_target_kind_validity",
    "idx_events_scope_sequence",
    "idx_fragments_scope_observed",
    "idx_fragments_scope_occurred",
    "idx_governance_receipts_scope_created",
    "idx_points_scope_validity",
    "idx_recall_sessions_scope_started",
    "idx_retention_plans_scope_status",
    "idx_state_versions_state_validity",
    "idx_surface_members_point",
    "idx_surfaces_scope_validity",
    "idx_transitions_trace",
];

struct TestDatabase {
    _directory: TempDir,
    store: SqliteStore,
}

impl TestDatabase {
    async fn new() -> Self {
        let directory = tempfile::tempdir().expect("create database temp directory");
        let config = SqliteConfig::new(directory.path().join("memory.sqlite3"));
        let store = SqliteStore::open(&config)
            .await
            .expect("open SQLite test store");
        Self {
            _directory: directory,
            store,
        }
    }
}

#[tokio::test]
async fn migration_enables_required_pragmas_tables_and_indexes() {
    let database = TestDatabase::new().await;

    let journal_mode: String = sqlx::query_scalar("PRAGMA journal_mode")
        .fetch_one(database.store.pool())
        .await
        .expect("read journal mode");
    assert_eq!(journal_mode.to_ascii_lowercase(), "wal");

    let foreign_keys: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
        .fetch_one(database.store.pool())
        .await
        .expect("read foreign key setting");
    assert_eq!(foreign_keys, 1);

    let synchronous: i64 = sqlx::query_scalar("PRAGMA synchronous")
        .fetch_one(database.store.pool())
        .await
        .expect("read synchronous setting");
    assert_eq!(synchronous, 1, "NORMAL synchronous must be enabled");

    let names: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type IN ('table', 'view') ORDER BY name",
    )
    .fetch_all(database.store.pool())
    .await
    .expect("list migrated tables");
    for required in REQUIRED_TABLES {
        assert!(
            names.iter().any(|name| name == required),
            "missing table {required}"
        );
    }

    let indexes: Vec<String> =
        sqlx::query_scalar("SELECT name FROM sqlite_master WHERE type = 'index' ORDER BY name")
            .fetch_all(database.store.pool())
            .await
            .expect("list migrated indexes");
    for required in REQUIRED_INDEXES {
        assert!(
            indexes.iter().any(|name| name == required),
            "missing index {required}"
        );
    }
}

#[tokio::test]
async fn migration_provides_a_working_fts5_index() {
    let database = TestDatabase::new().await;

    sqlx::query("INSERT INTO fragment_fts (fragment_id, scope_key, body) VALUES (?1, ?2, ?3)")
        .bind("fragment-1")
        .bind("scope-1")
        .bind("durable memory retrieval")
        .execute(database.store.pool())
        .await
        .expect("insert FTS document");

    let fragment_id: String = sqlx::query_scalar(
        "SELECT fragment_id FROM fragment_fts WHERE fragment_fts MATCH 'memory'",
    )
    .fetch_one(database.store.pool())
    .await
    .expect("query FTS document");
    assert_eq!(fragment_id, "fragment-1");
}

#[tokio::test]
async fn reopening_the_same_database_is_migration_idempotent() {
    let directory = tempfile::tempdir().expect("create database temp directory");
    let database_path = directory.path().join("memory.sqlite3");
    let config = SqliteConfig::new(&database_path);

    let first = SqliteStore::open(&config)
        .await
        .expect("open database first time");
    first.pool().close().await;

    let second = SqliteStore::open(&config)
        .await
        .expect("open database second time");
    let applied_migrations: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations WHERE success = 1")
            .fetch_one(second.pool())
            .await
            .expect("count applied migrations");
    assert_eq!(applied_migrations, 3);
}

#[tokio::test]
async fn surface_schema_preserves_multiple_versions() {
    let database = TestDatabase::new().await;
    let scope_key = "a".repeat(64);
    for version in [1_i64, 2] {
        sqlx::query(
            "INSERT INTO surfaces (
                id, scope_key, kind, status, semantic_center_json,
                activation_threshold, confidence, version, valid_from_us,
                source_evidence_json, created_at_us
             ) VALUES ('surface-1', ?1, 'dynamic', 'active', '[]', 0.5, 0.8, ?2, ?2, '[]', ?2)",
        )
        .bind(&scope_key)
        .bind(version)
        .execute(database.store.pool())
        .await
        .expect("insert immutable surface version");
    }

    let versions: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM surfaces WHERE id = 'surface-1'")
        .fetch_one(database.store.pool())
        .await
        .expect("count surface versions");
    assert_eq!(versions, 2);
}

#[tokio::test]
async fn a_failed_migration_rolls_back_its_partial_schema() {
    let directory = tempfile::tempdir().expect("create migration temp directory");
    let migrations_path = directory.path().join("migrations");
    tokio::fs::create_dir(&migrations_path)
        .await
        .expect("create migration directory");
    write_migration(
        &migrations_path,
        "0001_base.sql",
        "CREATE TABLE stable_table (id INTEGER PRIMARY KEY) STRICT;",
    )
    .await;
    write_migration(
        &migrations_path,
        "0002_broken.sql",
        "CREATE TABLE partial_table (id INTEGER PRIMARY KEY) STRICT;\nTHIS IS INVALID SQL;",
    )
    .await;

    let database_path = directory.path().join("rollback.sqlite3");
    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(&database_path)
        .create_if_missing(true);
    let mut connection = SqliteConnection::connect_with(&options)
        .await
        .expect("open rollback test database");
    let migrator = sqlx::migrate::Migrator::new(migrations_path)
        .await
        .expect("load test migrations");

    let error = migrator
        .run(&mut connection)
        .await
        .expect_err("broken migration must fail");
    let error_message = error.to_string();
    assert!(
        error_message.contains("syntax error"),
        "unexpected migration failure: {error_message}"
    );

    let partial_table_count: i64 = connection
        .fetch_one(
            "SELECT COUNT(*) AS count FROM sqlite_master \
             WHERE type = 'table' AND name = 'partial_table'",
        )
        .await
        .expect("inspect schema after failed migration")
        .get("count");
    assert_eq!(partial_table_count, 0);
}

#[tokio::test]
async fn migrated_database_passes_integrity_checks() {
    let database = TestDatabase::new().await;
    database
        .store
        .integrity_check()
        .await
        .expect("database must pass quick_check and integrity_check");
}

async fn write_migration(directory: &Path, name: &str, contents: &str) {
    tokio::fs::write(directory.join(name), contents)
        .await
        .expect("write test migration");
}
