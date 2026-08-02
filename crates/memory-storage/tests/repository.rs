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
    let supporting = Harness::text_event(scope.clone(), "verified source", Vec::new());
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
    let draft = Harness::text_event(
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

    let evidence_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM evidence_refs WHERE owner_fragment_id = ?1")
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
    let draft = Harness::text_event(scope.clone(), "evidence", Vec::new());

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

    let mut conflicting = Harness::text_event(scope, "different", Vec::new());
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
async fn nanosecond_fragment_payload_remains_authoritative_across_idempotent_reads() {
    let harness = Harness::new().await;
    let occurred_at = DateTime::from_timestamp(1_725_000_000, 123_456_789)
        .expect("valid nanosecond fixture time");
    let mut draft = Harness::text_event_at(
        full_scope(),
        "nanosecond authoritative payload",
        occurred_at,
    );
    draft.recorded_at = fixed_time();

    let first = harness
        .repository
        .append(&draft)
        .await
        .expect("append nanosecond payload");
    let duplicate = harness
        .repository
        .append(&draft)
        .await
        .expect("idempotent nanosecond append");

    assert!(first.created);
    assert!(!duplicate.created);
    assert_eq!(duplicate.event.draft.payload, draft.payload);
    assert_eq!(
        harness
            .repository
            .get_fragment(draft.fragment_id)
            .await
            .expect("read authoritative nanosecond fragment"),
        draft.payload
    );
}

#[tokio::test]
async fn fragment_lookup_requires_the_exact_scope_before_decoding() {
    let harness = Harness::new().await;
    let scope = full_scope();
    let foreign_scope = MemoryScope::new(NamespaceId::new());
    let draft = Harness::text_event(scope.clone(), "scope-qualified fragment", Vec::new());
    harness
        .repository
        .append(&draft)
        .await
        .expect("append scoped fragment");

    assert_eq!(
        harness
            .repository
            .get_fragment_in_scope(&scope, draft.fragment_id)
            .await
            .expect("read exact scoped fragment"),
        draft.payload
    );
    assert!(matches!(
        harness
            .repository
            .get_fragment_in_scope(&foreign_scope, draft.fragment_id)
            .await,
        Err(StorageError::NotFound {
            entity: "fragment",
            ..
        })
    ));
}

#[tokio::test]
async fn batch_fragment_lookup_preserves_input_order_and_rejects_foreign_scope() {
    let harness = Harness::new().await;
    let scope = full_scope();
    let first = Harness::text_event(scope.clone(), "first scoped fragment", Vec::new());
    let second = Harness::text_event(scope.clone(), "second scoped fragment", Vec::new());
    let foreign = Harness::text_event(
        MemoryScope::new(NamespaceId::new()),
        "foreign scoped fragment",
        Vec::new(),
    );
    harness
        .repository
        .append_batch(&[first.clone(), second.clone(), foreign.clone()])
        .await
        .expect("append batch lookup fixtures");

    let ids = [second.fragment_id, first.fragment_id];
    let fragments = harness
        .repository
        .get_fragments_in_scope(&scope, &ids)
        .await
        .expect("batch read exact scoped fragments");
    assert_eq!(
        fragments
            .iter()
            .map(|fragment| fragment.id)
            .collect::<Vec<_>>(),
        ids
    );

    let error = harness
        .repository
        .get_fragments_in_scope(&scope, &[first.fragment_id, foreign.fragment_id])
        .await
        .expect_err("foreign fragment must not cross the requested scope");
    assert!(matches!(
        error,
        StorageError::NotFound {
            entity: "fragment",
            ..
        }
    ));
}

#[tokio::test]
async fn append_rejects_a_stage_that_disagrees_with_the_fragment() {
    let harness = Harness::new().await;
    let mut draft = Harness::text_event(full_scope(), "stage invariant", Vec::new());
    draft.stage = jc_memory_protocol::CognitiveStage::FutureHypothesis;

    assert!(matches!(
        harness.repository.append(&draft).await,
        Err(StorageError::InvalidDraft {
            reason: "stage does not match fragment kind and validity"
        })
    ));
    assert_eq!(harness.authoritative_counts().await, (0, 0, 0, 0, 0));
}

#[tokio::test]
async fn content_dedup_rejects_an_incompatible_storage_representation() {
    let harness = Harness::new().await;
    let scope = full_scope();
    let bytes = b"same representation bytes";
    let text = Harness::text_event(
        scope.clone(),
        std::str::from_utf8(bytes).expect("fixture UTF-8"),
        Vec::new(),
    );
    harness.repository.append(&text).await.expect("append text");
    let before = harness.authoritative_counts().await;
    let blob_id = BlobId::from_str(&hex::encode(Sha256::digest(bytes))).expect("valid blob ID");
    let blob = Harness::event(
        scope,
        FragmentContent::Blob {
            blob_id,
            media_type: "text/plain; charset=utf-8".to_owned(),
            byte_len: bytes.len() as u64,
        },
        Vec::new(),
    );

    assert!(matches!(
        harness.repository.append(&blob).await,
        Err(StorageError::IdentityConflict {
            entity: "fragment_content",
            ..
        })
    ));
    assert_eq!(harness.authoritative_counts().await, before);
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
    let draft = Harness::text_event(full_scope(), "must roll back", vec![invalid_evidence]);

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
        .map(|index| Harness::text_event(scope.clone(), &format!("event {index}"), Vec::new()))
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
    let other = Harness::text_event(other_scope, "other scope", Vec::new());
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
        let draft = Harness::text_event(
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
    let other = Harness::text_event(
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
async fn recent_candidates_respect_scope_cutoff_order_and_limit() {
    let harness = Harness::new().await;
    let scope = full_scope();
    let other_scope = MemoryScope::new(NamespaceId::new());
    let older = Harness::text_event_at(
        scope.clone(),
        "older bounded candidate",
        fixed_time() - Duration::days(2),
    );
    let newer = Harness::text_event_at(
        scope.clone(),
        "newer bounded candidate",
        fixed_time() + Duration::days(2),
    );
    let other = Harness::text_event_at(
        other_scope,
        "other scope candidate",
        fixed_time() - Duration::days(1),
    );
    for draft in [&older, &newer, &other] {
        harness
            .repository
            .append(draft)
            .await
            .expect("append bounded candidate");
    }

    let historical = harness
        .repository
        .recent_fragments(&scope, Some(fixed_time()), 10)
        .await
        .expect("load historical candidates");
    assert_eq!(
        historical
            .iter()
            .map(|hit| hit.fragment_id)
            .collect::<Vec<_>>(),
        vec![older.fragment_id]
    );

    let current = harness
        .repository
        .recent_fragments(&scope, None, 1)
        .await
        .expect("load current candidates");
    assert_eq!(current.len(), 1);
    assert_eq!(current[0].fragment_id, newer.fragment_id);
    assert!(
        harness
            .repository
            .recent_fragments(&scope, None, 0)
            .await
            .expect("honor zero candidate limit")
            .is_empty()
    );
}

#[tokio::test]
async fn recent_candidates_read_authoritative_content_without_fts_dependency() {
    let harness = Harness::new().await;
    let scope = full_scope();
    let draft = Harness::text_event(scope.clone(), "权威最近候选不能依赖派生 FTS", Vec::new());
    harness
        .repository
        .append(&draft)
        .await
        .expect("写入权威候选");
    sqlx::query("DELETE FROM fragment_fts WHERE fragment_id = ?1")
        .bind(draft.fragment_id.to_string())
        .execute(harness.store.pool())
        .await
        .expect("删除派生 FTS 行");

    let candidates = harness
        .repository
        .recent_fragments(&scope, None, 10)
        .await
        .expect("从权威内容读取最近候选");

    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].fragment_id, draft.fragment_id);
    assert_eq!(candidates[0].event_id, draft.id);
    assert_eq!(candidates[0].body, "权威最近候选不能依赖派生 FTS");
}

#[tokio::test]
async fn all_fragment_content_kinds_and_microsecond_times_round_trip() {
    let harness = Harness::new().await;
    let scope = full_scope();
    let blob_id =
        BlobId::from_str(&hex::encode(Sha256::digest(b"binary evidence"))).expect("valid blob ID");
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
        let draft = Harness::event(scope.clone(), content, Vec::new());
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

    let duplicate_content = Harness::text_event(scope, "round trip text", Vec::new());
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
    let draft = Harness::text_event(full_scope(), "enum corruption", Vec::new());
    harness
        .repository
        .append(&draft)
        .await
        .expect("append event");
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
    let draft = Harness::text_event(full_scope(), "json corruption", Vec::new());
    harness
        .repository
        .append(&draft)
        .await
        .expect("append event");
    let mut connection = harness
        .store
        .pool()
        .acquire()
        .await
        .expect("acquire connection");
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
    let draft = Harness::text_event(full_scope(), "number corruption", Vec::new());
    harness
        .repository
        .append(&draft)
        .await
        .expect("append event");
    let mut connection = harness
        .store
        .pool()
        .acquire()
        .await
        .expect("acquire connection");
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
    let draft = Harness::text_event(full_scope(), "reference corruption", Vec::new());
    harness
        .repository
        .append(&draft)
        .await
        .expect("append event");
    let content_id: String = sqlx::query_scalar("SELECT content_id FROM fragments WHERE id = ?1")
        .bind(draft.fragment_id.to_string())
        .fetch_one(harness.store.pool())
        .await
        .expect("read content ID");
    let mut connection = harness
        .store
        .pool()
        .acquire()
        .await
        .expect("acquire connection");
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
    let draft = Harness::text_event(full_scope(), "event corruption", Vec::new());
    harness
        .repository
        .append(&draft)
        .await
        .expect("append event");
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
        let config =
            SqliteConfig::new(directory.path().join("memory.sqlite3")).with_max_connections(12);
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

    fn text_event(scope: MemoryScope, text: &str, evidence: Vec<EvidenceRef>) -> EventDraft {
        Self::event(
            scope,
            FragmentContent::Text {
                text: text.to_owned(),
            },
            evidence,
        )
    }

    fn text_event_at(scope: MemoryScope, text: &str, occurred_at: DateTime<Utc>) -> EventDraft {
        let observed_at = occurred_at + Duration::microseconds(123);
        let fragment = CognitiveFragment {
            id: FragmentId::new(),
            scope,
            session_id: Some(SessionId::new()),
            trace_id: Some(TraceId::new()),
            kind: FragmentKind::Observation,
            content: FragmentContent::Text {
                text: text.to_owned(),
            },
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
            evidence: Vec::new(),
            metadata: serde_json::Map::new(),
        };
        let mut draft = EventFactory::new()
            .from_fragment(fragment)
            .expect("create event draft");
        draft.recorded_at = observed_at + Duration::microseconds(456);
        draft
    }

    fn event(
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
    sqlx::query_scalar(sqlx::AssertSqlSafe(statement))
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
        serde_json::to_vec(&serde_json::to_value(scope).expect("canonicalize scope"))
            .expect("serialize scope"),
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
