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
    "runtime_runs",
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
    "idx_runtime_runs_scope_state",
    "idx_state_versions_state_validity",
    "idx_surface_members_point",
    "idx_surfaces_scope_validity",
    "idx_transitions_trace",
    "idx_transitions_runtime_run_step",
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
    assert_eq!(applied_migrations, 5);
}

#[tokio::test]
async fn v4_upgrade_preserves_v3_transition_rows_and_foreign_key_consumers() {
    let directory = tempfile::tempdir().expect("创建 v4 升级测试目录");
    let v3_migrations = directory.path().join("v3-migrations");
    tokio::fs::create_dir(&v3_migrations)
        .await
        .expect("创建 v3 migration 目录");
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
    for name in [
        "0001_initial.sql",
        "0002_consolidation_derived.sql",
        "0003_governance.sql",
    ] {
        tokio::fs::copy(source.join(name), v3_migrations.join(name))
            .await
            .expect("复制 v1-v3 migration");
    }
    let database_path = directory.path().join("upgrade.sqlite3");
    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(&database_path)
        .create_if_missing(true)
        .foreign_keys(true);
    let mut connection = SqliteConnection::connect_with(&options)
        .await
        .expect("打开升级数据库");
    sqlx::migrate::Migrator::new(v3_migrations)
        .await
        .expect("加载 v1-v3 migration")
        .run(&mut connection)
        .await
        .expect("应用 v1-v3 migration");
    let scope_key = "c".repeat(64);
    sqlx::query(
        "INSERT INTO transitions (
            id, scope_key, trace_id, from_stage, to_stage, payload_json, created_at_us
         ) VALUES ('pre-v4-transition', ?1, 'trace', 'old', 'new', '{}', 1)",
    )
    .bind(&scope_key)
    .execute(&mut connection)
    .await
    .expect("在 v3 Schema 写入 Transition");
    sqlx::query(
        "INSERT INTO approvals (
            id, scope_key, transition_id, principal_id, status, created_at_us
         ) VALUES ('pre-v4-approval', ?1, 'pre-v4-transition', 'principal', 'pending', 1)",
    )
    .bind(&scope_key)
    .execute(&mut connection)
    .await
    .expect("在 v3 Schema 写入外键消费者");

    sqlx::migrate::Migrator::new(source)
        .await
        .expect("加载完整 migration")
        .run(&mut connection)
        .await
        .expect("从 v3 升级到 v4");
    let runtime_fields: (Option<String>, Option<i64>) =
        sqlx::query_as("SELECT run_id, step FROM transitions WHERE id = 'pre-v4-transition'")
            .fetch_one(&mut connection)
            .await
            .expect("读取升级后的旧 Transition");
    assert_eq!(runtime_fields, (None, None));
    let approval_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM approvals WHERE transition_id = 'pre-v4-transition'",
    )
    .fetch_one(&mut connection)
    .await
    .expect("读取升级后的外键消费者");
    assert_eq!(approval_count, 1);
    let foreign_key_violations: Vec<(String, i64, String, i64)> =
        sqlx::query_as("PRAGMA foreign_key_check")
            .fetch_all(&mut connection)
            .await
            .expect("检查升级后外键");
    assert!(foreign_key_violations.is_empty());
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
async fn runtime_rows_are_append_only_without_breaking_legacy_transition_foreign_keys() {
    let database = TestDatabase::new().await;
    let scope_key = "a".repeat(64);
    let other_scope_key = "b".repeat(64);
    for run_id in [
        "run-update",
        "run-null",
        "run-delete-transition",
        "run-convert",
        "run-identity",
        "run-delete",
        "run-state",
    ] {
        insert_runtime_run(&database.store, run_id, &scope_key).await;
    }
    insert_runtime_transition(&database.store, "rt-update", "run-update", &scope_key, 0).await;
    insert_runtime_transition(&database.store, "rt-null", "run-null", &scope_key, 0).await;
    insert_runtime_transition(
        &database.store,
        "rt-delete",
        "run-delete-transition",
        &scope_key,
        0,
    )
    .await;
    insert_runtime_transition(&database.store, "rt-parent", "run-delete", &scope_key, 0).await;
    insert_legacy_transition(&database.store, "legacy-convert", &scope_key).await;
    insert_legacy_transition(&database.store, "legacy-fk", &scope_key).await;

    assert_sql_rejected(
        &database.store,
        "UPDATE transitions SET payload_json = '{\"changed\":true}' WHERE id = 'rt-update'",
    )
    .await;
    assert_sql_rejected(
        &database.store,
        "UPDATE transitions SET run_id = NULL WHERE id = 'rt-null'",
    )
    .await;
    assert_sql_rejected(
        &database.store,
        "DELETE FROM transitions WHERE id = 'rt-delete'",
    )
    .await;
    assert_sql_rejected(
        &database.store,
        "UPDATE transitions
         SET run_id = 'run-convert', step = 0, input_refs_json = '[]',
             output_refs_json = '[]', attempt = 1, deadline_at_us = 2
         WHERE id = 'legacy-convert'",
    )
    .await;
    assert_sql_rejected(
        &database.store,
        "UPDATE runtime_runs SET run_id = 'rewritten-run' WHERE run_id = 'run-identity'",
    )
    .await;
    sqlx::query("INSERT INTO runtime_runs (run_id, scope_key, scope_json, state, next_step, version, created_at_us, updated_at_us) VALUES ('run-scope', ?1, '{}', 'observing', 0, 0, 1, 1)")
        .bind(&scope_key)
        .execute(database.store.pool())
        .await
        .unwrap();
    let statement = "UPDATE runtime_runs SET scope_key = ?1 WHERE run_id = 'run-scope'";
    let error = sqlx::query(statement)
        .bind(&other_scope_key)
        .execute(database.store.pool())
        .await
        .expect_err("Runtime run Scope 不得改写");
    assert!(
        error
            .to_string()
            .contains("runtime run identity is immutable")
    );
    assert_sql_rejected(
        &database.store,
        "UPDATE runtime_runs SET scope_json = '{\"rewritten\":true}' WHERE run_id = 'run-identity'",
    )
    .await;
    assert_sql_rejected(
        &database.store,
        "DELETE FROM runtime_runs WHERE run_id = 'run-delete'",
    )
    .await;

    sqlx::query("UPDATE runtime_runs SET state = 'recalling', next_step = 1, version = 1 WHERE run_id = 'run-state'")
        .execute(database.store.pool())
        .await
        .expect("Runner 状态推进字段应允许更新");
    sqlx::query(
        "INSERT INTO approvals (
            id, scope_key, transition_id, principal_id, status, created_at_us
         ) VALUES ('legacy-approval', ?1, 'legacy-fk', 'principal', 'pending', 1)",
    )
    .bind(&scope_key)
    .execute(database.store.pool())
    .await
    .expect("旧 Transition 外键消费者应保持兼容");
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

async fn insert_runtime_run(store: &SqliteStore, run_id: &str, scope_key: &str) {
    sqlx::query(
        "INSERT INTO runtime_runs (
            run_id, scope_key, scope_json, state, next_step, version,
            created_at_us, updated_at_us
         ) VALUES (?1, ?2, '{}', 'observing', 0, 0, 1, 1)",
    )
    .bind(run_id)
    .bind(scope_key)
    .execute(store.pool())
    .await
    .expect("插入 Runtime run Fixture");
}

async fn insert_runtime_transition(
    store: &SqliteStore,
    id: &str,
    run_id: &str,
    scope_key: &str,
    step: i64,
) {
    sqlx::query(
        "INSERT INTO transitions (
            id, scope_key, trace_id, from_stage, to_stage, payload_json, created_at_us,
            run_id, step, input_refs_json, output_refs_json, attempt, deadline_at_us
         ) VALUES (?1, ?2, 'trace', 'observing', 'recalling', '{}', 1,
                   ?3, ?4, '[]', '[]', 1, 2)",
    )
    .bind(id)
    .bind(scope_key)
    .bind(run_id)
    .bind(step)
    .execute(store.pool())
    .await
    .expect("插入 Runtime Transition Fixture");
}

async fn insert_legacy_transition(store: &SqliteStore, id: &str, scope_key: &str) {
    sqlx::query(
        "INSERT INTO transitions (
            id, scope_key, trace_id, from_stage, to_stage, payload_json, created_at_us
         ) VALUES (?1, ?2, 'trace', 'legacy_from', 'legacy_to', '{}', 1)",
    )
    .bind(id)
    .bind(scope_key)
    .execute(store.pool())
    .await
    .expect("插入旧 Transition Fixture");
}

async fn assert_sql_rejected(store: &SqliteStore, statement: &str) {
    let error = sqlx::query(statement)
        .execute(store.pool())
        .await
        .expect_err("不可变 Runtime 行修改必须被拒绝");
    let message = error.to_string();
    assert!(
        message.contains("runtime transition is immutable")
            || message.contains("runtime run identity is immutable")
            || message.contains("runtime run cannot be deleted"),
        "非预期拒绝原因: {message}"
    );
}
