use std::{
    fmt::{self, Display},
    str::FromStr as _,
    sync::Arc,
    time::Duration,
};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use jc_memory_events::validation::validate_fragment;
use jc_memory_protocol::{
    AgentId, BlobId, CognitiveFragment, CognitiveStage, EventDraft, EventId, EvidenceId,
    EvidenceRef, FragmentContent, FragmentId, FragmentKind, FragmentSource, MemoryEvent,
    MemoryScope, NamespaceId, Perspective, ProjectId, SecurityLabel, SessionId, TenantId, TraceId,
    UserId, ValidityStatus, VerificationMethod,
};
use serde_json::Value;
use sha2::{Digest as _, Sha256};
use sqlx::{Decode, Row, Sqlite, SqlitePool, Transaction, Type, sqlite::SqliteRow};

use crate::{SqliteStore, StorageError};

const MAX_SEARCH_LIMIT: u32 = 100;
const MAX_BUSY_ATTEMPTS: u32 = 3;
const TEXT_MEDIA_TYPE: &str = "text/plain; charset=utf-8";
const JSON_MEDIA_TYPE: &str = "application/json";

fn busy_retry_delay(event_id: EventId, attempt: u32) -> Duration {
    let bytes = event_id.as_uuid().as_bytes();
    let seed = u64::from(u16::from_le_bytes([bytes[0], bytes[1]]));
    let jitter_ms = (seed + u64::from(attempt)) % 5;
    Duration::from_millis(5 * (u64::from(attempt) + 1) + jitter_ms)
}

/// Result of atomically appending an immutable event draft.
#[derive(Clone, Debug, PartialEq)]
pub struct AppendReceipt {
    /// Persisted event with its exact-scope sequence.
    pub event: MemoryEvent,
    /// Whether this call created the event rather than observing an idempotent duplicate.
    pub created: bool,
}

/// One scope-filtered full-text search result.
#[derive(Clone, Debug, PartialEq)]
pub struct FragmentSearchHit {
    /// 承载该 Fragment 的权威 Event 标识。
    pub event_id: EventId,
    /// Fragment matched by the FTS index.
    pub fragment_id: FragmentId,
    /// Exact memory scope used for the search.
    pub scope: MemoryScope,
    /// Indexed text returned by FTS5.
    pub body: String,
    /// FTS5 BM25 rank, where lower values sort first.
    pub rank: f64,
}

/// 一个精确 Scope 内的权威存储统计。
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RepositoryStats {
    /// 已持久化的 Fragment 数量。
    pub fragment_count: u64,
    /// 已持久化的 Event 数量。
    pub event_count: u64,
}

/// Durable operations for immutable memory fragments and events.
#[async_trait]
pub trait MemoryRepository: Send + Sync {
    /// Atomically persists one validated event draft and allocates its scope sequence.
    async fn append(&self, draft: &EventDraft) -> Result<AppendReceipt, StorageError>;

    /// Reconstructs a fragment from normalized authoritative rows.
    async fn get_fragment(&self, id: FragmentId) -> Result<CognitiveFragment, StorageError>;

    /// Reconstructs a persisted event and its normalized fragment payload.
    async fn get_event(&self, id: EventId) -> Result<MemoryEvent, StorageError>;

    /// Searches literal text within one exact memory scope.
    async fn search_fts(
        &self,
        scope: &MemoryScope,
        query: &str,
        limit: u32,
    ) -> Result<Vec<FragmentSearchHit>, StorageError>;

    /// 在一个精确 Scope 和可选历史截止时间内读取最近的有界文本候选。
    async fn recent_fragments(
        &self,
        scope: &MemoryScope,
        effective_at: Option<DateTime<Utc>>,
        limit: u32,
    ) -> Result<Vec<FragmentSearchHit>, StorageError>;

    /// 统计一个精确 Scope 内的权威 Fragment 和 Event。
    async fn stats(&self, scope: &MemoryScope) -> Result<RepositoryStats, StorageError>;
}

/// Injectable boundary used to prove transaction rollback before durable commit.
pub trait RepositoryFailpoint: Send + Sync {
    /// Runs after all rows are written but before the transaction commits.
    ///
    /// # Errors
    ///
    /// Returning an error aborts and rolls back the write transaction.
    fn before_commit(&self) -> Result<(), StorageError>;
}

/// `SQLite` implementation of [`MemoryRepository`].
#[derive(Clone)]
pub struct SqliteMemoryRepository {
    pool: SqlitePool,
    failpoint: Option<Arc<dyn RepositoryFailpoint>>,
}

impl fmt::Debug for SqliteMemoryRepository {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SqliteMemoryRepository")
            .field("pool", &self.pool)
            .field("failpoint_configured", &self.failpoint.is_some())
            .finish()
    }
}

impl SqliteMemoryRepository {
    /// Creates a repository from an existing `SQLx` `SQLite` pool.
    #[must_use]
    pub const fn new(pool: SqlitePool) -> Self {
        Self {
            pool,
            failpoint: None,
        }
    }

    /// Creates a repository sharing the pool owned by `store`.
    #[must_use]
    pub fn from_store(store: &SqliteStore) -> Self {
        Self::new(store.pool().clone())
    }

    /// Installs a deterministic pre-commit failure hook.
    #[must_use]
    pub fn with_failpoint(mut self, failpoint: Arc<dyn RepositoryFailpoint>) -> Self {
        self.failpoint = Some(failpoint);
        self
    }

    async fn append_once(&self, draft: &EventDraft) -> Result<AppendReceipt, StorageError> {
        let scope_key = scope_key(&draft.scope)?;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;

        let existing_hash: Option<String> =
            sqlx::query_scalar("SELECT content_hash FROM events WHERE id = ?1")
                .bind(draft.id.to_string())
                .fetch_optional(&mut *transaction)
                .await?;
        if let Some(existing_hash) = existing_hash {
            if existing_hash != draft.content_hash.as_str() {
                transaction.rollback().await?;
                return Err(StorageError::IdentityConflict {
                    entity: "event",
                    id: draft.id.to_string(),
                });
            }
            transaction.commit().await?;
            return Ok(AppendReceipt {
                event: self.get_event(draft.id).await?,
                created: false,
            });
        }

        let existing_fragment_event = sqlx::query(
            "SELECT id, content_hash FROM events
             WHERE fragment_id = ?1 ORDER BY sequence LIMIT 1",
        )
        .bind(draft.fragment_id.to_string())
        .fetch_optional(&mut *transaction)
        .await?;
        if let Some(row) = existing_fragment_event {
            let event_id_raw: String = row.try_get("id")?;
            let existing_hash: String = row.try_get("content_hash")?;
            if existing_hash != draft.content_hash.as_str() {
                transaction.rollback().await?;
                return Err(StorageError::IdentityConflict {
                    entity: "fragment",
                    id: draft.fragment_id.to_string(),
                });
            }
            let event_id = parse_id::<EventId>(&event_id_raw, "events", &event_id_raw, "id")?;
            transaction.commit().await?;
            return Ok(AppendReceipt {
                event: self.get_event(event_id).await?,
                created: false,
            });
        }

        match insert_draft(&mut transaction, draft, &scope_key).await {
            Ok(sequence) => {
                if let Some(failpoint) = &self.failpoint
                    && let Err(error) = failpoint.before_commit()
                {
                    transaction.rollback().await?;
                    return Err(error);
                }
                transaction.commit().await?;
                Ok(AppendReceipt {
                    event: MemoryEvent {
                        draft: draft.clone(),
                        sequence,
                    },
                    created: true,
                })
            }
            Err(error) => {
                transaction.rollback().await?;
                Err(error)
            }
        }
    }
}

#[async_trait]
impl MemoryRepository for SqliteMemoryRepository {
    async fn append(&self, draft: &EventDraft) -> Result<AppendReceipt, StorageError> {
        validate_draft(draft)?;
        for attempt in 0..MAX_BUSY_ATTEMPTS {
            match self.append_once(draft).await {
                Err(error) if error.is_busy_or_locked() && attempt + 1 < MAX_BUSY_ATTEMPTS => {
                    tokio::time::sleep(busy_retry_delay(draft.id, attempt)).await;
                }
                result => return result,
            }
        }
        unreachable!("bounded append loop always returns on its final attempt")
    }

    async fn get_fragment(&self, id: FragmentId) -> Result<CognitiveFragment, StorageError> {
        let id_string = id.to_string();
        let row = sqlx::query(
            "SELECT
                f.id, f.scope_key, f.namespace_id, f.tenant_id, f.user_id, f.agent_id,
                f.project_id, f.session_id, f.trace_id, f.kind, f.content_id,
                f.source_json, f.perspective, f.occurred_at_us, f.observed_at_us,
                f.confidence, f.importance, f.validity, f.security_label, f.metadata_json,
                c.id AS stored_content_id, c.storage_kind, c.inline_value, c.blob_id,
                c.media_type, c.byte_len AS content_byte_len, c.sha256 AS content_sha256
             FROM fragments AS f
             LEFT JOIN fragment_contents AS c ON c.id = f.content_id
             WHERE f.id = ?1",
        )
        .bind(&id_string)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| StorageError::NotFound {
            entity: "fragment",
            id: id_string.clone(),
        })?;

        decode_fragment(&self.pool, &row, &id_string).await
    }

    async fn get_event(&self, id: EventId) -> Result<MemoryEvent, StorageError> {
        let id_string = id.to_string();
        let row = sqlx::query(
            "SELECT id, fragment_id, scope_key, namespace_id, tenant_id, stage, sequence,
                    recorded_at_us, content_hash, payload_json
             FROM events WHERE id = ?1",
        )
        .bind(&id_string)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| StorageError::NotFound {
            entity: "event",
            id: id_string.clone(),
        })?;

        let fragment_id = parse_id::<FragmentId>(
            &column::<String>(&row, "fragment_id", "events", &id_string)?,
            "events",
            &id_string,
            "fragment_id",
        )?;
        let fragment = self.get_fragment(fragment_id).await?;
        decode_event(&row, id, fragment_id, fragment, &id_string)
    }

    async fn search_fts(
        &self,
        scope: &MemoryScope,
        query: &str,
        limit: u32,
    ) -> Result<Vec<FragmentSearchHit>, StorageError> {
        let query = query.trim();
        let limit = limit.min(MAX_SEARCH_LIMIT);
        if query.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }

        let scope_key = scope_key(scope)?;
        let escaped_query = format!("\"{}\"", query.replace('"', "\"\""));
        let rows = sqlx::query(
            "SELECT fragment_fts.fragment_id, events.id AS event_id,
                    fragment_fts.body, bm25(fragment_fts) AS rank
             FROM fragment_fts
             JOIN events ON events.fragment_id = fragment_fts.fragment_id
             WHERE fragment_fts MATCH ?1 AND fragment_fts.scope_key = ?2
             ORDER BY rank, fragment_fts.fragment_id
             LIMIT ?3",
        )
        .bind(escaped_query)
        .bind(scope_key)
        .bind(i64::from(limit))
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter()
            .map(|row| {
                let raw_id: String = row.try_get("fragment_id").map_err(|error| {
                    corrupt(
                        "fragment_fts",
                        "unknown",
                        format!("invalid fragment_id: {error}"),
                    )
                })?;
                let fragment_id =
                    parse_id::<FragmentId>(&raw_id, "fragment_fts", &raw_id, "fragment_id")?;
                let raw_event_id: String = row.try_get("event_id").map_err(|error| {
                    corrupt(
                        "fragment_fts",
                        &raw_id,
                        format!("invalid event_id: {error}"),
                    )
                })?;
                let event_id = parse_id::<EventId>(&raw_event_id, "events", &raw_event_id, "id")?;
                let body: String = row.try_get("body").map_err(|error| {
                    corrupt("fragment_fts", &raw_id, format!("invalid body: {error}"))
                })?;
                let rank: f64 = row.try_get("rank").map_err(|error| {
                    corrupt("fragment_fts", &raw_id, format!("invalid rank: {error}"))
                })?;
                if !rank.is_finite() {
                    return Err(corrupt("fragment_fts", &raw_id, "rank is not finite"));
                }
                Ok(FragmentSearchHit {
                    event_id,
                    fragment_id,
                    scope: scope.clone(),
                    body,
                    rank,
                })
            })
            .collect()
    }

    async fn recent_fragments(
        &self,
        scope: &MemoryScope,
        effective_at: Option<DateTime<Utc>>,
        limit: u32,
    ) -> Result<Vec<FragmentSearchHit>, StorageError> {
        let limit = limit.min(MAX_SEARCH_LIMIT);
        if limit == 0 {
            return Ok(Vec::new());
        }

        let scope_key = scope_key(scope)?;
        let effective_at_us = effective_at.map_or(i64::MAX, |value| value.timestamp_micros());
        let rows = sqlx::query(
            "SELECT fragments.id AS fragment_id, events.id AS event_id, fragment_fts.body
             FROM fragments INDEXED BY idx_fragments_scope_occurred
             JOIN fragment_fts ON fragment_fts.fragment_id = fragments.id
             JOIN events ON events.fragment_id = fragments.id
             WHERE fragments.scope_key = ?1 AND fragment_fts.scope_key = ?1
                   AND fragments.occurred_at_us <= ?2
             ORDER BY fragments.occurred_at_us DESC, fragments.id
             LIMIT ?3",
        )
        .bind(scope_key)
        .bind(effective_at_us)
        .bind(i64::from(limit))
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter()
            .map(|row| {
                let raw_id: String = row.try_get("fragment_id").map_err(|error| {
                    corrupt(
                        "fragment_fts",
                        "unknown",
                        format!("invalid fragment_id: {error}"),
                    )
                })?;
                let fragment_id =
                    parse_id::<FragmentId>(&raw_id, "fragment_fts", &raw_id, "fragment_id")?;
                let raw_event_id: String = row.try_get("event_id").map_err(|error| {
                    corrupt("events", &raw_id, format!("invalid event_id: {error}"))
                })?;
                let event_id = parse_id::<EventId>(&raw_event_id, "events", &raw_event_id, "id")?;
                let body: String = row.try_get("body").map_err(|error| {
                    corrupt("fragment_fts", &raw_id, format!("invalid body: {error}"))
                })?;
                Ok(FragmentSearchHit {
                    event_id,
                    fragment_id,
                    scope: scope.clone(),
                    body,
                    rank: 0.0,
                })
            })
            .collect()
    }

    async fn stats(&self, scope: &MemoryScope) -> Result<RepositoryStats, StorageError> {
        let scope_key = scope_key(scope)?;
        let fragment_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM fragments WHERE scope_key = ?1")
                .bind(&scope_key)
                .fetch_one(&self.pool)
                .await?;
        let event_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM events WHERE scope_key = ?1")
                .bind(scope_key)
                .fetch_one(&self.pool)
                .await?;
        Ok(RepositoryStats {
            fragment_count: u64::try_from(fragment_count).map_err(|error| {
                corrupt("fragments", "count", format!("invalid count: {error}"))
            })?,
            event_count: u64::try_from(event_count)
                .map_err(|error| corrupt("events", "count", format!("invalid count: {error}")))?,
        })
    }
}

fn decode_event(
    row: &SqliteRow,
    id: EventId,
    fragment_id: FragmentId,
    fragment: CognitiveFragment,
    row_id: &str,
) -> Result<MemoryEvent, StorageError> {
    let payload_json = column::<String>(row, "payload_json", "events", row_id)?;
    let payload: CognitiveFragment = serde_json::from_str(&payload_json)
        .map_err(|error| corrupt("events", row_id, format!("invalid payload_json: {error}")))?;
    if payload != fragment {
        return Err(corrupt(
            "events",
            row_id,
            "payload_json differs from normalized fragment rows",
        ));
    }
    validate_event_scope(row, &fragment.scope, row_id)?;

    let content_hash = parse_id::<BlobId>(
        &column::<String>(row, "content_hash", "events", row_id)?,
        "events",
        row_id,
        "content_hash",
    )?;
    let expected_hash = canonical_json_digest(&fragment)
        .map_err(|error| corrupt("events", row_id, error.to_string()))?;
    if content_hash != expected_hash {
        return Err(corrupt(
            "events",
            row_id,
            "content_hash does not match normalized payload",
        ));
    }

    let sequence = nonnegative_u64(
        column::<i64>(row, "sequence", "events", row_id)?,
        "events",
        row_id,
        "sequence",
    )?;
    let recorded_at = decode_time(
        column::<i64>(row, "recorded_at_us", "events", row_id)?,
        "events",
        row_id,
        "recorded_at_us",
    )?;
    let stage = decode_stage(&column::<String>(row, "stage", "events", row_id)?)
        .ok_or_else(|| corrupt("events", row_id, "unknown cognitive stage"))?;
    Ok(MemoryEvent {
        draft: EventDraft {
            id,
            fragment_id,
            scope: fragment.scope.clone(),
            stage,
            recorded_at,
            content_hash,
            payload: fragment,
        },
        sequence,
    })
}

fn validate_event_scope(
    row: &SqliteRow,
    scope: &MemoryScope,
    row_id: &str,
) -> Result<(), StorageError> {
    let stored_scope_key = column::<String>(row, "scope_key", "events", row_id)?;
    let expected_scope_key =
        scope_key(scope).map_err(|error| corrupt("events", row_id, error.to_string()))?;
    if stored_scope_key != expected_scope_key {
        return Err(corrupt(
            "events",
            row_id,
            "scope_key does not match the canonical event scope",
        ));
    }
    if column::<String>(row, "namespace_id", "events", row_id)? != scope.namespace_id.to_string() {
        return Err(corrupt(
            "events",
            row_id,
            "namespace_id differs from payload scope",
        ));
    }
    let tenant_id = column::<Option<String>>(row, "tenant_id", "events", row_id)?;
    if tenant_id != scope.tenant_id.map(|value| value.to_string()) {
        return Err(corrupt(
            "events",
            row_id,
            "tenant_id differs from payload scope",
        ));
    }
    Ok(())
}

async fn insert_draft(
    transaction: &mut Transaction<'_, Sqlite>,
    draft: &EventDraft,
    scope_key: &str,
) -> Result<u64, StorageError> {
    let sequence = allocate_sequence(transaction, scope_key).await?;
    let content = content_record(&draft.payload.content)?;
    let stored_content_id = insert_content(transaction, &content, draft.recorded_at).await?;
    insert_fragment(transaction, draft, scope_key, &stored_content_id).await?;
    insert_blob_reference(transaction, draft).await?;
    insert_event(transaction, draft, scope_key, sequence).await?;
    insert_evidence(transaction, draft).await?;
    insert_fts(transaction, draft, scope_key).await?;
    Ok(sequence)
}

async fn insert_blob_reference(
    transaction: &mut Transaction<'_, Sqlite>,
    draft: &EventDraft,
) -> Result<(), StorageError> {
    let FragmentContent::Blob { blob_id, .. } = &draft.payload.content else {
        return Ok(());
    };
    let recorded_at_us = draft.recorded_at.timestamp_micros();
    sqlx::query(
        "INSERT INTO blob_refs (
            blob_id, owner_type, owner_id, ref_count, created_at_us, updated_at_us
         ) VALUES (?1, 'fragment', ?2, 1, ?3, ?3)",
    )
    .bind(blob_id.as_str())
    .bind(draft.fragment_id.to_string())
    .bind(recorded_at_us)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn allocate_sequence(
    transaction: &mut Transaction<'_, Sqlite>,
    scope_key: &str,
) -> Result<u64, StorageError> {
    let sequence: i64 = sqlx::query_scalar(
        "INSERT INTO scope_sequences (scope_key, next_sequence) VALUES (?1, 1)
         ON CONFLICT(scope_key) DO UPDATE
            SET next_sequence = scope_sequences.next_sequence + 1
         RETURNING next_sequence - 1",
    )
    .bind(scope_key)
    .fetch_one(&mut **transaction)
    .await?;
    u64::try_from(sequence).map_err(|_| StorageError::SequenceOverflow {
        scope_key: scope_key.to_owned(),
    })
}

async fn insert_content(
    transaction: &mut Transaction<'_, Sqlite>,
    content: &ContentRecord,
    created_at: DateTime<Utc>,
) -> Result<String, StorageError> {
    sqlx::query(
        "INSERT INTO fragment_contents (
            id, storage_kind, inline_value, blob_id, media_type, byte_len, sha256, created_at_us
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(sha256, media_type) DO NOTHING",
    )
    .bind(&content.id)
    .bind(content.storage_kind)
    .bind(&content.inline_value)
    .bind(&content.blob_id)
    .bind(&content.media_type)
    .bind(content.byte_len)
    .bind(&content.sha256)
    .bind(created_at.timestamp_micros())
    .execute(&mut **transaction)
    .await?;
    let row = sqlx::query(
        "SELECT id, storage_kind, inline_value, blob_id, byte_len
         FROM fragment_contents WHERE sha256 = ?1 AND media_type = ?2",
    )
    .bind(&content.sha256)
    .bind(&content.media_type)
    .fetch_one(&mut **transaction)
    .await?;
    let stored_id: String = row.try_get("id")?;
    let compatible = row.try_get::<String, _>("storage_kind")? == content.storage_kind
        && row.try_get::<Option<Vec<u8>>, _>("inline_value")? == content.inline_value
        && row.try_get::<Option<String>, _>("blob_id")? == content.blob_id
        && row.try_get::<i64, _>("byte_len")? == content.byte_len
        && stored_id == content.id;
    if !compatible {
        return Err(StorageError::IdentityConflict {
            entity: "fragment_content",
            id: content.sha256.clone(),
        });
    }
    Ok(stored_id)
}

async fn insert_fragment(
    transaction: &mut Transaction<'_, Sqlite>,
    draft: &EventDraft,
    scope_key: &str,
    stored_content_id: &str,
) -> Result<(), StorageError> {
    let fragment = &draft.payload;
    sqlx::query(
        "INSERT INTO fragments (
            id, scope_key, namespace_id, tenant_id, user_id, agent_id, project_id,
            session_id, trace_id, kind, content_id, source_json, perspective,
            occurred_at_us, observed_at_us, confidence, importance, validity,
            security_label, metadata_json, created_at_us
         ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
            ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21
         )",
    )
    .bind(fragment.id.to_string())
    .bind(scope_key)
    .bind(fragment.scope.namespace_id.to_string())
    .bind(fragment.scope.tenant_id.map(|value| value.to_string()))
    .bind(fragment.scope.user_id.map(|value| value.to_string()))
    .bind(fragment.scope.agent_id.map(|value| value.to_string()))
    .bind(fragment.scope.project_id.map(|value| value.to_string()))
    .bind(fragment.session_id.map(|value| value.to_string()))
    .bind(fragment.trace_id.map(|value| value.to_string()))
    .bind(encode_kind(fragment.kind))
    .bind(stored_content_id)
    .bind(serde_json::to_string(&fragment.source)?)
    .bind(encode_perspective(fragment.perspective))
    .bind(fragment.occurred_at.timestamp_micros())
    .bind(fragment.observed_at.timestamp_micros())
    .bind(f64::from(fragment.confidence))
    .bind(f64::from(fragment.importance))
    .bind(encode_validity(fragment.validity))
    .bind(encode_security_label(fragment.security_label))
    .bind(serde_json::to_string(&fragment.metadata)?)
    .bind(draft.recorded_at.timestamp_micros())
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn insert_event(
    transaction: &mut Transaction<'_, Sqlite>,
    draft: &EventDraft,
    scope_key: &str,
    sequence: u64,
) -> Result<(), StorageError> {
    sqlx::query(
        "INSERT INTO events (
            id, fragment_id, scope_key, namespace_id, tenant_id, stage, sequence,
            recorded_at_us, content_hash, payload_json
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
    )
    .bind(draft.id.to_string())
    .bind(draft.fragment_id.to_string())
    .bind(scope_key)
    .bind(draft.scope.namespace_id.to_string())
    .bind(draft.scope.tenant_id.map(|value| value.to_string()))
    .bind(encode_stage(draft.stage))
    .bind(
        i64::try_from(sequence).map_err(|_| StorageError::SequenceOverflow {
            scope_key: scope_key.to_owned(),
        })?,
    )
    .bind(draft.recorded_at.timestamp_micros())
    .bind(draft.content_hash.as_str())
    .bind(serde_json::to_string(&draft.payload)?)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn insert_evidence(
    transaction: &mut Transaction<'_, Sqlite>,
    draft: &EventDraft,
) -> Result<(), StorageError> {
    for evidence in &draft.payload.evidence {
        sqlx::query(
            "INSERT INTO evidence_refs (
                id, owner_fragment_id, referenced_fragment_id, referenced_event_id,
                verification_method, note, created_at_us
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )
        .bind(evidence.id.to_string())
        .bind(draft.fragment_id.to_string())
        .bind(evidence.fragment_id.map(|value| value.to_string()))
        .bind(evidence.event_id.map(|value| value.to_string()))
        .bind(encode_verification_method(evidence.method))
        .bind(&evidence.note)
        .bind(draft.recorded_at.timestamp_micros())
        .execute(&mut **transaction)
        .await?;
    }
    Ok(())
}

async fn insert_fts(
    transaction: &mut Transaction<'_, Sqlite>,
    draft: &EventDraft,
    scope_key: &str,
) -> Result<(), StorageError> {
    if let Some(body) = searchable_body(&draft.payload.content)? {
        sqlx::query("INSERT INTO fragment_fts (fragment_id, scope_key, body) VALUES (?1, ?2, ?3)")
            .bind(draft.fragment_id.to_string())
            .bind(scope_key)
            .bind(body)
            .execute(&mut **transaction)
            .await?;
    }
    Ok(())
}

async fn decode_fragment(
    pool: &SqlitePool,
    row: &SqliteRow,
    row_id: &str,
) -> Result<CognitiveFragment, StorageError> {
    let stored_content_id =
        column::<Option<String>>(row, "stored_content_id", "fragments", row_id)?.ok_or_else(
            || {
                corrupt(
                    "fragments",
                    row_id,
                    "referenced fragment content is missing",
                )
            },
        )?;
    let declared_content_id = column::<String>(row, "content_id", "fragments", row_id)?;
    if stored_content_id != declared_content_id {
        return Err(corrupt(
            "fragments",
            row_id,
            "joined content does not match content_id",
        ));
    }

    let scope = decode_scope(row, row_id)?;
    let stored_scope_key = column::<String>(row, "scope_key", "fragments", row_id)?;
    let expected_scope_key =
        scope_key(&scope).map_err(|error| corrupt("fragments", row_id, error.to_string()))?;
    if stored_scope_key != expected_scope_key {
        return Err(corrupt(
            "fragments",
            row_id,
            "scope_key does not match canonical scope",
        ));
    }

    let content = decode_content(row, row_id, &stored_content_id)?;
    let (source, metadata) = decode_source_and_metadata(row, row_id)?;
    let scalars = decode_fragment_scalars(row, row_id)?;
    let evidence = decode_evidence(pool, row_id).await?;
    let fragment = CognitiveFragment {
        id: parse_id::<FragmentId>(
            &column::<String>(row, "id", "fragments", row_id)?,
            "fragments",
            row_id,
            "id",
        )?,
        scope,
        session_id: parse_optional_id::<SessionId>(
            column::<Option<String>>(row, "session_id", "fragments", row_id)?,
            "fragments",
            row_id,
            "session_id",
        )?,
        trace_id: parse_optional_id::<TraceId>(
            column::<Option<String>>(row, "trace_id", "fragments", row_id)?,
            "fragments",
            row_id,
            "trace_id",
        )?,
        kind: scalars.kind,
        content,
        source,
        perspective: scalars.perspective,
        occurred_at: scalars.occurred_at,
        observed_at: scalars.observed_at,
        confidence: scalars.confidence,
        importance: scalars.importance,
        validity: scalars.validity,
        security_label: scalars.security_label,
        evidence,
        metadata,
    };
    validate_fragment(&fragment)
        .map_err(|error| corrupt("fragments", row_id, error.to_string()))?;
    Ok(fragment)
}

struct FragmentScalars {
    kind: FragmentKind,
    perspective: Perspective,
    occurred_at: DateTime<Utc>,
    observed_at: DateTime<Utc>,
    confidence: f32,
    importance: f32,
    validity: ValidityStatus,
    security_label: SecurityLabel,
}

fn decode_fragment_scalars(row: &SqliteRow, row_id: &str) -> Result<FragmentScalars, StorageError> {
    let kind = decode_kind(&column::<String>(row, "kind", "fragments", row_id)?)
        .ok_or_else(|| corrupt("fragments", row_id, "unknown fragment kind"))?;
    let perspective =
        decode_perspective(&column::<String>(row, "perspective", "fragments", row_id)?)
            .ok_or_else(|| corrupt("fragments", row_id, "unknown perspective"))?;
    let validity = decode_validity(&column::<String>(row, "validity", "fragments", row_id)?)
        .ok_or_else(|| corrupt("fragments", row_id, "unknown validity status"))?;
    let security_label = decode_security_label(&column::<String>(
        row,
        "security_label",
        "fragments",
        row_id,
    )?)
    .ok_or_else(|| corrupt("fragments", row_id, "unknown security label"))?;
    Ok(FragmentScalars {
        kind,
        perspective,
        occurred_at: decode_time(
            column::<i64>(row, "occurred_at_us", "fragments", row_id)?,
            "fragments",
            row_id,
            "occurred_at_us",
        )?,
        observed_at: decode_time(
            column::<i64>(row, "observed_at_us", "fragments", row_id)?,
            "fragments",
            row_id,
            "observed_at_us",
        )?,
        confidence: unit_f32(
            column::<f64>(row, "confidence", "fragments", row_id)?,
            "fragments",
            row_id,
            "confidence",
        )?,
        importance: unit_f32(
            column::<f64>(row, "importance", "fragments", row_id)?,
            "fragments",
            row_id,
            "importance",
        )?,
        validity,
        security_label,
    })
}

fn decode_source_and_metadata(
    row: &SqliteRow,
    row_id: &str,
) -> Result<(FragmentSource, serde_json::Map<String, Value>), StorageError> {
    let source_json = column::<String>(row, "source_json", "fragments", row_id)?;
    let source = serde_json::from_str(&source_json)
        .map_err(|error| corrupt("fragments", row_id, format!("invalid source_json: {error}")))?;
    let metadata_json = column::<String>(row, "metadata_json", "fragments", row_id)?;
    let metadata_value: Value = serde_json::from_str(&metadata_json).map_err(|error| {
        corrupt(
            "fragments",
            row_id,
            format!("invalid metadata_json: {error}"),
        )
    })?;
    let Value::Object(metadata) = metadata_value else {
        return Err(corrupt(
            "fragments",
            row_id,
            "metadata_json is not an object",
        ));
    };
    Ok((source, metadata))
}

fn decode_scope(row: &SqliteRow, row_id: &str) -> Result<MemoryScope, StorageError> {
    Ok(MemoryScope {
        namespace_id: parse_id::<NamespaceId>(
            &column::<String>(row, "namespace_id", "fragments", row_id)?,
            "fragments",
            row_id,
            "namespace_id",
        )?,
        tenant_id: parse_optional_id::<TenantId>(
            column::<Option<String>>(row, "tenant_id", "fragments", row_id)?,
            "fragments",
            row_id,
            "tenant_id",
        )?,
        user_id: parse_optional_id::<UserId>(
            column::<Option<String>>(row, "user_id", "fragments", row_id)?,
            "fragments",
            row_id,
            "user_id",
        )?,
        agent_id: parse_optional_id::<AgentId>(
            column::<Option<String>>(row, "agent_id", "fragments", row_id)?,
            "fragments",
            row_id,
            "agent_id",
        )?,
        project_id: parse_optional_id::<ProjectId>(
            column::<Option<String>>(row, "project_id", "fragments", row_id)?,
            "fragments",
            row_id,
            "project_id",
        )?,
    })
}

fn decode_content(
    row: &SqliteRow,
    row_id: &str,
    content_id: &str,
) -> Result<FragmentContent, StorageError> {
    let storage_kind = column::<String>(row, "storage_kind", "fragments", row_id)?;
    let inline_value = column::<Option<Vec<u8>>>(row, "inline_value", "fragments", row_id)?;
    let blob_id = column::<Option<String>>(row, "blob_id", "fragments", row_id)?;
    let media_type = column::<String>(row, "media_type", "fragments", row_id)?;
    let byte_len = nonnegative_u64(
        column::<i64>(row, "content_byte_len", "fragments", row_id)?,
        "fragments",
        row_id,
        "content_byte_len",
    )?;
    let stored_sha = column::<String>(row, "content_sha256", "fragments", row_id)?;
    let stored_digest = parse_id::<BlobId>(&stored_sha, "fragments", row_id, "content_sha256")?;
    let expected_content_id = content_record_id(&storage_kind, &media_type, &stored_sha);
    if expected_content_id != content_id {
        return Err(corrupt(
            "fragments",
            row_id,
            "content row identifier does not match its representation",
        ));
    }

    match storage_kind.as_str() {
        "inline_text" => {
            if blob_id.is_some() || media_type != TEXT_MEDIA_TYPE {
                return Err(corrupt(
                    "fragments",
                    row_id,
                    "invalid inline text representation",
                ));
            }
            let bytes = inline_value
                .ok_or_else(|| corrupt("fragments", row_id, "inline text bytes are missing"))?;
            verify_inline_content(row_id, &bytes, byte_len, &stored_digest)?;
            let text = String::from_utf8(bytes).map_err(|error| {
                corrupt("fragments", row_id, format!("text is not UTF-8: {error}"))
            })?;
            Ok(FragmentContent::Text { text })
        }
        "inline_json" => {
            if blob_id.is_some() || media_type != JSON_MEDIA_TYPE {
                return Err(corrupt(
                    "fragments",
                    row_id,
                    "invalid inline JSON representation",
                ));
            }
            let bytes = inline_value
                .ok_or_else(|| corrupt("fragments", row_id, "inline JSON bytes are missing"))?;
            verify_inline_content(row_id, &bytes, byte_len, &stored_digest)?;
            let value = serde_json::from_slice(&bytes).map_err(|error| {
                corrupt(
                    "fragments",
                    row_id,
                    format!("inline JSON is invalid: {error}"),
                )
            })?;
            Ok(FragmentContent::Json { value })
        }
        "blob" => {
            if inline_value.is_some() || media_type.trim().is_empty() {
                return Err(corrupt("fragments", row_id, "invalid blob representation"));
            }
            let blob_id_raw =
                blob_id.ok_or_else(|| corrupt("fragments", row_id, "blob_id is missing"))?;
            let blob_id = parse_id::<BlobId>(&blob_id_raw, "fragments", row_id, "blob_id")?;
            if blob_id != stored_digest {
                return Err(corrupt(
                    "fragments",
                    row_id,
                    "blob_id differs from content SHA-256",
                ));
            }
            Ok(FragmentContent::Blob {
                blob_id,
                media_type,
                byte_len,
            })
        }
        _ => Err(corrupt("fragments", row_id, "unknown content storage kind")),
    }
}

async fn decode_evidence(
    pool: &SqlitePool,
    owner_fragment_id: &str,
) -> Result<Vec<EvidenceRef>, StorageError> {
    let rows = sqlx::query(
        "SELECT id, referenced_fragment_id, referenced_event_id, verification_method, note
         FROM evidence_refs WHERE owner_fragment_id = ?1 ORDER BY rowid",
    )
    .bind(owner_fragment_id)
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            let evidence_id_raw: String = row.try_get("id").map_err(|error| {
                corrupt(
                    "evidence_refs",
                    owner_fragment_id,
                    format!("invalid evidence id column: {error}"),
                )
            })?;
            let fragment_raw: Option<String> =
                row.try_get("referenced_fragment_id").map_err(|error| {
                    corrupt(
                        "evidence_refs",
                        &evidence_id_raw,
                        format!("invalid fragment reference: {error}"),
                    )
                })?;
            let event_raw: Option<String> =
                row.try_get("referenced_event_id").map_err(|error| {
                    corrupt(
                        "evidence_refs",
                        &evidence_id_raw,
                        format!("invalid event reference: {error}"),
                    )
                })?;
            if fragment_raw.is_some() == event_raw.is_some() {
                return Err(corrupt(
                    "evidence_refs",
                    &evidence_id_raw,
                    "evidence must reference exactly one target",
                ));
            }
            let method_raw: String = row.try_get("verification_method").map_err(|error| {
                corrupt(
                    "evidence_refs",
                    &evidence_id_raw,
                    format!("invalid verification method: {error}"),
                )
            })?;
            let method = decode_verification_method(&method_raw).ok_or_else(|| {
                corrupt(
                    "evidence_refs",
                    &evidence_id_raw,
                    "unknown verification method",
                )
            })?;
            Ok(EvidenceRef {
                id: parse_id::<EvidenceId>(
                    &evidence_id_raw,
                    "evidence_refs",
                    &evidence_id_raw,
                    "id",
                )?,
                fragment_id: parse_optional_id::<FragmentId>(
                    fragment_raw,
                    "evidence_refs",
                    &evidence_id_raw,
                    "referenced_fragment_id",
                )?,
                event_id: parse_optional_id::<EventId>(
                    event_raw,
                    "evidence_refs",
                    &evidence_id_raw,
                    "referenced_event_id",
                )?,
                method,
                note: row.try_get("note").map_err(|error| {
                    corrupt(
                        "evidence_refs",
                        &evidence_id_raw,
                        format!("invalid note: {error}"),
                    )
                })?,
            })
        })
        .collect()
}

struct ContentRecord {
    id: String,
    storage_kind: &'static str,
    inline_value: Option<Vec<u8>>,
    blob_id: Option<String>,
    media_type: String,
    byte_len: i64,
    sha256: String,
}

fn content_record(content: &FragmentContent) -> Result<ContentRecord, StorageError> {
    let (storage_kind, inline_value, blob_id, media_type, byte_len, sha256) = match content {
        FragmentContent::Text { text } => {
            let bytes = text.as_bytes().to_vec();
            let byte_len = i64::try_from(bytes.len()).map_err(|_| StorageError::InvalidDraft {
                reason: "text content length exceeds SQLite INTEGER",
            })?;
            let sha256 = hex::encode(Sha256::digest(&bytes));
            (
                "inline_text",
                Some(bytes),
                None,
                TEXT_MEDIA_TYPE.to_owned(),
                byte_len,
                sha256,
            )
        }
        FragmentContent::Json { value } => {
            let bytes = serde_json::to_vec(&canonicalize(value.clone()))?;
            let byte_len = i64::try_from(bytes.len()).map_err(|_| StorageError::InvalidDraft {
                reason: "JSON content length exceeds SQLite INTEGER",
            })?;
            let sha256 = hex::encode(Sha256::digest(&bytes));
            (
                "inline_json",
                Some(bytes),
                None,
                JSON_MEDIA_TYPE.to_owned(),
                byte_len,
                sha256,
            )
        }
        FragmentContent::Blob {
            blob_id,
            media_type,
            byte_len,
        } => (
            "blob",
            None,
            Some(blob_id.to_string()),
            media_type.clone(),
            i64::try_from(*byte_len).map_err(|_| StorageError::InvalidDraft {
                reason: "blob content length exceeds SQLite INTEGER",
            })?,
            blob_id.to_string(),
        ),
    };
    let id = content_record_id(storage_kind, &media_type, &sha256);
    Ok(ContentRecord {
        id,
        storage_kind,
        inline_value,
        blob_id,
        media_type,
        byte_len,
        sha256,
    })
}

fn validate_draft(draft: &EventDraft) -> Result<(), StorageError> {
    validate_fragment(&draft.payload)?;
    if draft.fragment_id != draft.payload.id {
        return Err(StorageError::InvalidDraft {
            reason: "fragment_id differs from payload.id",
        });
    }
    if draft.scope != draft.payload.scope {
        return Err(StorageError::InvalidDraft {
            reason: "event scope differs from payload scope",
        });
    }
    if draft.stage != stage_for_fragment(&draft.payload) {
        return Err(StorageError::InvalidDraft {
            reason: "stage does not match fragment kind and validity",
        });
    }
    if canonical_json_digest(&draft.payload)? != draft.content_hash {
        return Err(StorageError::InvalidDraft {
            reason: "content_hash differs from canonical payload",
        });
    }
    Ok(())
}

fn stage_for_fragment(fragment: &CognitiveFragment) -> CognitiveStage {
    match fragment.kind {
        FragmentKind::Inference | FragmentKind::Prediction | FragmentKind::Plan => {
            CognitiveStage::FutureHypothesis
        }
        FragmentKind::Decision | FragmentKind::Approval | FragmentKind::Rejection => {
            CognitiveStage::RuntimeDecision
        }
        FragmentKind::Action | FragmentKind::ToolCall => CognitiveStage::ToolAction,
        FragmentKind::ToolResult
        | FragmentKind::Outcome
        | FragmentKind::Failure
        | FragmentKind::Success => CognitiveStage::ActualResult,
        FragmentKind::StateTransition => CognitiveStage::ResultVerification,
        FragmentKind::Fact
        | FragmentKind::Correction
        | FragmentKind::Constraint
        | FragmentKind::Preference
            if fragment.validity == ValidityStatus::Verified =>
        {
            CognitiveStage::PastEvidence
        }
        FragmentKind::Observation
        | FragmentKind::Claim
        | FragmentKind::Fact
        | FragmentKind::Correction
        | FragmentKind::Constraint
        | FragmentKind::Preference => CognitiveStage::CurrentObservation,
    }
}

fn searchable_body(content: &FragmentContent) -> Result<Option<String>, StorageError> {
    match content {
        FragmentContent::Text { text } => Ok(Some(text.clone())),
        FragmentContent::Json { value } => {
            Ok(Some(serde_json::to_string(&canonicalize(value.clone()))?))
        }
        FragmentContent::Blob { .. } => Ok(None),
    }
}

fn scope_key(scope: &MemoryScope) -> Result<String, StorageError> {
    Ok(hex::encode(Sha256::digest(serde_json::to_vec(
        &canonicalize(serde_json::to_value(scope)?),
    )?)))
}

fn canonical_json_digest(value: &CognitiveFragment) -> Result<BlobId, StorageError> {
    let bytes = serde_json::to_vec(&canonicalize(serde_json::to_value(value)?))?;
    BlobId::from_str(&hex::encode(Sha256::digest(bytes))).map_err(|_| StorageError::InvalidDraft {
        reason: "SHA-256 digest could not form a BlobId",
    })
}

fn canonicalize(value: Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.into_iter().map(canonicalize).collect()),
        Value::Object(values) => {
            let mut entries: Vec<_> = values.into_iter().collect();
            entries.sort_by(|left, right| left.0.cmp(&right.0));
            Value::Object(
                entries
                    .into_iter()
                    .map(|(key, value)| (key, canonicalize(value)))
                    .collect(),
            )
        }
        scalar => scalar,
    }
}

fn content_record_id(storage_kind: &str, media_type: &str, sha256: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(storage_kind.as_bytes());
    digest.update([0]);
    digest.update(media_type.as_bytes());
    digest.update([0]);
    digest.update(sha256.as_bytes());
    hex::encode(digest.finalize())
}

fn verify_inline_content(
    row_id: &str,
    bytes: &[u8],
    expected_len: u64,
    expected_digest: &BlobId,
) -> Result<(), StorageError> {
    let actual_len = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    if actual_len != expected_len {
        return Err(corrupt("fragments", row_id, "inline byte length mismatch"));
    }
    let actual_digest = BlobId::from_str(&hex::encode(Sha256::digest(bytes)))
        .expect("SHA-256 always forms a valid BlobId");
    if &actual_digest != expected_digest {
        return Err(corrupt("fragments", row_id, "inline SHA-256 mismatch"));
    }
    Ok(())
}

fn column<T>(
    row: &SqliteRow,
    name: &str,
    table: &'static str,
    row_id: &str,
) -> Result<T, StorageError>
where
    for<'decode> T: Decode<'decode, Sqlite> + Type<Sqlite>,
{
    row.try_get(name)
        .map_err(|error| corrupt(table, row_id, format!("invalid {name}: {error}")))
}

fn parse_id<T>(raw: &str, table: &'static str, row_id: &str, field: &str) -> Result<T, StorageError>
where
    T: std::str::FromStr,
    T::Err: Display,
{
    raw.parse()
        .map_err(|error| corrupt(table, row_id, format!("invalid {field}: {error}")))
}

fn parse_optional_id<T>(
    raw: Option<String>,
    table: &'static str,
    row_id: &str,
    field: &str,
) -> Result<Option<T>, StorageError>
where
    T: std::str::FromStr,
    T::Err: Display,
{
    raw.map(|value| parse_id(&value, table, row_id, field))
        .transpose()
}

fn decode_time(
    micros: i64,
    table: &'static str,
    row_id: &str,
    field: &str,
) -> Result<DateTime<Utc>, StorageError> {
    DateTime::from_timestamp_micros(micros)
        .ok_or_else(|| corrupt(table, row_id, format!("{field} is outside UTC range")))
}

fn nonnegative_u64(
    value: i64,
    table: &'static str,
    row_id: &str,
    field: &str,
) -> Result<u64, StorageError> {
    u64::try_from(value)
        .map_err(|_| corrupt(table, row_id, format!("{field} is negative or too large")))
}

#[allow(clippy::cast_possible_truncation)]
fn unit_f32(
    value: f64,
    table: &'static str,
    row_id: &str,
    field: &str,
) -> Result<f32, StorageError> {
    if value.is_finite() && (0.0..=1.0).contains(&value) {
        let decoded = value as f32;
        if f64::from(decoded).to_bits() == value.to_bits() {
            Ok(decoded)
        } else {
            Err(corrupt(
                table,
                row_id,
                format!("{field} cannot be represented exactly as f32"),
            ))
        }
    } else {
        Err(corrupt(
            table,
            row_id,
            format!("{field} is not a finite unit interval"),
        ))
    }
}

fn corrupt(table: &'static str, id: &str, reason: impl Into<String>) -> StorageError {
    StorageError::CorruptRow {
        table,
        id: id.to_owned(),
        reason: reason.into(),
    }
}

fn encode_kind(value: FragmentKind) -> &'static str {
    match value {
        FragmentKind::Observation => "observation",
        FragmentKind::Claim => "claim",
        FragmentKind::Inference => "inference",
        FragmentKind::Prediction => "prediction",
        FragmentKind::Decision => "decision",
        FragmentKind::Action => "action",
        FragmentKind::ToolCall => "tool_call",
        FragmentKind::ToolResult => "tool_result",
        FragmentKind::Outcome => "outcome",
        FragmentKind::Correction => "correction",
        FragmentKind::Constraint => "constraint",
        FragmentKind::Preference => "preference",
        FragmentKind::Fact => "fact",
        FragmentKind::Failure => "failure",
        FragmentKind::Success => "success",
        FragmentKind::Plan => "plan",
        FragmentKind::Approval => "approval",
        FragmentKind::Rejection => "rejection",
        FragmentKind::StateTransition => "state_transition",
    }
}

fn decode_kind(value: &str) -> Option<FragmentKind> {
    Some(match value {
        "observation" => FragmentKind::Observation,
        "claim" => FragmentKind::Claim,
        "inference" => FragmentKind::Inference,
        "prediction" => FragmentKind::Prediction,
        "decision" => FragmentKind::Decision,
        "action" => FragmentKind::Action,
        "tool_call" => FragmentKind::ToolCall,
        "tool_result" => FragmentKind::ToolResult,
        "outcome" => FragmentKind::Outcome,
        "correction" => FragmentKind::Correction,
        "constraint" => FragmentKind::Constraint,
        "preference" => FragmentKind::Preference,
        "fact" => FragmentKind::Fact,
        "failure" => FragmentKind::Failure,
        "success" => FragmentKind::Success,
        "plan" => FragmentKind::Plan,
        "approval" => FragmentKind::Approval,
        "rejection" => FragmentKind::Rejection,
        "state_transition" => FragmentKind::StateTransition,
        _ => return None,
    })
}

fn encode_perspective(value: Perspective) -> &'static str {
    match value {
        Perspective::FirstPerson => "first_person",
        Perspective::SecondPerson => "second_person",
        Perspective::ThirdPerson => "third_person",
        Perspective::System => "system",
        Perspective::Tool => "tool",
    }
}

fn decode_perspective(value: &str) -> Option<Perspective> {
    Some(match value {
        "first_person" => Perspective::FirstPerson,
        "second_person" => Perspective::SecondPerson,
        "third_person" => Perspective::ThirdPerson,
        "system" => Perspective::System,
        "tool" => Perspective::Tool,
        _ => return None,
    })
}

fn encode_validity(value: ValidityStatus) -> &'static str {
    match value {
        ValidityStatus::Unverified => "unverified",
        ValidityStatus::Verified => "verified",
        ValidityStatus::Disputed => "disputed",
        ValidityStatus::Superseded => "superseded",
        ValidityStatus::Invalidated => "invalidated",
        ValidityStatus::Quarantined => "quarantined",
    }
}

fn decode_validity(value: &str) -> Option<ValidityStatus> {
    Some(match value {
        "unverified" => ValidityStatus::Unverified,
        "verified" => ValidityStatus::Verified,
        "disputed" => ValidityStatus::Disputed,
        "superseded" => ValidityStatus::Superseded,
        "invalidated" => ValidityStatus::Invalidated,
        "quarantined" => ValidityStatus::Quarantined,
        _ => return None,
    })
}

fn encode_security_label(value: SecurityLabel) -> &'static str {
    match value {
        SecurityLabel::Public => "public",
        SecurityLabel::Internal => "internal",
        SecurityLabel::Confidential => "confidential",
        SecurityLabel::Restricted => "restricted",
    }
}

fn decode_security_label(value: &str) -> Option<SecurityLabel> {
    Some(match value {
        "public" => SecurityLabel::Public,
        "internal" => SecurityLabel::Internal,
        "confidential" => SecurityLabel::Confidential,
        "restricted" => SecurityLabel::Restricted,
        _ => return None,
    })
}

fn encode_verification_method(value: VerificationMethod) -> &'static str {
    match value {
        VerificationMethod::Manual => "manual",
        VerificationMethod::Tool => "tool",
        VerificationMethod::ExternalSource => "external_source",
        VerificationMethod::CrossEvidence => "cross_evidence",
    }
}

fn decode_verification_method(value: &str) -> Option<VerificationMethod> {
    Some(match value {
        "manual" => VerificationMethod::Manual,
        "tool" => VerificationMethod::Tool,
        "external_source" => VerificationMethod::ExternalSource,
        "cross_evidence" => VerificationMethod::CrossEvidence,
        _ => return None,
    })
}

fn encode_stage(value: CognitiveStage) -> &'static str {
    match value {
        CognitiveStage::PastEvidence => "past_evidence",
        CognitiveStage::CurrentObservation => "current_observation",
        CognitiveStage::FutureHypothesis => "future_hypothesis",
        CognitiveStage::RuntimeDecision => "runtime_decision",
        CognitiveStage::ToolAction => "tool_action",
        CognitiveStage::ActualResult => "actual_result",
        CognitiveStage::ResultVerification => "result_verification",
    }
}

fn decode_stage(value: &str) -> Option<CognitiveStage> {
    Some(match value {
        "past_evidence" => CognitiveStage::PastEvidence,
        "current_observation" => CognitiveStage::CurrentObservation,
        "future_hypothesis" => CognitiveStage::FutureHypothesis,
        "runtime_decision" => CognitiveStage::RuntimeDecision,
        "tool_action" => CognitiveStage::ToolAction,
        "actual_result" => CognitiveStage::ActualResult,
        "result_verification" => CognitiveStage::ResultVerification,
        _ => return None,
    })
}
