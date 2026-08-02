use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    str::FromStr as _,
    sync::Arc,
};

use chrono::{DateTime, Utc};
use jc_memory_events::validation::validate_state_metadata;
use jc_memory_graph::{CognitiveEdge, CognitivePoint, EvidenceLink};
use jc_memory_protocol::{
    CognitiveFragment, ConflictView, EdgeId, EdgeKind, EventId, EvidenceId, EvidenceView,
    FragmentContent, FragmentId, FragmentKind, MemoryScope, PointId, PointKind, StateDeclaration,
    StateId, StateVersionId, StateView, ValidityStatus, VerificationMethod,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::{QueryBuilder, Row, Sqlite, SqlitePool, Transaction};

use crate::{SqliteStore, StorageError};

/// 可独立追赶的持久化投影类别。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProjectorKind {
    /// 显式状态版本投影。
    State,
    /// Point 与 Edge 图投影。
    Graph,
}

#[derive(Debug)]
struct StoredProjectionEvent {
    id: EventId,
    sequence: u64,
    recorded_at: DateTime<Utc>,
    fragment: CognitiveFragment,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StoredMutationKind {
    Correct,
    Reinforce,
    Weaken,
    Supersede,
    Contradict,
    Forget,
}

#[derive(Debug)]
struct TargetStateVersion {
    state_id: StateId,
    state_key: String,
    version_id: StateVersionId,
    valid_from: DateTime<Utc>,
}

async fn checkpoint_in_transaction(
    transaction: &mut Transaction<'_, Sqlite>,
    scope_hash: &str,
) -> Result<Option<u64>, StorageError> {
    let sequence: Option<i64> = sqlx::query_scalar(
        "SELECT sequence FROM projection_checkpoints
         WHERE scope_key = ?1 AND projector = 'state'",
    )
    .bind(scope_hash)
    .fetch_optional(&mut **transaction)
    .await?;
    sequence
        .map(|value| {
            u64::try_from(value).map_err(|_| checkpoint_corrupt(ProjectorKind::State, scope_hash))
        })
        .transpose()
}

fn checkpoint_value(sequence: u64, updated_at: DateTime<Utc>) -> ProjectionCheckpoint {
    ProjectionCheckpoint {
        projector: ProjectorKind::State,
        sequence,
        updated_at,
    }
}

fn decode_projection_event(
    row: &sqlx::sqlite::SqliteRow,
    scope: &MemoryScope,
    scope_hash: &str,
) -> Result<StoredProjectionEvent, StorageError> {
    let raw_event_id = row
        .try_get::<String, _>("id")
        .map_err(|_| event_corrupt("id", scope_hash))?;
    let id = EventId::from_str(&raw_event_id).map_err(|_| event_corrupt("id", scope_hash))?;
    let raw_fragment_id = row
        .try_get::<String, _>("fragment_id")
        .map_err(|_| event_corrupt("fragment_id", scope_hash))?;
    let raw_sequence = row
        .try_get::<i64, _>("sequence")
        .map_err(|_| event_corrupt("sequence", scope_hash))?;
    let sequence =
        u64::try_from(raw_sequence).map_err(|_| event_corrupt("sequence", scope_hash))?;
    let recorded_at_us = row
        .try_get::<i64, _>("recorded_at_us")
        .map_err(|_| event_corrupt("recorded_at", scope_hash))?;
    let recorded_at = DateTime::from_timestamp_micros(recorded_at_us)
        .ok_or_else(|| event_corrupt("recorded_at", scope_hash))?;
    let payload_json = row
        .try_get::<String, _>("payload_json")
        .map_err(|_| event_corrupt("payload_json", scope_hash))?;
    let fragment: CognitiveFragment = serde_json::from_str(&payload_json)
        .map_err(|_| event_corrupt("payload_json", scope_hash))?;
    if fragment.id.to_string() != raw_fragment_id {
        return Err(event_corrupt("fragment_id", scope_hash));
    }
    if &fragment.scope != scope {
        return Err(StorageError::ProjectionScopeMismatch {
            projector: ProjectorKind::State,
            scope_hash: scope_hash.to_owned(),
        });
    }
    Ok(StoredProjectionEvent {
        id,
        sequence,
        recorded_at,
        fragment,
    })
}

async fn graph_checkpoint_in_transaction(
    transaction: &mut Transaction<'_, Sqlite>,
    scope_hash: &str,
) -> Result<Option<u64>, StorageError> {
    let sequence: Option<i64> = sqlx::query_scalar(
        "SELECT sequence FROM projection_checkpoints
         WHERE scope_key = ?1 AND projector = 'graph'",
    )
    .bind(scope_hash)
    .fetch_optional(&mut **transaction)
    .await?;
    sequence
        .map(|value| {
            u64::try_from(value).map_err(|_| checkpoint_corrupt(ProjectorKind::Graph, scope_hash))
        })
        .transpose()
}

fn graph_checkpoint_value(sequence: u64, updated_at: DateTime<Utc>) -> ProjectionCheckpoint {
    ProjectionCheckpoint {
        projector: ProjectorKind::Graph,
        sequence,
        updated_at,
    }
}

fn decode_graph_projection_event(
    row: &sqlx::sqlite::SqliteRow,
    scope: &MemoryScope,
    scope_hash: &str,
) -> Result<StoredProjectionEvent, StorageError> {
    let raw_event_id = row
        .try_get::<String, _>("id")
        .map_err(|_| graph_event_corrupt("id", scope_hash))?;
    let id = EventId::from_str(&raw_event_id).map_err(|_| graph_event_corrupt("id", scope_hash))?;
    let raw_fragment_id = row
        .try_get::<String, _>("fragment_id")
        .map_err(|_| graph_event_corrupt("fragment_id", scope_hash))?;
    let raw_sequence = row
        .try_get::<i64, _>("sequence")
        .map_err(|_| graph_event_corrupt("sequence", scope_hash))?;
    let sequence =
        u64::try_from(raw_sequence).map_err(|_| graph_event_corrupt("sequence", scope_hash))?;
    let recorded_at_us = row
        .try_get::<i64, _>("recorded_at_us")
        .map_err(|_| graph_event_corrupt("recorded_at", scope_hash))?;
    let recorded_at = DateTime::from_timestamp_micros(recorded_at_us)
        .ok_or_else(|| graph_event_corrupt("recorded_at", scope_hash))?;
    let payload_json = row
        .try_get::<String, _>("payload_json")
        .map_err(|_| graph_event_corrupt("payload_json", scope_hash))?;
    let fragment: CognitiveFragment = serde_json::from_str(&payload_json)
        .map_err(|_| graph_event_corrupt("payload_json", scope_hash))?;
    if fragment.id.to_string() != raw_fragment_id {
        return Err(graph_event_corrupt("fragment_id", scope_hash));
    }
    if &fragment.scope != scope {
        return Err(StorageError::ProjectionScopeMismatch {
            projector: ProjectorKind::Graph,
            scope_hash: scope_hash.to_owned(),
        });
    }
    Ok(StoredProjectionEvent {
        id,
        sequence,
        recorded_at,
        fragment,
    })
}

async fn apply_graph_event(
    transaction: &mut Transaction<'_, Sqlite>,
    scope: &MemoryScope,
    scope_hash: &str,
    event: &StoredProjectionEvent,
) -> Result<(), StorageError> {
    let Some(source) = upsert_graph_point(transaction, scope_hash, event).await? else {
        return Ok(());
    };
    if event.fragment.validity == ValidityStatus::Quarantined {
        return Ok(());
    }
    let Some(relation) = graph_relation(&event.fragment, scope_hash)? else {
        return Ok(());
    };
    let target_id = graph_mutation_target_id(&event.fragment, scope_hash)?;
    let target_event = load_graph_target_event(transaction, scope, scope_hash, target_id).await?;
    let Some(target) = upsert_graph_point(transaction, scope_hash, &target_event).await? else {
        return Ok(());
    };
    if source.id == target.id {
        return Err(graph_event_corrupt("target_fragment_id", scope_hash));
    }
    let evidence = graph_relation_evidence(event, &target_event, scope_hash)?;
    let edge = CognitiveEdge {
        id: edge_id(event.id, scope_hash)?,
        scope: scope.clone(),
        source_point_id: source.id,
        target_point_id: target.id,
        kind: relation,
        weight: 0.5,
        confidence: source.confidence.min(target.confidence),
        evidence: vec![EvidenceLink {
            fragment_id: event.fragment.id,
            event_id: Some(event.id),
            evidence_ref_id: Some(evidence.id),
        }],
        valid_from: source.occurred_at.max(target.occurred_at),
        valid_to: None,
        model_version_id: None,
        human_confirmed: evidence.method == VerificationMethod::Manual,
        tool_verified: evidence.method == VerificationMethod::Tool,
    };
    upsert_graph_edge(transaction, scope_hash, event.recorded_at, &edge).await
}

async fn upsert_graph_point(
    transaction: &mut Transaction<'_, Sqlite>,
    scope_hash: &str,
    event: &StoredProjectionEvent,
) -> Result<Option<CognitivePoint>, StorageError> {
    let Ok(kind) = PointKind::try_from(event.fragment.kind) else {
        return Ok(None);
    };
    let point = CognitivePoint {
        id: point_id(event.fragment.id, scope_hash)?,
        scope: event.fragment.scope.clone(),
        kind,
        source_fragments: vec![event.fragment.id],
        occurred_at: event.fragment.occurred_at,
        confidence: event.fragment.confidence,
        validity: event.fragment.validity,
    };
    sqlx::query(
        "INSERT INTO points (
            id, scope_key, kind, source_fragment_id, valid_from_us,
            valid_to_us, confidence, created_at_us
         ) VALUES (?1, ?2, ?3, ?4, ?5, NULL, ?6, ?7)
         ON CONFLICT(id) DO NOTHING",
    )
    .bind(point.id.to_string())
    .bind(scope_hash)
    .bind(point_kind_name(point.kind))
    .bind(event.fragment.id.to_string())
    .bind(point.occurred_at.timestamp_micros())
    .bind(f64::from(point.confidence))
    .bind(event.recorded_at.timestamp_micros())
    .execute(&mut **transaction)
    .await?;
    validate_stored_point(transaction, scope_hash, &point).await?;
    Ok(Some(point))
}

async fn validate_stored_point(
    transaction: &mut Transaction<'_, Sqlite>,
    scope_hash: &str,
    point: &CognitivePoint,
) -> Result<(), StorageError> {
    let row = sqlx::query(
        "SELECT scope_key, kind, source_fragment_id, valid_from_us, valid_to_us, confidence
         FROM points WHERE id = ?1",
    )
    .bind(point.id.to_string())
    .fetch_one(&mut **transaction)
    .await?;
    let compatible = row.try_get::<String, _>("scope_key")? == scope_hash
        && row.try_get::<String, _>("kind")? == point_kind_name(point.kind)
        && row.try_get::<String, _>("source_fragment_id")? == point.source_fragments[0].to_string()
        && row.try_get::<i64, _>("valid_from_us")? == point.occurred_at.timestamp_micros()
        && row.try_get::<Option<i64>, _>("valid_to_us")?.is_none()
        && row.try_get::<f64, _>("confidence")?.to_bits() == f64::from(point.confidence).to_bits();
    if compatible {
        Ok(())
    } else {
        Err(graph_event_corrupt("point_identity", scope_hash))
    }
}

fn graph_relation(
    fragment: &CognitiveFragment,
    scope_hash: &str,
) -> Result<Option<EdgeKind>, StorageError> {
    let Some(mutation) = fragment.metadata.get("mutation") else {
        return Ok(None);
    };
    if !matches!(
        mutation.as_str(),
        Some("correct" | "reinforce" | "weaken" | "supersede" | "contradict" | "forget")
    ) {
        return Err(graph_event_corrupt("mutation", scope_hash));
    }
    let Some(raw_relation) = fragment.metadata.get("relation") else {
        return Ok(None);
    };
    if raw_relation.is_null() {
        return Ok(None);
    }
    serde_json::from_value(raw_relation.clone())
        .map(Some)
        .map_err(|_| graph_event_corrupt("relation", scope_hash))
}

fn graph_mutation_target_id(
    fragment: &CognitiveFragment,
    scope_hash: &str,
) -> Result<FragmentId, StorageError> {
    let raw = fragment
        .metadata
        .get("target_fragment_id")
        .and_then(Value::as_str)
        .ok_or_else(|| graph_event_corrupt("target_fragment_id", scope_hash))?;
    FragmentId::from_str(raw).map_err(|_| graph_event_corrupt("target_fragment_id", scope_hash))
}

async fn load_graph_target_event(
    transaction: &mut Transaction<'_, Sqlite>,
    scope: &MemoryScope,
    scope_hash: &str,
    target_id: FragmentId,
) -> Result<StoredProjectionEvent, StorageError> {
    let row = sqlx::query(
        "SELECT id, fragment_id, sequence, recorded_at_us, payload_json
         FROM events WHERE scope_key = ?1 AND fragment_id = ?2
         ORDER BY sequence, id LIMIT 1",
    )
    .bind(scope_hash)
    .bind(target_id.to_string())
    .fetch_optional(&mut **transaction)
    .await?;
    if let Some(row) = row {
        return decode_graph_projection_event(&row, scope, scope_hash);
    }
    let foreign_scope: Option<String> =
        sqlx::query_scalar("SELECT scope_key FROM fragments WHERE id = ?1")
            .bind(target_id.to_string())
            .fetch_optional(&mut **transaction)
            .await?;
    if foreign_scope.is_some_and(|foreign| foreign != scope_hash) {
        return Err(StorageError::ProjectionScopeMismatch {
            projector: ProjectorKind::Graph,
            scope_hash: scope_hash.to_owned(),
        });
    }
    Err(graph_event_corrupt("target_fragment_id", scope_hash))
}

fn graph_relation_evidence<'a>(
    event: &'a StoredProjectionEvent,
    target: &StoredProjectionEvent,
    scope_hash: &str,
) -> Result<&'a jc_memory_protocol::EvidenceRef, StorageError> {
    event
        .fragment
        .evidence
        .iter()
        .find(|evidence| {
            evidence.fragment_id == Some(target.fragment.id) || evidence.event_id == Some(target.id)
        })
        .ok_or_else(|| graph_event_corrupt("evidence", scope_hash))
}

async fn upsert_graph_edge(
    transaction: &mut Transaction<'_, Sqlite>,
    scope_hash: &str,
    created_at: DateTime<Utc>,
    edge: &CognitiveEdge,
) -> Result<(), StorageError> {
    let evidence = edge
        .evidence
        .first()
        .ok_or_else(|| graph_event_corrupt("evidence", scope_hash))?;
    sqlx::query(
        "INSERT INTO edges (
            id, scope_key, source_point_id, target_point_id, kind, weight,
            confidence, source_fragment_id, valid_from_us, valid_to_us,
            model_version_id, human_confirmed, tool_verified, created_at_us
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, NULL, NULL, ?10, ?11, ?12)
         ON CONFLICT(id) DO NOTHING",
    )
    .bind(edge.id.to_string())
    .bind(scope_hash)
    .bind(edge.source_point_id.to_string())
    .bind(edge.target_point_id.to_string())
    .bind(edge_kind_name(edge.kind))
    .bind(f64::from(edge.weight))
    .bind(f64::from(edge.confidence))
    .bind(evidence.fragment_id.to_string())
    .bind(edge.valid_from.timestamp_micros())
    .bind(i64::from(edge.human_confirmed))
    .bind(i64::from(edge.tool_verified))
    .bind(created_at.timestamp_micros())
    .execute(&mut **transaction)
    .await?;
    validate_stored_edge(transaction, scope_hash, edge).await
}

async fn validate_stored_edge(
    transaction: &mut Transaction<'_, Sqlite>,
    scope_hash: &str,
    edge: &CognitiveEdge,
) -> Result<(), StorageError> {
    let row = sqlx::query(
        "SELECT scope_key, source_point_id, target_point_id, kind, weight, confidence,
                source_fragment_id, valid_from_us, valid_to_us, model_version_id,
                human_confirmed, tool_verified
         FROM edges WHERE id = ?1",
    )
    .bind(edge.id.to_string())
    .fetch_one(&mut **transaction)
    .await?;
    let compatible = row.try_get::<String, _>("scope_key")? == scope_hash
        && row.try_get::<String, _>("source_point_id")? == edge.source_point_id.to_string()
        && row.try_get::<String, _>("target_point_id")? == edge.target_point_id.to_string()
        && row.try_get::<String, _>("kind")? == edge_kind_name(edge.kind)
        && row.try_get::<f64, _>("weight")?.to_bits() == f64::from(edge.weight).to_bits()
        && row.try_get::<f64, _>("confidence")?.to_bits() == f64::from(edge.confidence).to_bits()
        && row.try_get::<String, _>("source_fragment_id")?
            == edge.evidence[0].fragment_id.to_string()
        && row.try_get::<i64, _>("valid_from_us")? == edge.valid_from.timestamp_micros()
        && row.try_get::<Option<i64>, _>("valid_to_us")?.is_none()
        && row
            .try_get::<Option<String>, _>("model_version_id")?
            .is_none()
        && row.try_get::<i64, _>("human_confirmed")? == i64::from(edge.human_confirmed)
        && row.try_get::<i64, _>("tool_verified")? == i64::from(edge.tool_verified);
    if compatible {
        Ok(())
    } else {
        Err(graph_event_corrupt("edge_identity", scope_hash))
    }
}

async fn apply_state_event(
    transaction: &mut Transaction<'_, Sqlite>,
    scope_hash: &str,
    event: &StoredProjectionEvent,
) -> Result<(), StorageError> {
    if event.fragment.validity == ValidityStatus::Quarantined {
        return Ok(());
    }
    let mutation = mutation_kind(&event.fragment, scope_hash)?;
    let state = state_metadata_for_projection(&event.fragment, scope_hash)?;
    match (mutation, state) {
        (Some(StoredMutationKind::Forget), state) => {
            mutate_target(
                transaction,
                scope_hash,
                event,
                state.as_ref(),
                TargetMutation::Invalidate,
            )
            .await
        }
        (Some(StoredMutationKind::Weaken), state) => {
            mutate_target(
                transaction,
                scope_hash,
                event,
                state.as_ref(),
                TargetMutation::Dispute,
            )
            .await
        }
        (Some(StoredMutationKind::Contradict), state) => {
            contradict_target(transaction, scope_hash, event, state.as_ref()).await
        }
        (Some(StoredMutationKind::Correct | StoredMutationKind::Supersede), Some(state)) => {
            supersede_target(transaction, scope_hash, event, &state).await
        }
        (None, Some(state)) if state_source_is_visible(&event.fragment) => {
            record_or_advance_state(transaction, scope_hash, event, &state).await
        }
        _ => Ok(()),
    }
}

fn mutation_kind(
    fragment: &CognitiveFragment,
    scope_hash: &str,
) -> Result<Option<StoredMutationKind>, StorageError> {
    let Some(raw) = fragment.metadata.get("mutation") else {
        return Ok(None);
    };
    let Some(kind) = raw.as_str() else {
        return Err(event_corrupt("mutation", scope_hash));
    };
    let kind = match kind {
        "correct" => StoredMutationKind::Correct,
        "reinforce" => StoredMutationKind::Reinforce,
        "weaken" => StoredMutationKind::Weaken,
        "supersede" => StoredMutationKind::Supersede,
        "contradict" => StoredMutationKind::Contradict,
        "forget" => StoredMutationKind::Forget,
        _ => return Err(event_corrupt("mutation", scope_hash)),
    };
    Ok(Some(kind))
}

fn state_metadata_for_projection(
    fragment: &CognitiveFragment,
    scope_hash: &str,
) -> Result<Option<StateDeclaration>, StorageError> {
    if fragment.metadata.get("state").is_none() {
        return Ok(None);
    }
    if !matches!(
        fragment.kind,
        FragmentKind::Fact | FragmentKind::StateTransition | FragmentKind::Correction
    ) {
        return Ok(None);
    }
    validate_state_metadata(fragment).map_err(|_| event_corrupt("metadata.state", scope_hash))
}

fn state_source_is_visible(fragment: &CognitiveFragment) -> bool {
    matches!(
        fragment.kind,
        FragmentKind::Fact | FragmentKind::StateTransition | FragmentKind::Correction
    ) && !matches!(
        fragment.validity,
        ValidityStatus::Quarantined | ValidityStatus::Invalidated
    )
}

async fn record_or_advance_state(
    transaction: &mut Transaction<'_, Sqlite>,
    scope_hash: &str,
    event: &StoredProjectionEvent,
    state: &StateDeclaration,
) -> Result<(), StorageError> {
    if version_exists(transaction, event.fragment.id).await? {
        return Ok(());
    }
    let state_row = sqlx::query(
        "SELECT id, current_version_id FROM states
         WHERE scope_key = ?1 AND state_key = ?2",
    )
    .bind(scope_hash)
    .bind(&state.key)
    .fetch_optional(&mut **transaction)
    .await?;
    let Some(state_row) = state_row else {
        let state_id = state_id(event.fragment.id, scope_hash)?;
        let version_id = version_id(event.fragment.id, scope_hash)?;
        sqlx::query(
            "INSERT INTO states (id, scope_key, state_key, current_version_id, created_at_us)
             VALUES (?1, ?2, ?3, NULL, ?4)",
        )
        .bind(state_id.to_string())
        .bind(scope_hash)
        .bind(&state.key)
        .bind(event.recorded_at.timestamp_micros())
        .execute(&mut **transaction)
        .await?;
        insert_state_version(
            transaction,
            state_id,
            None,
            event,
            state,
            event.fragment.validity,
        )
        .await?;
        sqlx::query("UPDATE states SET current_version_id = ?1 WHERE id = ?2")
            .bind(version_id.to_string())
            .bind(state_id.to_string())
            .execute(&mut **transaction)
            .await?;
        return Ok(());
    };
    let state_id = parse_state_id(
        &state_row.try_get::<String, _>("id")?,
        "state_id",
        scope_hash,
    )?;
    advance_existing_state(transaction, scope_hash, state_id, event, state).await
}

async fn advance_existing_state(
    transaction: &mut Transaction<'_, Sqlite>,
    scope_hash: &str,
    state_id: StateId,
    event: &StoredProjectionEvent,
    state: &StateDeclaration,
) -> Result<(), StorageError> {
    let open = sqlx::query(
        "SELECT id, value_json, valid_from_us
         FROM state_versions
         WHERE state_id = ?1 AND valid_to_us IS NULL
         ORDER BY valid_from_us DESC, id DESC",
    )
    .bind(state_id.to_string())
    .fetch_all(&mut **transaction)
    .await?;
    if open.is_empty() {
        let previous = latest_version_id(transaction, state_id, scope_hash).await?;
        insert_state_version(
            transaction,
            state_id,
            previous,
            event,
            state,
            event.fragment.validity,
        )
        .await?;
        set_current_version(transaction, state_id, event.fragment.id).await?;
        return Ok(());
    }
    let latest_time_us = open[0].try_get::<i64, _>("valid_from_us")?;
    let latest_time = DateTime::from_timestamp_micros(latest_time_us)
        .ok_or_else(|| chain_corrupt("valid_from", scope_hash))?;
    if event.fragment.occurred_at < latest_time {
        return Err(event_corrupt("occurred_at", scope_hash));
    }
    let canonical_value = canonical_json(&state.value)?;
    if event.fragment.occurred_at == latest_time {
        if open.iter().any(|row| {
            row.try_get::<String, _>("value_json").ok().as_deref() == Some(&canonical_value)
        }) {
            return Ok(());
        }
        sqlx::query(
            "UPDATE state_versions SET validity = 'disputed'
             WHERE state_id = ?1 AND valid_to_us IS NULL",
        )
        .bind(state_id.to_string())
        .execute(&mut **transaction)
        .await?;
        let previous = parse_version_id(
            &open[0].try_get::<String, _>("id")?,
            "previous_version_id",
            scope_hash,
        )?;
        insert_state_version(
            transaction,
            state_id,
            Some(previous),
            event,
            state,
            ValidityStatus::Disputed,
        )
        .await?;
        clear_current_version(transaction, state_id).await?;
        return Ok(());
    }
    let previous = parse_version_id(
        &open[0].try_get::<String, _>("id")?,
        "previous_version_id",
        scope_hash,
    )?;
    close_open_versions(
        transaction,
        state_id,
        event.fragment.occurred_at,
        ValidityStatus::Superseded,
    )
    .await?;
    insert_state_version(
        transaction,
        state_id,
        Some(previous),
        event,
        state,
        event.fragment.validity,
    )
    .await?;
    set_current_version(transaction, state_id, event.fragment.id).await
}

#[derive(Clone, Copy)]
enum TargetMutation {
    Invalidate,
    Dispute,
}

async fn mutate_target(
    transaction: &mut Transaction<'_, Sqlite>,
    scope_hash: &str,
    event: &StoredProjectionEvent,
    state: Option<&StateDeclaration>,
    mutation: TargetMutation,
) -> Result<(), StorageError> {
    let Some(target) =
        target_state_version(transaction, scope_hash, event, state.is_some()).await?
    else {
        return Ok(());
    };
    validate_mutation_key(&target, state, scope_hash)?;
    let (validity, valid_to_us) = match mutation {
        TargetMutation::Invalidate => (
            ValidityStatus::Invalidated,
            Some(event.fragment.occurred_at.timestamp_micros()),
        ),
        TargetMutation::Dispute => (ValidityStatus::Disputed, None),
    };
    sqlx::query(
        "UPDATE state_versions
         SET validity = ?1, valid_to_us = COALESCE(?2, valid_to_us)
         WHERE id = ?3",
    )
    .bind(validity_name(validity))
    .bind(valid_to_us)
    .bind(target.version_id.to_string())
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        "UPDATE states SET current_version_id = NULL
         WHERE id = ?1 AND current_version_id = ?2",
    )
    .bind(target.state_id.to_string())
    .bind(target.version_id.to_string())
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn contradict_target(
    transaction: &mut Transaction<'_, Sqlite>,
    scope_hash: &str,
    event: &StoredProjectionEvent,
    state: Option<&StateDeclaration>,
) -> Result<(), StorageError> {
    let Some(target) =
        target_state_version(transaction, scope_hash, event, state.is_some()).await?
    else {
        return Ok(());
    };
    validate_mutation_key(&target, state, scope_hash)?;
    sqlx::query("UPDATE state_versions SET validity = 'disputed' WHERE id = ?1")
        .bind(target.version_id.to_string())
        .execute(&mut **transaction)
        .await?;
    clear_current_version(transaction, target.state_id).await?;
    if let Some(state) = state {
        if event.fragment.occurred_at < target.valid_from {
            return Err(event_corrupt("occurred_at", scope_hash));
        }
        insert_state_version(
            transaction,
            target.state_id,
            Some(target.version_id),
            event,
            state,
            ValidityStatus::Disputed,
        )
        .await?;
    }
    Ok(())
}

async fn supersede_target(
    transaction: &mut Transaction<'_, Sqlite>,
    scope_hash: &str,
    event: &StoredProjectionEvent,
    state: &StateDeclaration,
) -> Result<(), StorageError> {
    let target = target_state_version(transaction, scope_hash, event, true)
        .await?
        .ok_or_else(|| event_corrupt("target_fragment_id", scope_hash))?;
    validate_mutation_key(&target, Some(state), scope_hash)?;
    if event.fragment.occurred_at < target.valid_from {
        return Err(event_corrupt("occurred_at", scope_hash));
    }
    close_open_versions(
        transaction,
        target.state_id,
        event.fragment.occurred_at,
        ValidityStatus::Superseded,
    )
    .await?;
    insert_state_version(
        transaction,
        target.state_id,
        Some(target.version_id),
        event,
        state,
        event.fragment.validity,
    )
    .await?;
    set_current_version(transaction, target.state_id, event.fragment.id).await
}

async fn target_state_version(
    transaction: &mut Transaction<'_, Sqlite>,
    scope_hash: &str,
    event: &StoredProjectionEvent,
    required: bool,
) -> Result<Option<TargetStateVersion>, StorageError> {
    let target_id = mutation_target_id(&event.fragment, scope_hash)?;
    let row = sqlx::query(
        "SELECT s.id AS state_id, s.state_key, v.id AS version_id, v.valid_from_us
         FROM state_versions v
         JOIN states s ON s.id = v.state_id
         WHERE s.scope_key = ?1 AND v.source_fragment_id = ?2
         ORDER BY v.valid_from_us DESC, v.id DESC
         LIMIT 1",
    )
    .bind(scope_hash)
    .bind(target_id.to_string())
    .fetch_optional(&mut **transaction)
    .await?;
    if let Some(row) = row {
        let valid_from_us = row.try_get::<i64, _>("valid_from_us")?;
        return Ok(Some(TargetStateVersion {
            state_id: parse_state_id(
                &row.try_get::<String, _>("state_id")?,
                "state_id",
                scope_hash,
            )?,
            state_key: row.try_get("state_key")?,
            version_id: parse_version_id(
                &row.try_get::<String, _>("version_id")?,
                "version_id",
                scope_hash,
            )?,
            valid_from: DateTime::from_timestamp_micros(valid_from_us)
                .ok_or_else(|| chain_corrupt("valid_from", scope_hash))?,
        }));
    }
    let foreign_scope: Option<String> = sqlx::query_scalar(
        "SELECT s.scope_key
         FROM state_versions v JOIN states s ON s.id = v.state_id
         WHERE v.source_fragment_id = ?1 LIMIT 1",
    )
    .bind(target_id.to_string())
    .fetch_optional(&mut **transaction)
    .await?;
    if foreign_scope.is_some_and(|value| value != scope_hash) {
        return Err(StorageError::ProjectionScopeMismatch {
            projector: ProjectorKind::State,
            scope_hash: scope_hash.to_owned(),
        });
    }
    if required {
        return Err(event_corrupt("target_fragment_id", scope_hash));
    }
    Ok(None)
}

fn mutation_target_id(
    fragment: &CognitiveFragment,
    scope_hash: &str,
) -> Result<FragmentId, StorageError> {
    let raw = fragment
        .metadata
        .get("target_fragment_id")
        .and_then(Value::as_str)
        .ok_or_else(|| event_corrupt("target_fragment_id", scope_hash))?;
    FragmentId::from_str(raw).map_err(|_| event_corrupt("target_fragment_id", scope_hash))
}

fn validate_mutation_key(
    target: &TargetStateVersion,
    state: Option<&StateDeclaration>,
    scope_hash: &str,
) -> Result<(), StorageError> {
    if state.is_some_and(|state| state.key != target.state_key) {
        return Err(event_corrupt("state.key", scope_hash));
    }
    Ok(())
}

async fn insert_state_version(
    transaction: &mut Transaction<'_, Sqlite>,
    state_id: StateId,
    previous_version_id: Option<StateVersionId>,
    event: &StoredProjectionEvent,
    state: &StateDeclaration,
    validity: ValidityStatus,
) -> Result<(), StorageError> {
    sqlx::query(
        "INSERT INTO state_versions (
            id, state_id, value_json, valid_from_us, valid_to_us, validity,
            source_fragment_id, previous_version_id, created_at_us, reason, source_event_id
         ) VALUES (?1, ?2, ?3, ?4, NULL, ?5, ?6, ?7, ?8, ?9, ?10)
         ON CONFLICT(id) DO NOTHING",
    )
    .bind(event.fragment.id.to_string())
    .bind(state_id.to_string())
    .bind(canonical_json(&state.value)?)
    .bind(event.fragment.occurred_at.timestamp_micros())
    .bind(validity_name(validity))
    .bind(event.fragment.id.to_string())
    .bind(previous_version_id.map(|id| id.to_string()))
    .bind(event.recorded_at.timestamp_micros())
    .bind(&state.reason)
    .bind(event.id.to_string())
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn version_exists(
    transaction: &mut Transaction<'_, Sqlite>,
    fragment_id: FragmentId,
) -> Result<bool, StorageError> {
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM state_versions WHERE source_fragment_id = ?1")
            .bind(fragment_id.to_string())
            .fetch_one(&mut **transaction)
            .await?;
    Ok(count > 0)
}

async fn latest_version_id(
    transaction: &mut Transaction<'_, Sqlite>,
    state_id: StateId,
    scope_hash: &str,
) -> Result<Option<StateVersionId>, StorageError> {
    let raw: Option<String> = sqlx::query_scalar(
        "SELECT id FROM state_versions WHERE state_id = ?1
         ORDER BY valid_from_us DESC, id DESC LIMIT 1",
    )
    .bind(state_id.to_string())
    .fetch_optional(&mut **transaction)
    .await?;
    raw.map(|raw| parse_version_id(&raw, "previous_version_id", scope_hash))
        .transpose()
}

async fn close_open_versions(
    transaction: &mut Transaction<'_, Sqlite>,
    state_id: StateId,
    valid_to: DateTime<Utc>,
    validity: ValidityStatus,
) -> Result<(), StorageError> {
    sqlx::query(
        "UPDATE state_versions SET valid_to_us = ?1, validity = ?2
         WHERE state_id = ?3 AND valid_to_us IS NULL",
    )
    .bind(valid_to.timestamp_micros())
    .bind(validity_name(validity))
    .bind(state_id.to_string())
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn set_current_version(
    transaction: &mut Transaction<'_, Sqlite>,
    state_id: StateId,
    fragment_id: FragmentId,
) -> Result<(), StorageError> {
    sqlx::query("UPDATE states SET current_version_id = ?1 WHERE id = ?2")
        .bind(fragment_id.to_string())
        .bind(state_id.to_string())
        .execute(&mut **transaction)
        .await?;
    Ok(())
}

async fn clear_current_version(
    transaction: &mut Transaction<'_, Sqlite>,
    state_id: StateId,
) -> Result<(), StorageError> {
    sqlx::query("UPDATE states SET current_version_id = NULL WHERE id = ?1")
        .bind(state_id.to_string())
        .execute(&mut **transaction)
        .await?;
    Ok(())
}

#[derive(Debug)]
struct DecodedStateRow {
    state_id: StateId,
    version_id: StateVersionId,
    key: String,
    value: Value,
    valid_from: DateTime<Utc>,
    valid_to: Option<DateTime<Utc>>,
    validity: ValidityStatus,
    source_fragment_id: FragmentId,
    source_event_id: Option<EventId>,
    reason: Option<String>,
}

impl DecodedStateRow {
    fn to_view(&self, evidence: Vec<EvidenceView>) -> StateView {
        StateView {
            state_id: self.state_id,
            version_id: self.version_id,
            key: self.key.clone(),
            value: self.value.clone(),
            valid_from: self.valid_from,
            valid_to: self.valid_to,
            validity: self.validity,
            evidence,
            reason: self.reason.clone(),
            source_event_id: self.source_event_id,
        }
    }
}

fn decode_state_row(
    row: &sqlx::sqlite::SqliteRow,
    scope_hash: &str,
) -> Result<DecodedStateRow, StorageError> {
    let state_id = parse_state_id(
        &row.try_get::<String, _>("state_id")?,
        "state_id",
        scope_hash,
    )?;
    let version_id = parse_version_id(
        &row.try_get::<String, _>("version_id")?,
        "version_id",
        scope_hash,
    )?;
    let raw_value: String = row.try_get("value_json")?;
    let value =
        serde_json::from_str(&raw_value).map_err(|_| chain_corrupt("value_json", scope_hash))?;
    let valid_from_us: i64 = row.try_get("valid_from_us")?;
    let valid_from = DateTime::from_timestamp_micros(valid_from_us)
        .ok_or_else(|| chain_corrupt("valid_from", scope_hash))?;
    let valid_to_us: Option<i64> = row.try_get("valid_to_us")?;
    let valid_to = valid_to_us
        .map(|value| {
            DateTime::from_timestamp_micros(value)
                .ok_or_else(|| chain_corrupt("valid_to", scope_hash))
        })
        .transpose()?;
    let validity = parse_validity(&row.try_get::<String, _>("validity")?, scope_hash)?;
    let raw_fragment_id: String = row.try_get("source_fragment_id")?;
    let source_fragment_id = FragmentId::from_str(&raw_fragment_id)
        .map_err(|_| chain_corrupt("source_fragment_id", scope_hash))?;
    let source_event_id = row
        .try_get::<Option<String>, _>("source_event_id")?
        .map(|raw| {
            EventId::from_str(&raw).map_err(|_| chain_corrupt("source_event_id", scope_hash))
        })
        .transpose()?;
    Ok(DecodedStateRow {
        state_id,
        version_id,
        key: row.try_get("state_key")?,
        value,
        valid_from,
        valid_to,
        validity,
        source_fragment_id,
        source_event_id,
        reason: row.try_get("reason")?,
    })
}

async fn validate_state_chain(
    pool: &SqlitePool,
    state_id: StateId,
    scope_hash: &str,
) -> Result<(), StorageError> {
    let current: Option<String> = sqlx::query_scalar(
        "SELECT current_version_id FROM states WHERE id = ?1 AND scope_key = ?2",
    )
    .bind(state_id.to_string())
    .bind(scope_hash)
    .fetch_optional(pool)
    .await?
    .flatten();
    let active_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM state_versions
         WHERE state_id = ?1 AND valid_to_us IS NULL
           AND validity NOT IN ('disputed', 'superseded', 'invalidated', 'quarantined')",
    )
    .bind(state_id.to_string())
    .fetch_one(pool)
    .await?;
    if active_count != i64::from(current.is_some()) {
        return Err(chain_corrupt("current_version_id", scope_hash));
    }
    if let Some(current) = current {
        let row =
            sqlx::query("SELECT state_id, valid_to_us, validity FROM state_versions WHERE id = ?1")
                .bind(current)
                .fetch_optional(pool)
                .await?
                .ok_or_else(|| chain_corrupt("current_version_id", scope_hash))?;
        let owner: String = row.try_get("state_id")?;
        let valid_to: Option<i64> = row.try_get("valid_to_us")?;
        let validity: String = row.try_get("validity")?;
        if owner != state_id.to_string()
            || valid_to.is_some()
            || matches!(
                validity.as_str(),
                "disputed" | "superseded" | "invalidated" | "quarantined"
            )
        {
            return Err(chain_corrupt("current_version_id", scope_hash));
        }
    }
    let broken_previous: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)
         FROM state_versions child
         JOIN state_versions parent ON parent.id = child.previous_version_id
         WHERE child.state_id = ?1 AND parent.state_id != child.state_id",
    )
    .bind(state_id.to_string())
    .fetch_one(pool)
    .await?;
    if broken_previous != 0 {
        return Err(chain_corrupt("previous_version_id", scope_hash));
    }
    Ok(())
}

async fn load_state_evidence(
    pool: &SqlitePool,
    fragment_id: FragmentId,
    event_id: Option<EventId>,
) -> Result<Vec<EvidenceView>, StorageError> {
    let raw_ids: Vec<String> =
        sqlx::query_scalar("SELECT id FROM evidence_refs WHERE owner_fragment_id = ?1 ORDER BY id")
            .bind(fragment_id.to_string())
            .fetch_all(pool)
            .await?;
    if raw_ids.is_empty() {
        return Ok(vec![EvidenceView {
            fragment_id,
            event_id,
            evidence_ref_id: None,
        }]);
    }
    raw_ids
        .into_iter()
        .map(|raw| {
            let evidence_ref_id =
                EvidenceId::from_str(&raw).map_err(|_| StorageError::CorruptRow {
                    table: "evidence_refs",
                    id: fragment_id.to_string(),
                    reason: "id is not a UUID".to_owned(),
                })?;
            Ok(EvidenceView {
                fragment_id,
                event_id,
                evidence_ref_id: Some(evidence_ref_id),
            })
        })
        .collect()
}

async fn select_state_rows(
    pool: &SqlitePool,
    scope_hash: &str,
    selector: StateSelector,
    limit: i64,
) -> Result<Vec<sqlx::sqlite::SqliteRow>, StorageError> {
    let mut query = QueryBuilder::<Sqlite>::new(
        "SELECT s.id AS state_id, s.state_key, s.current_version_id,
                v.id AS version_id, v.value_json, v.valid_from_us, v.valid_to_us,
                v.validity, v.source_fragment_id, v.source_event_id, v.reason
         FROM states s
         JOIN state_versions v ON v.state_id = s.id
         WHERE s.scope_key = ",
    );
    query.push_bind(scope_hash);
    match selector {
        StateSelector::ExactKey(key) => {
            query.push(" AND s.state_key = ").push_bind(key);
        }
        StateSelector::SourceFragments(ids) => {
            query.push(" AND v.source_fragment_id IN (");
            let mut separated = query.separated(", ");
            for id in ids {
                separated.push_bind(id.to_string());
            }
            separated.push_unseparated(")");
        }
    }
    query.push(" ORDER BY v.valid_from_us DESC, v.id DESC LIMIT ");
    query.push_bind(limit);
    Ok(query.build().fetch_all(pool).await?)
}

async fn select_graph_edges(
    pool: &SqlitePool,
    scope_hash: &str,
    seed_ids: &[PointId],
    allowed_kinds: &[EdgeKind],
    effective_at: DateTime<Utc>,
    max_nodes: usize,
) -> Result<Vec<sqlx::sqlite::SqliteRow>, StorageError> {
    let mut query = QueryBuilder::<Sqlite>::new(
        "SELECT e.id, e.scope_key, e.source_point_id, e.target_point_id, e.kind,
                e.weight, e.confidence, e.source_fragment_id, e.valid_from_us,
                e.valid_to_us, e.model_version_id, e.human_confirmed, e.tool_verified,
                (SELECT ev.id FROM events ev
                 WHERE ev.scope_key = e.scope_key
                   AND ev.fragment_id = e.source_fragment_id
                 ORDER BY ev.sequence, ev.id LIMIT 1) AS source_event_id,
                (SELECT er.id FROM evidence_refs er
                 WHERE er.owner_fragment_id = e.source_fragment_id
                   AND (
                       er.referenced_fragment_id = (
                           SELECT target.source_fragment_id FROM points target
                           WHERE target.id = e.target_point_id
                             AND target.scope_key = e.scope_key
                       )
                       OR er.referenced_event_id IN (
                           SELECT target_event.id
                           FROM points target
                           JOIN events target_event
                             ON target_event.fragment_id = target.source_fragment_id
                            AND target_event.scope_key = target.scope_key
                           WHERE target.id = e.target_point_id
                             AND target.scope_key = e.scope_key
                       )
                   )
                 ORDER BY er.created_at_us, er.id LIMIT 1) AS evidence_ref_id
         FROM edges e WHERE e.scope_key = ",
    );
    query.push_bind(scope_hash);
    query.push(" AND (e.source_point_id IN (");
    let mut sources = query.separated(", ");
    for id in seed_ids {
        sources.push_bind(id.to_string());
    }
    sources.push_unseparated(") OR e.target_point_id IN (");
    let mut targets = query.separated(", ");
    for id in seed_ids {
        targets.push_bind(id.to_string());
    }
    targets.push_unseparated(")) AND e.kind IN (");
    let mut kinds = query.separated(", ");
    for kind in allowed_kinds {
        kinds.push_bind(edge_kind_name(*kind));
    }
    kinds.push_unseparated(") AND e.valid_from_us <= ");
    query.push_bind(effective_at.timestamp_micros());
    query.push(" AND (e.valid_to_us IS NULL OR ");
    query
        .push_bind(effective_at.timestamp_micros())
        .push(" < e.valid_to_us) ORDER BY e.weight DESC, e.id LIMIT ")
        .push_bind(i64::try_from(max_nodes).unwrap_or(i64::MAX));
    Ok(query.build().fetch_all(pool).await?)
}

async fn load_projected_points(
    pool: &SqlitePool,
    scope: &MemoryScope,
    scope_hash: &str,
    point_ids: &[PointId],
    effective_at: DateTime<Utc>,
) -> Result<BTreeMap<PointId, ProjectedPoint>, StorageError> {
    if point_ids.is_empty() {
        return Ok(BTreeMap::new());
    }
    let mut query = QueryBuilder::<Sqlite>::new(
        "SELECT p.id, p.scope_key, p.kind, p.source_fragment_id, p.valid_from_us,
                p.valid_to_us, p.confidence, ev.payload_json
         FROM points p
         JOIN events ev ON ev.id = (
             SELECT candidate.id FROM events candidate
             WHERE candidate.scope_key = p.scope_key
               AND candidate.fragment_id = p.source_fragment_id
             ORDER BY candidate.sequence, candidate.id LIMIT 1
         )
         WHERE p.scope_key = ",
    );
    query.push_bind(scope_hash).push(" AND p.id IN (");
    let mut ids = query.separated(", ");
    for id in point_ids {
        ids.push_bind(id.to_string());
    }
    ids.push_unseparated(") AND p.valid_from_us <= ");
    query.push_bind(effective_at.timestamp_micros());
    query.push(" AND (p.valid_to_us IS NULL OR ");
    query
        .push_bind(effective_at.timestamp_micros())
        .push(" < p.valid_to_us) ORDER BY p.valid_from_us, p.id");
    let rows = query.build().fetch_all(pool).await?;
    rows.into_iter()
        .map(|row| {
            let point = decode_projected_point(&row, scope, scope_hash)?;
            Ok((point.point.id, point))
        })
        .collect()
}

fn decode_projected_point(
    row: &sqlx::sqlite::SqliteRow,
    scope: &MemoryScope,
    scope_hash: &str,
) -> Result<ProjectedPoint, StorageError> {
    if row.try_get::<String, _>("scope_key")? != scope_hash {
        return Err(StorageError::ProjectionScopeMismatch {
            projector: ProjectorKind::Graph,
            scope_hash: scope_hash.to_owned(),
        });
    }
    let payload = row.try_get::<String, _>("payload_json")?;
    let fragment: CognitiveFragment = serde_json::from_str(&payload)
        .map_err(|_| graph_event_corrupt("point_payload", scope_hash))?;
    let source_fragment_id = FragmentId::from_str(&row.try_get::<String, _>("source_fragment_id")?)
        .map_err(|_| graph_event_corrupt("point_source_fragment_id", scope_hash))?;
    if fragment.id != source_fragment_id || fragment.scope != *scope {
        return Err(StorageError::ProjectionScopeMismatch {
            projector: ProjectorKind::Graph,
            scope_hash: scope_hash.to_owned(),
        });
    }
    let point_id = PointId::from_str(&row.try_get::<String, _>("id")?)
        .map_err(|_| graph_event_corrupt("point_id", scope_hash))?;
    let raw_kind = row.try_get::<String, _>("kind")?;
    let kind: PointKind = serde_json::from_value(Value::String(raw_kind))
        .map_err(|_| graph_event_corrupt("point_kind", scope_hash))?;
    if PointKind::try_from(fragment.kind).ok() != Some(kind) {
        return Err(graph_event_corrupt("point_kind", scope_hash));
    }
    let valid_from_us = row.try_get::<i64, _>("valid_from_us")?;
    let occurred_at = DateTime::from_timestamp_micros(valid_from_us)
        .ok_or_else(|| graph_event_corrupt("point_valid_from", scope_hash))?;
    let confidence = decode_graph_real(row.try_get("confidence")?, "point_confidence", scope_hash)?;
    if valid_from_us != fragment.occurred_at.timestamp_micros()
        || !confidence.is_finite()
        || !(0.0..=1.0).contains(&confidence)
    {
        return Err(graph_event_corrupt("point_projection", scope_hash));
    }
    Ok(ProjectedPoint {
        point: CognitivePoint {
            id: point_id,
            scope: scope.clone(),
            kind,
            source_fragments: vec![source_fragment_id],
            occurred_at,
            confidence,
            validity: fragment.validity,
        },
        text: fragment_text(&fragment.content),
        entities: fragment_entities(&fragment),
    })
}

fn decode_graph_edge(
    row: &sqlx::sqlite::SqliteRow,
    scope: &MemoryScope,
    scope_hash: &str,
) -> Result<CognitiveEdge, StorageError> {
    if row.try_get::<String, _>("scope_key")? != scope_hash {
        return Err(StorageError::ProjectionScopeMismatch {
            projector: ProjectorKind::Graph,
            scope_hash: scope_hash.to_owned(),
        });
    }
    let id = EdgeId::from_str(&row.try_get::<String, _>("id")?)
        .map_err(|_| graph_event_corrupt("edge_id", scope_hash))?;
    let source_point_id = PointId::from_str(&row.try_get::<String, _>("source_point_id")?)
        .map_err(|_| graph_event_corrupt("source_point_id", scope_hash))?;
    let target_point_id = PointId::from_str(&row.try_get::<String, _>("target_point_id")?)
        .map_err(|_| graph_event_corrupt("target_point_id", scope_hash))?;
    if source_point_id == target_point_id {
        return Err(graph_event_corrupt("edge_endpoints", scope_hash));
    }
    let kind = serde_json::from_value(Value::String(row.try_get::<String, _>("kind")?))
        .map_err(|_| graph_event_corrupt("edge_kind", scope_hash))?;
    let weight = decode_graph_real(row.try_get("weight")?, "edge_weight", scope_hash)?;
    let confidence = decode_graph_real(row.try_get("confidence")?, "edge_confidence", scope_hash)?;
    if !unit_interval(weight) || !unit_interval(confidence) {
        return Err(graph_event_corrupt("edge_weight", scope_hash));
    }
    let valid_from = DateTime::from_timestamp_micros(row.try_get("valid_from_us")?)
        .ok_or_else(|| graph_event_corrupt("edge_valid_from", scope_hash))?;
    let valid_to = row
        .try_get::<Option<i64>, _>("valid_to_us")?
        .map(|value| {
            DateTime::from_timestamp_micros(value)
                .ok_or_else(|| graph_event_corrupt("edge_valid_to", scope_hash))
        })
        .transpose()?;
    if valid_to.is_some_and(|value| value < valid_from) {
        return Err(graph_event_corrupt("edge_valid_to", scope_hash));
    }
    let human_confirmed = decode_graph_flag(row, "human_confirmed", scope_hash)?;
    let tool_verified = decode_graph_flag(row, "tool_verified", scope_hash)?;
    let source_fragment_id = FragmentId::from_str(&row.try_get::<String, _>("source_fragment_id")?)
        .map_err(|_| graph_event_corrupt("edge_source_fragment_id", scope_hash))?;
    let source_event_id = row
        .try_get::<Option<String>, _>("source_event_id")?
        .ok_or_else(|| graph_event_corrupt("edge_source_event_id", scope_hash))?;
    let source_event_id = EventId::from_str(&source_event_id)
        .map_err(|_| graph_event_corrupt("edge_source_event_id", scope_hash))?;
    let evidence_ref_id = row
        .try_get::<Option<String>, _>("evidence_ref_id")?
        .map(|raw| {
            EvidenceId::from_str(&raw)
                .map_err(|_| graph_event_corrupt("edge_evidence_ref_id", scope_hash))
        })
        .transpose()?;
    Ok(CognitiveEdge {
        id,
        scope: scope.clone(),
        source_point_id,
        target_point_id,
        kind,
        weight,
        confidence,
        evidence: vec![EvidenceLink {
            fragment_id: source_fragment_id,
            event_id: Some(source_event_id),
            evidence_ref_id,
        }],
        valid_from,
        valid_to,
        model_version_id: row.try_get("model_version_id")?,
        human_confirmed,
        tool_verified,
    })
}

fn decode_graph_flag(
    row: &sqlx::sqlite::SqliteRow,
    field: &'static str,
    scope_hash: &str,
) -> Result<bool, StorageError> {
    match row.try_get::<i64, _>(field)? {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(graph_event_corrupt(field, scope_hash)),
    }
}

fn fragment_text(content: &FragmentContent) -> String {
    match content {
        FragmentContent::Text { text } => text.clone(),
        FragmentContent::Json { value } => value.to_string(),
        FragmentContent::Blob { blob_id, .. } => blob_id.to_string(),
    }
}

fn fragment_entities(fragment: &CognitiveFragment) -> Vec<String> {
    fragment
        .metadata
        .get("entities")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(ToOwned::to_owned)
        .collect()
}

fn unit_interval(value: f32) -> bool {
    value.is_finite() && (0.0..=1.0).contains(&value)
}

fn decode_graph_real(
    value: f64,
    field: &'static str,
    scope_hash: &str,
) -> Result<f32, StorageError> {
    #[allow(clippy::cast_possible_truncation)]
    let narrowed = value as f32;
    if value.to_bits() == f64::from(narrowed).to_bits() {
        Ok(narrowed)
    } else {
        Err(graph_event_corrupt(field, scope_hash))
    }
}

fn state_is_recall_visible(validity: ValidityStatus) -> bool {
    !matches!(
        validity,
        ValidityStatus::Invalidated | ValidityStatus::Quarantined
    )
}

fn canonical_json(value: &Value) -> Result<String, StorageError> {
    Ok(serde_json::to_string(&canonicalize(value.clone()))?)
}

const fn validity_name(validity: ValidityStatus) -> &'static str {
    match validity {
        ValidityStatus::Unverified => "unverified",
        ValidityStatus::Verified => "verified",
        ValidityStatus::Disputed => "disputed",
        ValidityStatus::Superseded => "superseded",
        ValidityStatus::Invalidated => "invalidated",
        ValidityStatus::Quarantined => "quarantined",
    }
}

fn parse_validity(raw: &str, scope_hash: &str) -> Result<ValidityStatus, StorageError> {
    match raw {
        "unverified" => Ok(ValidityStatus::Unverified),
        "verified" => Ok(ValidityStatus::Verified),
        "disputed" => Ok(ValidityStatus::Disputed),
        "superseded" => Ok(ValidityStatus::Superseded),
        "invalidated" => Ok(ValidityStatus::Invalidated),
        "quarantined" => Ok(ValidityStatus::Quarantined),
        _ => Err(chain_corrupt("validity", scope_hash)),
    }
}

fn state_id(fragment_id: FragmentId, scope_hash: &str) -> Result<StateId, StorageError> {
    parse_state_id(&fragment_id.to_string(), "state_id", scope_hash)
}

fn version_id(fragment_id: FragmentId, scope_hash: &str) -> Result<StateVersionId, StorageError> {
    parse_version_id(&fragment_id.to_string(), "version_id", scope_hash)
}

fn point_id(fragment_id: FragmentId, scope_hash: &str) -> Result<PointId, StorageError> {
    PointId::from_str(&fragment_id.to_string()).map_err(|_| chain_corrupt("point_id", scope_hash))
}

fn edge_id(event_id: EventId, scope_hash: &str) -> Result<EdgeId, StorageError> {
    EdgeId::from_str(&event_id.to_string()).map_err(|_| graph_event_corrupt("edge_id", scope_hash))
}

const fn point_kind_name(kind: PointKind) -> &'static str {
    match kind {
        PointKind::Fact => "fact",
        PointKind::Decision => "decision",
        PointKind::Constraint => "constraint",
        PointKind::StateChange => "state_change",
        PointKind::Action => "action",
        PointKind::Result => "result",
        PointKind::Failure => "failure",
        PointKind::Preference => "preference",
        PointKind::Correction => "correction",
    }
}

const fn edge_kind_name(kind: EdgeKind) -> &'static str {
    match kind {
        EdgeKind::TemporalBefore => "temporal_before",
        EdgeKind::TemporalAfter => "temporal_after",
        EdgeKind::Causes => "causes",
        EdgeKind::CausedBy => "caused_by",
        EdgeKind::Supports => "supports",
        EdgeKind::Contradicts => "contradicts",
        EdgeKind::Supersedes => "supersedes",
        EdgeKind::DependsOn => "depends_on",
        EdgeKind::PartOf => "part_of",
        EdgeKind::SameEntity => "same_entity",
        EdgeKind::CoActivated => "co_activated",
        EdgeKind::PredictedBy => "predicted_by",
        EdgeKind::ValidatedBy => "validated_by",
        EdgeKind::DisprovedBy => "disproved_by",
        EdgeKind::RecalledBy => "recalled_by",
        EdgeKind::UsedInDecision => "used_in_decision",
        EdgeKind::ExecutedAs => "executed_as",
        EdgeKind::ResultedIn => "resulted_in",
    }
}

fn parse_state_id(
    raw: &str,
    field: &'static str,
    scope_hash: &str,
) -> Result<StateId, StorageError> {
    StateId::from_str(raw).map_err(|_| chain_corrupt(field, scope_hash))
}

fn parse_version_id(
    raw: &str,
    field: &'static str,
    scope_hash: &str,
) -> Result<StateVersionId, StorageError> {
    StateVersionId::from_str(raw).map_err(|_| chain_corrupt(field, scope_hash))
}

fn event_corrupt(field: &'static str, scope_hash: &str) -> StorageError {
    StorageError::ProjectionEventCorrupt {
        projector: ProjectorKind::State,
        field,
        scope_hash: scope_hash.to_owned(),
    }
}

fn graph_event_corrupt(field: &'static str, scope_hash: &str) -> StorageError {
    StorageError::ProjectionEventCorrupt {
        projector: ProjectorKind::Graph,
        field,
        scope_hash: scope_hash.to_owned(),
    }
}

fn checkpoint_corrupt(projector: ProjectorKind, scope_hash: &str) -> StorageError {
    StorageError::ProjectionCheckpointCorrupt {
        projector,
        scope_hash: scope_hash.to_owned(),
    }
}

fn chain_corrupt(field: &'static str, scope_hash: &str) -> StorageError {
    StorageError::StateChainCorrupt {
        field,
        scope_hash: scope_hash.to_owned(),
    }
}

impl ProjectorKind {
    /// 返回持久化到 Checkpoint 表中的稳定标识。
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::State => "state",
            Self::Graph => "graph",
        }
    }
}

impl fmt::Display for ProjectorKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// 一个 Projector 在精确 Scope 内已提交的位置。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectionCheckpoint {
    /// Checkpoint 所属的投影类别。
    pub projector: ProjectorKind,
    /// 已完整提交的最大 Event sequence。
    pub sequence: u64,
    /// Checkpoint 最后更新时间。
    pub updated_at: DateTime<Utc>,
}

/// 一次 Recall 固定追赶到的 Event sequence。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProjectionTarget {
    /// 追赶开始时 Scope 内可见的最大 Event sequence。
    pub sequence: u64,
}

/// State 读取时使用的受限选择器。
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StateSelector {
    /// 读取一个完整且精确匹配的 State key。
    ExactKey(String),
    /// 读取由指定权威 Fragment 产生的 `StateVersion`。
    SourceFragments(Vec<FragmentId>),
}

/// 一个有界 State 读取返回的当前值、历史值、冲突与证据。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProjectedStateSet {
    /// 在查询有效时间点成立的状态版本。
    pub current: Vec<StateView>,
    /// 查询有效时间点之前已经结束的相关版本。
    pub history: Vec<StateView>,
    /// 同刻竞争且尚未解决的显式冲突。
    pub conflicts: Vec<ConflictView>,
    /// 本次返回状态引用的去重权威证据。
    pub evidence: Vec<EvidenceView>,
    /// 是否因 `max_nodes` 排除了候选版本。
    pub truncated: bool,
}

/// 从 `SQLite` 投影中重建的可激活 Point 及其检索文本。
#[derive(Clone, Debug, PartialEq)]
pub struct ProjectedPoint {
    /// 保留权威 Fragment 来源的认知 Point。
    pub point: CognitivePoint,
    /// 不经摘要替换的原始可检索文本。
    pub text: String,
    /// 调用方显式提供的实体名称。
    pub entities: Vec<String>,
}

/// 一次受 Scope、有效时间和节点预算限制的持久化图读取。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProjectedGraph {
    /// 本次预算内加载的 Point。
    pub points: Vec<ProjectedPoint>,
    /// 两端 Point 均在本次结果内的 Edge。
    pub edges: Vec<CognitiveEdge>,
}

/// 用于证明投影事务在提交前完整回滚的故障边界。
pub trait ProjectionFailpoint: Send + Sync {
    /// 在派生行与 Checkpoint 写入后、事务提交前运行。
    ///
    /// # Errors
    ///
    /// 返回错误时调用方必须回滚整个投影事务。
    fn before_commit(
        &self,
        projector: ProjectorKind,
        next_sequence: u64,
    ) -> Result<(), StorageError>;
}

#[derive(Debug)]
struct NoProjectionFailpoint;

impl ProjectionFailpoint for NoProjectionFailpoint {
    fn before_commit(
        &self,
        _projector: ProjectorKind,
        _next_sequence: u64,
    ) -> Result<(), StorageError> {
        Ok(())
    }
}

/// 共享 `SQLite` 连接池的 State 与 Graph 投影存储。
#[derive(Clone)]
pub struct SqliteProjectionStore {
    pool: SqlitePool,
    failpoint: Arc<dyn ProjectionFailpoint>,
}

impl fmt::Debug for SqliteProjectionStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SqliteProjectionStore")
            .field("pool", &self.pool)
            .field("failpoint_refs", &Arc::strong_count(&self.failpoint))
            .finish()
    }
}

impl SqliteProjectionStore {
    /// 从现有 `SQLite` 连接池创建投影存储。
    #[must_use]
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            pool,
            failpoint: Arc::new(NoProjectionFailpoint),
        }
    }

    /// 共享已经打开的 `SqliteStore` 连接池。
    #[must_use]
    pub fn from_store(store: &SqliteStore) -> Self {
        Self::new(store.pool().clone())
    }

    /// 替换事务提交前的确定性故障钩子。
    #[must_use]
    pub fn with_failpoint(mut self, failpoint: Arc<dyn ProjectionFailpoint>) -> Self {
        self.failpoint = failpoint;
        self
    }

    /// 固定当前精确 Scope 的最大权威 Event sequence。
    ///
    /// # Errors
    ///
    /// Scope 无法 canonical 序列化、查询失败或 sequence 损坏时返回错误。
    pub async fn target(&self, scope: &MemoryScope) -> Result<ProjectionTarget, StorageError> {
        let scope_hash = canonical_scope_key(scope)?;
        let sequence: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(sequence), 0) FROM events WHERE scope_key = ?1",
        )
        .bind(&scope_hash)
        .fetch_one(&self.pool)
        .await?;
        let sequence =
            u64::try_from(sequence).map_err(|_| StorageError::ProjectionEventCorrupt {
                projector: ProjectorKind::State,
                field: "sequence",
                scope_hash,
            })?;
        Ok(ProjectionTarget { sequence })
    }

    /// 读取一个 Projector 在精确 Scope 内的已提交位置。
    ///
    /// # Errors
    ///
    /// Scope 无法 canonical 序列化、查询失败或 Checkpoint 损坏时返回错误。
    pub async fn checkpoint(
        &self,
        scope: &MemoryScope,
        projector: ProjectorKind,
    ) -> Result<ProjectionCheckpoint, StorageError> {
        let scope_hash = canonical_scope_key(scope)?;
        let row = sqlx::query(
            "SELECT sequence, updated_at_us
             FROM projection_checkpoints
             WHERE scope_key = ?1 AND projector = ?2",
        )
        .bind(&scope_hash)
        .bind(projector.as_str())
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else {
            return Ok(ProjectionCheckpoint {
                projector,
                sequence: 0,
                updated_at: DateTime::<Utc>::UNIX_EPOCH,
            });
        };
        let sequence = row.try_get::<i64, _>("sequence")?;
        let updated_at_us = row.try_get::<i64, _>("updated_at_us")?;
        let sequence =
            u64::try_from(sequence).map_err(|_| StorageError::ProjectionCheckpointCorrupt {
                projector,
                scope_hash: scope_hash.clone(),
            })?;
        let updated_at = DateTime::from_timestamp_micros(updated_at_us).ok_or_else(|| {
            StorageError::ProjectionCheckpointCorrupt {
                projector,
                scope_hash,
            }
        })?;
        Ok(ProjectionCheckpoint {
            projector,
            sequence,
            updated_at,
        })
    }

    /// 在固定目标内分批追平精确 Scope 的 State 投影。
    ///
    /// # Errors
    ///
    /// Event、State 链或 Checkpoint 损坏，以及任一事务失败时返回错误。
    pub async fn catch_up_state(
        &self,
        scope: &MemoryScope,
        target: ProjectionTarget,
        batch_size: u32,
    ) -> Result<ProjectionCheckpoint, StorageError> {
        let scope_hash = canonical_scope_key(scope)?;
        let mut checkpoint = self.checkpoint(scope, ProjectorKind::State).await?;
        while self
            .state_projection_is_pending(&scope_hash, target)
            .await?
        {
            checkpoint = self
                .project_state_batch(scope, target, batch_size.max(1))
                .await?;
        }
        Ok(checkpoint)
    }

    async fn state_projection_is_pending(
        &self,
        scope_hash: &str,
        target: ProjectionTarget,
    ) -> Result<bool, StorageError> {
        let checkpoint: Option<i64> = sqlx::query_scalar(
            "SELECT sequence FROM projection_checkpoints
             WHERE scope_key = ?1 AND projector = 'state'",
        )
        .bind(scope_hash)
        .fetch_optional(&self.pool)
        .await?;
        if let Some(checkpoint) = checkpoint {
            let checkpoint = u64::try_from(checkpoint)
                .map_err(|_| checkpoint_corrupt(ProjectorKind::State, scope_hash))?;
            return Ok(checkpoint < target.sequence);
        }
        let target =
            i64::try_from(target.sequence).map_err(|_| event_corrupt("sequence", scope_hash))?;
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(
                SELECT 1 FROM events WHERE scope_key = ?1 AND sequence <= ?2
             )",
        )
        .bind(scope_hash)
        .bind(target)
        .fetch_one(&self.pool)
        .await?;
        Ok(exists)
    }

    /// 在固定目标内分批追平精确 Scope 的 Graph 投影。
    ///
    /// # Errors
    ///
    /// Event、Point、Edge 或 Checkpoint 损坏，以及任一事务失败时返回错误。
    pub async fn catch_up_graph(
        &self,
        scope: &MemoryScope,
        target: ProjectionTarget,
        batch_size: u32,
    ) -> Result<ProjectionCheckpoint, StorageError> {
        let scope_hash = canonical_scope_key(scope)?;
        let mut checkpoint = self.checkpoint(scope, ProjectorKind::Graph).await?;
        while self
            .graph_projection_is_pending(&scope_hash, target)
            .await?
        {
            checkpoint = self
                .project_graph_batch(scope, target, batch_size.max(1))
                .await?;
        }
        Ok(checkpoint)
    }

    async fn graph_projection_is_pending(
        &self,
        scope_hash: &str,
        target: ProjectionTarget,
    ) -> Result<bool, StorageError> {
        let checkpoint: Option<i64> = sqlx::query_scalar(
            "SELECT sequence FROM projection_checkpoints
             WHERE scope_key = ?1 AND projector = 'graph'",
        )
        .bind(scope_hash)
        .fetch_optional(&self.pool)
        .await?;
        if let Some(checkpoint) = checkpoint {
            let checkpoint = u64::try_from(checkpoint)
                .map_err(|_| checkpoint_corrupt(ProjectorKind::Graph, scope_hash))?;
            return Ok(checkpoint < target.sequence);
        }
        let target = i64::try_from(target.sequence)
            .map_err(|_| graph_event_corrupt("sequence", scope_hash))?;
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(
                SELECT 1 FROM events WHERE scope_key = ?1 AND sequence <= ?2
             )",
        )
        .bind(scope_hash)
        .bind(target)
        .fetch_one(&self.pool)
        .await?;
        Ok(exists)
    }

    /// 按 key 或来源 Fragment 有界读取 State 投影。
    ///
    /// # Errors
    ///
    /// 查询失败、标识/时间无法解码或 State 链不满足结构不变量时返回错误。
    pub async fn load_states(
        &self,
        scope: &MemoryScope,
        selector: StateSelector,
        effective_at: DateTime<Utc>,
        max_nodes: usize,
    ) -> Result<ProjectedStateSet, StorageError> {
        if matches!(&selector, StateSelector::SourceFragments(ids) if ids.is_empty()) {
            return Ok(ProjectedStateSet::default());
        }
        let scope_hash = canonical_scope_key(scope)?;
        let limit = i64::try_from(max_nodes.saturating_add(1)).unwrap_or(i64::MAX);
        let rows = select_state_rows(&self.pool, &scope_hash, selector, limit).await?;
        let truncated = rows.len() > max_nodes;
        let mut decoded = rows
            .into_iter()
            .take(max_nodes)
            .map(|row| decode_state_row(&row, &scope_hash))
            .collect::<Result<Vec<_>, _>>()?;

        let state_ids: BTreeSet<_> = decoded.iter().map(|row| row.state_id).collect();
        for state_id in state_ids {
            validate_state_chain(&self.pool, state_id, &scope_hash).await?;
        }

        let mut result = ProjectedStateSet {
            truncated,
            ..ProjectedStateSet::default()
        };
        let mut evidence_set = BTreeSet::new();
        let mut conflict_groups: BTreeMap<StateId, Vec<(PointId, Vec<EvidenceView>)>> =
            BTreeMap::new();
        for row in &mut decoded {
            let evidence =
                load_state_evidence(&self.pool, row.source_fragment_id, row.source_event_id)
                    .await?;
            for item in &evidence {
                evidence_set.insert((item.fragment_id, item.event_id, item.evidence_ref_id));
            }
            let view = row.to_view(evidence.clone());
            let effective = row.valid_from <= effective_at
                && row.valid_to.is_none_or(|valid_to| effective_at < valid_to);
            if effective && state_is_recall_visible(row.validity) {
                if row.validity == ValidityStatus::Disputed {
                    conflict_groups
                        .entry(row.state_id)
                        .or_default()
                        .push((point_id(row.source_fragment_id, &scope_hash)?, evidence));
                }
                result.current.push(view);
            } else if row.valid_from < effective_at {
                result.history.push(view);
            }
        }
        for (_, members) in conflict_groups {
            if members.len() < 2 {
                continue;
            }
            let mut point_ids: Vec<_> = members.iter().map(|(point_id, _)| *point_id).collect();
            point_ids.sort_unstable();
            let mut evidence = members
                .into_iter()
                .flat_map(|(_, evidence)| evidence)
                .collect::<Vec<_>>();
            evidence.sort_by_key(|item| (item.fragment_id, item.event_id, item.evidence_ref_id));
            evidence.dedup();
            result.conflicts.push(ConflictView {
                point_ids,
                edge_ids: Vec::new(),
                evidence,
            });
        }
        result.evidence = evidence_set
            .into_iter()
            .map(|(fragment_id, event_id, evidence_ref_id)| EvidenceView {
                fragment_id,
                event_id,
                evidence_ref_id,
            })
            .collect();
        Ok(result)
    }

    /// 按 seed Fragment、关系白名单和有效时间有界读取精确 Scope 图。
    ///
    /// # Errors
    ///
    /// 查询失败、派生行无法解码或任一 Point/Edge 不属于精确 Scope 时返回错误。
    pub async fn load_graph(
        &self,
        scope: &MemoryScope,
        seed_fragments: &[FragmentId],
        allowed_kinds: &[EdgeKind],
        effective_at: DateTime<Utc>,
        max_nodes: usize,
    ) -> Result<ProjectedGraph, StorageError> {
        if seed_fragments.is_empty() || max_nodes == 0 {
            return Ok(ProjectedGraph::default());
        }
        let scope_hash = canonical_scope_key(scope)?;
        let mut seed_ids = Vec::new();
        let mut unique_seeds = BTreeSet::new();
        for fragment_id in seed_fragments {
            let id = point_id(*fragment_id, &scope_hash)?;
            if unique_seeds.insert(id) {
                seed_ids.push(id);
            }
        }
        let edge_rows = if allowed_kinds.is_empty() {
            Vec::new()
        } else {
            select_graph_edges(
                &self.pool,
                &scope_hash,
                &seed_ids,
                allowed_kinds,
                effective_at,
                max_nodes,
            )
            .await?
        };
        let mut edges = edge_rows
            .iter()
            .map(|row| decode_graph_edge(row, scope, &scope_hash))
            .collect::<Result<Vec<_>, _>>()?;
        let mut requested_ids = seed_ids.clone();
        for edge in &edges {
            requested_ids.push(edge.source_point_id);
            requested_ids.push(edge.target_point_id);
        }
        requested_ids.sort_unstable();
        requested_ids.dedup();
        let mut available =
            load_projected_points(&self.pool, scope, &scope_hash, &requested_ids, effective_at)
                .await?;

        let mut graph = ProjectedGraph::default();
        let mut selected = BTreeSet::new();
        for id in seed_ids {
            if graph.points.len() == max_nodes {
                break;
            }
            if let Some(point) = available.remove(&id) {
                selected.insert(id);
                graph.points.push(point);
            }
        }
        for edge in edges.drain(..) {
            let missing = usize::from(!selected.contains(&edge.source_point_id))
                + usize::from(!selected.contains(&edge.target_point_id));
            if graph.points.len().saturating_add(missing) > max_nodes {
                continue;
            }
            if !selected.contains(&edge.source_point_id) {
                let Some(point) = available.remove(&edge.source_point_id) else {
                    continue;
                };
                selected.insert(edge.source_point_id);
                graph.points.push(point);
            }
            if !selected.contains(&edge.target_point_id) {
                let Some(point) = available.remove(&edge.target_point_id) else {
                    continue;
                };
                selected.insert(edge.target_point_id);
                graph.points.push(point);
            }
            if selected.contains(&edge.source_point_id)
                && selected.contains(&edge.target_point_id)
                && edge.scope == *scope
            {
                graph.edges.push(edge);
            }
        }
        if graph.points.iter().any(|item| item.point.scope != *scope) {
            return Err(StorageError::ProjectionScopeMismatch {
                projector: ProjectorKind::Graph,
                scope_hash,
            });
        }
        Ok(graph)
    }

    async fn project_graph_batch(
        &self,
        scope: &MemoryScope,
        target: ProjectionTarget,
        batch_size: u32,
    ) -> Result<ProjectionCheckpoint, StorageError> {
        let scope_hash = canonical_scope_key(scope)?;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let result = self
            .project_graph_batch_inner(&mut transaction, scope, &scope_hash, target, batch_size)
            .await;
        match result {
            Ok(checkpoint) => {
                transaction.commit().await?;
                Ok(checkpoint)
            }
            Err(error) => {
                let _ = transaction.rollback().await;
                Err(error)
            }
        }
    }

    async fn project_graph_batch_inner(
        &self,
        transaction: &mut Transaction<'_, Sqlite>,
        scope: &MemoryScope,
        scope_hash: &str,
        target: ProjectionTarget,
        batch_size: u32,
    ) -> Result<ProjectionCheckpoint, StorageError> {
        let checkpoint = graph_checkpoint_in_transaction(transaction, scope_hash).await?;
        if checkpoint.is_some_and(|sequence| sequence >= target.sequence) {
            return Ok(graph_checkpoint_value(
                checkpoint.unwrap_or(0),
                DateTime::<Utc>::UNIX_EPOCH,
            ));
        }
        let checkpoint_i64 = checkpoint
            .map(|sequence| {
                i64::try_from(sequence)
                    .map_err(|_| checkpoint_corrupt(ProjectorKind::Graph, scope_hash))
            })
            .transpose()?;
        let target_i64 = i64::try_from(target.sequence)
            .map_err(|_| graph_event_corrupt("sequence", scope_hash))?;
        let rows = sqlx::query(
            "SELECT id, fragment_id, sequence, recorded_at_us, payload_json
             FROM events
             WHERE scope_key = ?1 AND (?2 IS NULL OR sequence > ?2) AND sequence <= ?3
             ORDER BY sequence, id
             LIMIT ?4",
        )
        .bind(scope_hash)
        .bind(checkpoint_i64)
        .bind(target_i64)
        .bind(i64::from(batch_size))
        .fetch_all(&mut **transaction)
        .await?;
        if rows.is_empty() {
            return Err(graph_event_corrupt("sequence_gap", scope_hash));
        }

        let mut expected_sequence = checkpoint.map_or(0, |sequence| sequence.saturating_add(1));
        let mut next_sequence = checkpoint.unwrap_or(0);
        let mut updated_at = DateTime::<Utc>::UNIX_EPOCH;
        for row in rows {
            let event = decode_graph_projection_event(&row, scope, scope_hash)?;
            if event.sequence != expected_sequence {
                return Err(graph_event_corrupt("sequence_gap", scope_hash));
            }
            apply_graph_event(transaction, scope, scope_hash, &event).await?;
            next_sequence = event.sequence;
            expected_sequence = expected_sequence.saturating_add(1);
            updated_at = event.recorded_at;
        }
        self.failpoint
            .before_commit(ProjectorKind::Graph, next_sequence)?;
        sqlx::query(
            "INSERT INTO projection_checkpoints
                (scope_key, projector, sequence, updated_at_us)
             VALUES (?1, 'graph', ?2, ?3)
             ON CONFLICT(scope_key, projector) DO UPDATE SET
                sequence = excluded.sequence,
                updated_at_us = excluded.updated_at_us",
        )
        .bind(scope_hash)
        .bind(
            i64::try_from(next_sequence)
                .map_err(|_| checkpoint_corrupt(ProjectorKind::Graph, scope_hash))?,
        )
        .bind(updated_at.timestamp_micros())
        .execute(&mut **transaction)
        .await?;
        Ok(graph_checkpoint_value(next_sequence, updated_at))
    }

    async fn project_state_batch(
        &self,
        scope: &MemoryScope,
        target: ProjectionTarget,
        batch_size: u32,
    ) -> Result<ProjectionCheckpoint, StorageError> {
        let scope_hash = canonical_scope_key(scope)?;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let result = self
            .project_state_batch_inner(&mut transaction, scope, &scope_hash, target, batch_size)
            .await;
        match result {
            Ok(checkpoint) => {
                transaction.commit().await?;
                Ok(checkpoint)
            }
            Err(error) => {
                let _ = transaction.rollback().await;
                Err(error)
            }
        }
    }

    async fn project_state_batch_inner(
        &self,
        transaction: &mut Transaction<'_, Sqlite>,
        scope: &MemoryScope,
        scope_hash: &str,
        target: ProjectionTarget,
        batch_size: u32,
    ) -> Result<ProjectionCheckpoint, StorageError> {
        let checkpoint = checkpoint_in_transaction(transaction, scope_hash).await?;
        if checkpoint.is_some_and(|sequence| sequence >= target.sequence) {
            return Ok(checkpoint_value(
                checkpoint.unwrap_or(0),
                DateTime::<Utc>::UNIX_EPOCH,
            ));
        }
        let checkpoint_i64 = checkpoint
            .map(|sequence| {
                i64::try_from(sequence)
                    .map_err(|_| checkpoint_corrupt(ProjectorKind::State, scope_hash))
            })
            .transpose()?;
        let target_i64 =
            i64::try_from(target.sequence).map_err(|_| event_corrupt("sequence", scope_hash))?;
        let rows = sqlx::query(
            "SELECT id, fragment_id, sequence, recorded_at_us, payload_json
             FROM events
             WHERE scope_key = ?1 AND (?2 IS NULL OR sequence > ?2) AND sequence <= ?3
             ORDER BY sequence, id
             LIMIT ?4",
        )
        .bind(scope_hash)
        .bind(checkpoint_i64)
        .bind(target_i64)
        .bind(i64::from(batch_size))
        .fetch_all(&mut **transaction)
        .await?;
        if rows.is_empty() {
            return Err(event_corrupt("sequence_gap", scope_hash));
        }

        let mut expected_sequence = checkpoint.map_or(0, |sequence| sequence.saturating_add(1));
        let mut next_sequence = checkpoint.unwrap_or(0);
        let mut updated_at = DateTime::<Utc>::UNIX_EPOCH;
        for row in rows {
            let event = decode_projection_event(&row, scope, scope_hash)?;
            if event.sequence != expected_sequence {
                return Err(event_corrupt("sequence_gap", scope_hash));
            }
            apply_state_event(transaction, scope_hash, &event).await?;
            next_sequence = event.sequence;
            expected_sequence = expected_sequence.saturating_add(1);
            updated_at = event.recorded_at;
        }
        self.failpoint
            .before_commit(ProjectorKind::State, next_sequence)?;
        sqlx::query(
            "INSERT INTO projection_checkpoints
                (scope_key, projector, sequence, updated_at_us)
             VALUES (?1, 'state', ?2, ?3)
             ON CONFLICT(scope_key, projector) DO UPDATE SET
                sequence = excluded.sequence,
                updated_at_us = excluded.updated_at_us",
        )
        .bind(scope_hash)
        .bind(
            i64::try_from(next_sequence)
                .map_err(|_| checkpoint_corrupt(ProjectorKind::State, scope_hash))?,
        )
        .bind(updated_at.timestamp_micros())
        .execute(&mut **transaction)
        .await?;
        Ok(checkpoint_value(next_sequence, updated_at))
    }
}

fn canonical_scope_key(scope: &MemoryScope) -> Result<String, StorageError> {
    let value = serde_json::to_value(scope)?;
    let bytes = serde_json::to_vec(&canonicalize(value))?;
    Ok(hex::encode(Sha256::digest(bytes)))
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
