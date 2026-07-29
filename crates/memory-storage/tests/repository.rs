//! Transactional memory repository contract tests.

use std::{str::FromStr as _, sync::Arc};

use chrono::{DateTime, Duration, Utc};
use jc_memory_events::factory::EventFactory;
use jc_memory_protocol::{
    AgentId, BlobId, CognitiveFragment, EventDraft, EvidenceId, EvidenceRef, FragmentContent,
    FragmentId, FragmentKind, FragmentSource, MemoryScope, NamespaceId, Perspective, ProjectId,
    SecurityLabel, SessionId, SourceType, TenantId, TraceId, UserId, ValidityStatus,
    VerificationMethod,
};
use jc_memory_storage::{
    MemoryRepository, SqliteConfig, SqliteMemoryRepository, SqliteStore, StorageError,
};
use sha2::{Digest as _, Sha256};
use sqlx::Executor as _;
use tempfile::TempDir;
use tokio::sync::Barrier;

#[tokio::test]
async fn append_persists_fragment_event_evidence_and_fts_atomically() {
    let harness = Harness::new().await;
    let scope = full_scope();
    let supporting = harness.text_event(scope.clone(), "verified source", Vec::new());
    harness
        .repository
        .append(&supporting)
        .await
        .expect("append supporting fragment");
    let evidence = EvidenceRef {
        id: EvidenceId::new(),
        fragment_id: Some(supporting.fragment_id),
        event_id: None,
        method: VerificationMethod::Tool,
        note: Some("verified by deterministic fixture".to_owned()),
    };
    let draft = harness.text_event(
        scope.clone(),
        "2018 payment migration failed",
        vec![evidence],
    );

    let receipt = harness
        .repository
        .append(&draft)
        .await
        .expect("append observed event");

    assert!(receipt.created);
    assert_eq!(receipt.event.sequence, 1);
    assert_eq!(receipt.event.draft, draft);
    assert_eq!(
        harness
            .repository
            .get_fragment(draft.fragment_id)
            .await
            .expect("get fragment"),
        draft.payload
    );
    assert_eq!(
        harness
            .repository
            .get_event(draft.id)
            .await
            .expect("get event"),
        receipt.event
    );

    let hits = harness
        .repository
        .search_fts(&scope, "payment migration", 10)
        .await
        .expect("search FTS");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].fragment_id, draft.fragment_id);
    assert_eq!(hits[0].body, "2018 payment migration failed");

    let evidence_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM evidence_refs WHERE owner_fragment_id = ?1",
    )
    .bind(draft.fragment_id.to_string())
    .fetch_one(harness.store.pool())
    .await
    .expect("count evidence references");
    assert_eq!(evidence_count, 1);

    let stored_scope_key: String =
        sqlx::query_scalar("SELECT scope_key FROM fragments WHERE id = ?1")
            .bind(draft.fragment_id.to_string())
            .fetch_one(harness.store.pool())
            .await
            .expect("read scope key");
    assert_eq!(stored_scope_key, canonical_scope_key(&scope));
}

#[tokio::test]
async fn duplicate_event_is_idempotent_but_conflicting_payload_is_rejected() {
    let harness = Harness::new().await;
    let scope = full_scope();
    let draft = harness.text_event(scope.clone(), "evidence", Vec::new());

    let first = harness
        .repository
        .append(&draft)
        .await
        .expect("first append");
    let duplicate = harness
        .repository
        .append(&draft)
        .await
        .expect("idempotent append");
    assert!(first.created);
    assert!(!duplicate.created);
    assert_eq!(duplicate.event, first.event);

    let mut conflicting = harness.text_event(scope, "different", Vec::new());
    conflicting.id = draft.id;
    assert!(matches!(
        harness.repository.append(&conflicting).await,
        Err(StorageError::IdentityConflict {
            entity: "event",
            ..
        })
    ));
}

#[tokio::test]
async fn evidence_foreign_key_failure_rolls_back_every_derived_row() {
    let harness = Harness::new().await;
    let before = harness.authoritative_counts().await;
    let invalid_evidence = EvidenceRef {
        id: EvidenceId::new(),
        fragment_id: Some(FragmentId::new()),
        event_id: None,
        method: VerificationMethod::Manual,
        note: None,
    };
    let draft = harness.text_event(
        full_scope(),
        "must roll back",
        vec![invalid_evidence],
    );

    assert!(matches!(
        harness.repository.append(&draft).await,
        Err(StorageError::Database(_))
    ));
    assert_eq!(harness.authoritative_counts().await, before);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_appends_allocate_gapless_sequences_per_exact_scope() {
    let harness = Harness::new().await;
    let scope = full_scope();
    let repository = Arc::clone(&harness.repository);
    let barrier = Arc::new(Barrier::new(8));
    let drafts: Vec<_> = (0..8)
        .map(|index| harness.text_event(scope.clone(), &format!("event {index}"), Vec::new()))
        .collect();
    let mut tasks = Vec::new();

    for draft in drafts {
        let repository = Arc::clone(&repository);
        let barrier = Arc::clone(&barrier);
        tasks.push(tokio::spawn(async move {
            barrier.wait().await;
            repository.append(&draft).await
        }));
    }

    let mut sequences = Vec::new();
    for task in tasks {
        sequences.push(
            task.await
                .expect("join append task")
                .expect("append event")
                .event
                .sequence,
        );
    }
    sequences.sort_unstable();
    assert_eq!(sequences, (0..8).collect::<Vec<_>>());

    let other_scope = MemoryScope::new(NamespaceId::new());
    let other = harness.text_event(other_scope, "other scope", Vec::new());
    assert_eq!(
        harness
            .repository
            .append(&other)
            .await
            .expect("append other scope")
            .event
            .sequence,
        0
    );
}

#[tokio::test]
async fn fts_search_is_scope_bound_escaped_and_hard_limited() {
    let harness = Harness::new().await;
    let first_scope = full_scope();
    let second_scope = MemoryScope::new(NamespaceId::new());
    for index in 0..120 {
        let draft = harness.text_event(
            first_scope.clone(),
            &format!("payment migration memory {index}"),
            Vec::new(),
        );
        harness
            .repository
            .append(&draft)
            .await
            .expect("append searchable fragment");
    }
    let other = harness.text_event(
        second_scope.clone(),
        "payment migration memory external",
        Vec::new(),
    );
    harness
        .repository
        .append(&other)
        .await
        .expect("append other-scope fragment");

    let hits = harness
        .repository
        .search_fts(&first_scope, "payment migration", 1_000)
        .await
        .expect("search bounded scope");
    assert_eq!(hits.len(), 100);
    assert!(hits.iter().all(|hit| hit.scope == first_scope));
    assert!(
        harness
            .repository
            .search_fts(&first_scope, "payment OR external", 10)
            .await
            .expect("escape boolean query")
            .is_empty()
    );
    assert!(
        harness
            .repository
            .search_fts(&first_scope, "\" OR *", 10)
            .await
            .expect("escape malformed query")
            .is_empty()
    );
    assert!(
        harness
            .repository
            .search_fts(&second_scope, "payment migration", 10)
            .await
            .expect("search other scope")
            .iter()
            .all(|hit| hit.fragment_id == other.fragment_id)
    );
}

#[tokio::test]
async fn all_fragment_content_kinds_and_microsecond_times_round_trip() {
    let harness = Harness::new().await;
    let scope = full_scope();
    let blob_id = BlobId::from_str(&hex::encode(Sha256::digest(b"binary evidence")))
        .expect("valid blob ID");
    let contents = vec![
        FragmentContent::Text {
            text: "round trip text".to_owned(),
        },
        FragmentContent::Json {
            value: serde_json::json!({"nested": {"answer": 42}, "items": [1, 2, 3]}),
        },
        FragmentContent::Blob {
            blob_id,
            media_type: "application/octet-stream".to_owned(),
            byte_len: 15,
        },
    ];

    for content in contents {
        let draft = harness.event(scope.clone(), content, Vec::new());
        harness
            .repository
            .append(&draft)
            .await
            .expect("append content kind");
        assert_eq!(
            harness
                .repository
                .get_fragment(draft.fragment_id)
                .await
                .expect("round-trip fragment"),
            draft.payload
        );
        assert_eq!(
            harness
                .repository
                .get_event(draft.id)
                .await
                .expect("round-trip event")
                .draft,
            draft
        );
    }

    let duplicate_content = harness.text_event(scope, "round trip text", Vec::new());
    harness
        .repository
        .append(&duplicate_content)
        .await
        .expect("append duplicate content");
    let text_content_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM fragment_contents WHERE media_type = 'text/plain; charset=utf-8'",
    )
    .fetch_one(harness.store.pool())
    .await
    .expect("count deduplicated text content");
    assert_eq!(text_content_count, 1);
}

#[tokio::test]
async fn invalid_fragment_enum_is_reported_as_corrupt_row() {
    let harness = Harness::new().await;
    let draft = harness.text_event(full_scope(), "enum corruption", Vec::new());
    harness.repository.append(&draft).await.expect("append event");
    sqlx::query("UPDATE fragments SET kind = 'unknown_kind' WHERE id = ?1")
        .bind(draft.fragment_id.to_string())
        .execute(harness.store.pool())
        .await
        .expect("corrupt enum");

    assert_corrupt_fragment(
        harness.repository.get_fragment(draft.fragment_id).await,
        draft.fragment_id,
    );
}

#[tokio::test]
async fn invalid_fragment_json_is_reported_as_corrupt_row() {
    let harness = Harness::new().await;
    let draft = harness.text_event(full_scope(), "json corruption", Vec::new());
    harness.repository.append(&draft).await.expect("append event");
    let mut connection = harness.store.pool().acquire().await.expect("acquire connection");
    connection
        .execute("PRAGMA ignore_check_constraints = ON")
        .await
        .expect("disable checks");
    sqlx::query("UPDATE fragments SET source_json = '{' WHERE id = ?1")
        .bind(draft.fragment_id.to_string())
        .execute(&mut *connection)
        .await
        .expect("corrupt JSON");
    connection
        .execute("PRAGMA ignore_check_constraints = OFF")
        .await
        .expect("restore checks");
    drop(connection);

    assert_corrupt_fragment(
        harness.repository.get_fragment(draft.fragment_id).await,
        draft.fragment_id,
    );
}

#[tokio::test]
async fn out_of_range_fragment_number_is_reported_as_corrupt_row() {
    let harness = Harness::new().await;
    let draft = harness.text_event(full_scope(), "number corruption", Vec::new());
    harness.repository.append(&draft).await.expect("append event");
    let mut connection = harness.store.pool().acquire().await.expect("acquire connection");
    connection
        .execute("PRAGMA ignore_check_constraints = ON")
        .await
        .expect("disable checks");
    sqlx::query("UPDATE fragments SET confidence = 1.5 WHERE id = ?1")
        .bind(draft.fragment_id.to_string())
        .execute(&mut *connection)
        .await
        .expect("corrupt confidence");
    connection
        .execute("PRAGMA ignore_check_constraints = OFF")
        .await
        .expect("restore checks");
    drop(connection);

    assert_corrupt_fragment(
        harness.repository.get_fragment(draft.fragment_id).await,
        draft.fragment_id,
    );
}

#[tokio::test]
async fn missing_fragment_content_is_reported_as_corrupt_row() {
    let harness = Harness::new().await;
    let draft = harness.text_event(full_scope(), "reference corruption", Vec::new());
    harness.repository.append(&draft).await.expect("append event");
    let content_id: String = sqlx::query_scalar("SELECT content_id FROM fragments WHERE id = ?1")
        .bind(draft.fragment_id.to_string())
        .fetch_one(harness.store.pool())
        .await
        .expect("read content ID");
    let mut connection = harness.store.pool().acquire().await.expect("acquire connection");
    connection
        .execute("PRAGMA foreign_keys = OFF")
        .await
        .expect("disable foreign keys");
    sqlx::query("DELETE FROM fragment_contents WHERE id = ?1")
        .bind(content_id)
        .execute(&mut *connection)
        .await
        .expect("delete referenced content");
    connection
        .execute("PRAGMA foreign_keys = ON")
        .await
        .expect("restore foreign keys");
    drop(connection);

    assert_corrupt_fragment(
        harness.repository.get_fragment(draft.fragment_id).await,
        draft.fragment_id,
    );
}

#[tokio::test]
async fn invalid_event_stage_is_reported_as_corrupt_row() {
    let harness = Harness::new().await;
    let draft = harness.text_event(full_scope(), "event corruption", Vec::new());
    harness.repository.append(&draft).await.expect("append event");
    sqlx::query("UPDATE events SET stage = 'unknown_stage' WHERE id = ?1")
        .bind(draft.id.to_string())
        .execute(harness.store.pool())
        .await
        .expect("corrupt event stage");

    assert!(matches!(
        harness.repository.get_event(draft.id).await,
        Err(StorageError::CorruptRow {
            table: "events",
            ..
        })
    ));
}

struct Harness {
    _directory: TempDir,
    store: SqliteStore,
    repository: Arc<SqliteMemoryRepository>,
}

impl Harness {
    async fn new() -> Self {
        let directory = tempfile::tempdir().expect("create repository temp directory");
        let config = SqliteConfig::new(directory.path().join("memory.sqlite3"))
            .with_max_connections(12);
        let store = SqliteStore::open(&config)
            .await
            .expect("open repository database");
        let repository = Arc::new(SqliteMemoryRepository::from_store(&store));
        Self {
            _directory: directory,
            store,
            repository,
        }
    }

    fn text_event(&self, scope: MemoryScope, text: &str, evidence: Vec<EvidenceRef>) -> EventDraft {
        self.event(
            scope,
            FragmentContent::Text {
                text: text.to_owned(),
            },
            evidence,
        )
    }

    fn event(
        &self,
        scope: MemoryScope,
        content: FragmentContent,
        evidence: Vec<EvidenceRef>,
    ) -> EventDraft {
        let occurred_at = fixed_time();
        let observed_at = occurred_at + Duration::microseconds(123);
        let fragment = CognitiveFragment {
            id: FragmentId::new(),
            scope,
            session_id: Some(SessionId::new()),
            trace_id: Some(TraceId::new()),
            kind: FragmentKind::Observation,
            content,
            source: FragmentSource {
                source_type: SourceType::Tool,
                source_id: "repository-fixture".to_owned(),
                actor_id: Some("fixture-actor".to_owned()),
                observer_id: Some("fixture-observer".to_owned()),
                tool_id: Some("fixture-tool".to_owned()),
                uri: Some("memory://fixture".to_owned()),
            },
            perspective: Perspective::Tool,
            occurred_at,
            observed_at,
            confidence: 0.875,
            importance: 0.625,
            validity: ValidityStatus::Unverified,
            security_label: SecurityLabel::Internal,
            evidence,
            metadata: serde_json::Map::from_iter([(
                "fixture".to_owned(),
                serde_json::Value::Bool(true),
            )]),
        };
        let mut draft = EventFactory::new()
            .from_fragment(fragment)
            .expect("create event draft");
        draft.recorded_at = observed_at + Duration::microseconds(456);
        draft
    }

    async fn authoritative_counts(&self) -> (i64, i64, i64, i64, i64) {
        let fragments = table_count(self.store.pool(), "fragments").await;
        let contents = table_count(self.store.pool(), "fragment_contents").await;
        let events = table_count(self.store.pool(), "events").await;
        let evidence = table_count(self.store.pool(), "evidence_refs").await;
        let fts = table_count(self.store.pool(), "fragment_fts").await;
        (fragments, contents, events, evidence, fts)
    }
}

async fn table_count(pool: &sqlx::SqlitePool, table: &str) -> i64 {
    let statement = format!("SELECT COUNT(*) FROM {table}");
    sqlx::query_scalar(&statement)
        .fetch_one(pool)
        .await
        .expect("count table rows")
}

fn full_scope() -> MemoryScope {
    let mut scope = MemoryScope::new(NamespaceId::new());
    scope.tenant_id = Some(TenantId::new());
    scope.user_id = Some(UserId::new());
    scope.agent_id = Some(AgentId::new());
    scope.project_id = Some(ProjectId::new());
    scope
}

fn fixed_time() -> DateTime<Utc> {
    DateTime::from_timestamp_micros(1_725_000_000_123_456).expect("valid fixture time")
}

fn canonical_scope_key(scope: &MemoryScope) -> String {
    hex::encode(Sha256::digest(
        serde_json::to_vec(scope).expect("serialize scope"),
    ))
}

fn assert_corrupt_fragment(
    result: Result<CognitiveFragment, StorageError>,
    expected_id: FragmentId,
) {
    match result {
        Err(StorageError::CorruptRow {
            table: "fragments",
            id,
            reason,
        }) => {
            assert_eq!(id, expected_id.to_string());
            assert!(!reason.is_empty());
        }
        other => panic!("expected corrupt fragment row, got {other:?}"),
    }
}
