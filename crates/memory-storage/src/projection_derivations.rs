//! Consolidation 派生项到模型无关 Recall View 的安全读取。

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};
use jc_memory_protocol::{
    CognitiveFragment, EpisodeView, EventId, EvidenceView, FragmentId, FragmentKind, KnowledgeKind,
    KnowledgeView, MemoryScope, PointId, ProcedureView, SurfaceId, SurfaceStatus, SurfaceView,
    ValidityStatus,
};
use serde::Deserialize;
use serde_json::Value;
use sqlx::{QueryBuilder, Row as _, Sqlite, SqlitePool, sqlite::SqliteRow};

use crate::StorageError;

/// 与一次有界 Recall 相关的强类型 Consolidation 派生项。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProjectedRecallDerivations {
    /// 与激活 Point 相交的有效 Surface。
    pub surfaces: Vec<SurfaceView>,
    /// 相关或按完整 key 精确命中的程序经验。
    pub procedures: Vec<ProcedureView>,
    /// 达到独立 Verified Evidence 门槛的约束。
    pub constraints: Vec<KnowledgeView>,
    /// 达到独立 Verified Evidence 门槛的偏好。
    pub preferences: Vec<KnowledgeView>,
    /// 预留给 Core 从已激活权威 Episode 筛选的失败记录。
    pub past_failures: Vec<EpisodeView>,
    /// 所有返回派生 View 的去重权威来源。
    pub evidence: Vec<EvidenceView>,
    /// 是否因硬预算省略了相关派生项。
    pub truncated: bool,
}

#[derive(Debug)]
struct ArtifactRow {
    kind: String,
    subject_key: String,
    source_cursor: u64,
    payload: Value,
    evidence: Value,
}

#[derive(Debug, Deserialize)]
struct ProcedurePayload {
    sample_count: u32,
    success_count: u32,
    failure_count: u32,
    success_rate: f64,
}

#[derive(Debug, Deserialize)]
struct SemanticPayload {
    point_id: PointId,
    kind: KnowledgeKind,
    content: Value,
    confidence: f32,
    validity: ValidityStatus,
    stable: bool,
}

#[derive(Clone, Debug, Deserialize)]
struct ArtifactEvidence {
    scope: MemoryScope,
    event_id: EventId,
    fragment_id: FragmentId,
    source_id: String,
    #[serde(default)]
    succeeded: Option<bool>,
}

#[derive(Debug)]
struct ValidatedEvidence {
    view: EvidenceView,
    source_id: String,
    kind: FragmentKind,
    validity: ValidityStatus,
}

#[derive(Debug)]
struct SurfaceCandidate {
    view: SurfaceView,
    intersection_weight: f32,
}

#[derive(Debug)]
struct SurfaceAccumulator {
    surface_id: SurfaceId,
    version: u64,
    status: SurfaceStatus,
    confidence: f32,
    evidence: Value,
    member_points: BTreeMap<PointId, f32>,
    intersection_weight: f32,
}

pub(crate) struct DerivationQuery<'query> {
    pub(crate) scope: &'query MemoryScope,
    pub(crate) scope_hash: &'query str,
    pub(crate) activated_point_ids: &'query [PointId],
    pub(crate) activated_fragment_ids: &'query [FragmentId],
    pub(crate) procedure_key: Option<&'query str>,
    pub(crate) effective_at: DateTime<Utc>,
    pub(crate) max_items: usize,
}

pub(crate) async fn load(
    pool: &SqlitePool,
    query: DerivationQuery<'_>,
) -> Result<ProjectedRecallDerivations, StorageError> {
    let DerivationQuery {
        scope,
        scope_hash,
        activated_point_ids,
        activated_fragment_ids,
        procedure_key,
        effective_at,
        max_items,
    } = query;
    validate_protocol_ids(pool, scope_hash).await?;
    let rows = load_artifacts(pool, scope_hash).await?;
    let max_sequence: i64 =
        sqlx::query_scalar("SELECT COALESCE(MAX(sequence), -1) FROM events WHERE scope_key = ?1")
            .bind(scope_hash)
            .fetch_one(pool)
            .await?;
    let active_points: BTreeSet<_> = activated_point_ids.iter().copied().collect();
    let active_fragments: BTreeSet<_> = activated_fragment_ids.iter().copied().collect();
    let mut result = ProjectedRecallDerivations::default();
    let mut remaining = max_items;
    let surfaces = load_surfaces(pool, scope, scope_hash, &active_points, effective_at).await?;
    for surface in surfaces {
        let cost = 1_usize.saturating_add(surface.view.member_points.len());
        if cost > remaining {
            result.truncated = true;
            break;
        }
        remaining -= cost;
        result
            .evidence
            .extend(surface.view.evidence.iter().cloned());
        result.surfaces.push(surface.view);
    }
    for row in rows.iter().filter(|row| row.kind == "procedure_experience") {
        validate_cursor(row, max_sequence, scope_hash)?;
        let evidence = decode_evidence(row, scope_hash)?;
        let validated = validate_evidence(pool, scope, scope_hash, &evidence).await?;
        let relevant = procedure_key == Some(row.subject_key.as_str())
            || validated
                .iter()
                .any(|item| active_fragments.contains(&item.view.fragment_id));
        let procedure = decode_procedure(row, &evidence, scope_hash)?;
        if !relevant {
            continue;
        }
        if remaining == 0 {
            result.truncated = true;
            continue;
        }
        remaining -= 1;
        result.evidence.extend(evidence_views(&validated));
        result.procedures.push(procedure);
    }
    for row in rows.iter().filter(|row| row.kind == "semantic_promotion") {
        validate_cursor(row, max_sequence, scope_hash)?;
        let evidence = decode_evidence(row, scope_hash)?;
        let validated = validate_evidence(pool, scope, scope_hash, &evidence).await?;
        let knowledge = decode_knowledge(pool, row, &validated, scope_hash).await?;
        if !active_points.contains(&knowledge.point_id) {
            continue;
        }
        if remaining == 0 {
            result.truncated = true;
            continue;
        }
        remaining -= 1;
        result.evidence.extend(knowledge.evidence.iter().cloned());
        match knowledge.kind {
            KnowledgeKind::Constraint => result.constraints.push(knowledge),
            KnowledgeKind::Preference => result.preferences.push(knowledge),
            KnowledgeKind::Fact => {}
        }
    }
    result.procedures.sort_by(|left, right| {
        right
            .sample_count
            .cmp(&left.sample_count)
            .then_with(|| left.procedure_key.cmp(&right.procedure_key))
    });
    result.constraints.sort_by_key(|item| item.point_id);
    result.preferences.sort_by_key(|item| item.point_id);
    result
        .evidence
        .sort_by_key(|item| (item.fragment_id, item.event_id, item.evidence_ref_id));
    result
        .evidence
        .dedup_by_key(|item| (item.fragment_id, item.event_id, item.evidence_ref_id));
    Ok(result)
}

async fn validate_protocol_ids(pool: &SqlitePool, scope_hash: &str) -> Result<(), StorageError> {
    let point_ids: Vec<String> =
        sqlx::query_scalar("SELECT id FROM points WHERE scope_key = ?1 ORDER BY id")
            .bind(scope_hash)
            .fetch_all(pool)
            .await?;
    if point_ids.iter().any(|id| id.parse::<PointId>().is_err()) {
        return Err(corrupt("point_id", scope_hash));
    }
    let surface_ids: Vec<String> =
        sqlx::query_scalar("SELECT id FROM surfaces WHERE scope_key = ?1 ORDER BY id")
            .bind(scope_hash)
            .fetch_all(pool)
            .await?;
    if surface_ids
        .iter()
        .any(|id| id.parse::<SurfaceId>().is_err())
    {
        return Err(corrupt("surface_id", scope_hash));
    }
    Ok(())
}

async fn load_surfaces(
    pool: &SqlitePool,
    scope: &MemoryScope,
    scope_hash: &str,
    active_points: &BTreeSet<PointId>,
    effective_at: DateTime<Utc>,
) -> Result<Vec<SurfaceCandidate>, StorageError> {
    if active_points.is_empty() {
        return Ok(Vec::new());
    }
    let rows = query_surface_rows(pool, scope_hash, active_points, effective_at).await?;
    let mut grouped = BTreeMap::<(SurfaceId, u64), SurfaceAccumulator>::new();
    for row in rows {
        let raw_surface_id: String = row.try_get("id")?;
        let surface_id = raw_surface_id
            .parse::<SurfaceId>()
            .map_err(|_| corrupt("surface_id", scope_hash))?;
        let raw_version: i64 = row.try_get("version")?;
        let version =
            u64::try_from(raw_version).map_err(|_| corrupt("surface_version", scope_hash))?;
        let raw_point_id: String = row.try_get("point_id")?;
        let point_id = raw_point_id
            .parse::<PointId>()
            .map_err(|_| corrupt("surface_member_id", scope_hash))?;
        let point_scope_key: Option<String> = row.try_get("point_scope_key")?;
        let point_scope_key =
            point_scope_key.ok_or_else(|| corrupt("surface_member_point", scope_hash))?;
        if point_scope_key != scope_hash {
            return Err(corrupt("surface_member_scope", scope_hash));
        }
        let weight: f32 = row.try_get("member_weight")?;
        let confidence: f32 = row.try_get("confidence")?;
        if !weight.is_finite()
            || !(0.0..=1.0).contains(&weight)
            || !confidence.is_finite()
            || !(0.0..=1.0).contains(&confidence)
        {
            return Err(corrupt("surface_score", scope_hash));
        }
        let raw_status: String = row.try_get("status")?;
        let raw_evidence: String = row.try_get("source_evidence_json")?;
        let key = (surface_id, version);
        let accumulator = grouped.entry(key).or_insert(SurfaceAccumulator {
            surface_id,
            version,
            status: parse_surface_status(&raw_status, scope_hash)?,
            confidence,
            evidence: parse_json(&raw_evidence, "surface_evidence", scope_hash)?,
            member_points: BTreeMap::new(),
            intersection_weight: 0.0,
        });
        accumulator.member_points.insert(point_id, weight);
        if active_points.contains(&point_id) {
            accumulator.intersection_weight += weight;
        }
    }
    let mut surfaces = Vec::with_capacity(grouped.len());
    for (_, accumulated) in grouped {
        let evidence = decode_nested_evidence(&accumulated.evidence, scope_hash)?;
        let validated = validate_evidence(pool, scope, scope_hash, &evidence).await?;
        surfaces.push(SurfaceCandidate {
            view: SurfaceView {
                surface_id: accumulated.surface_id,
                version: accumulated.version,
                status: accumulated.status,
                member_points: accumulated.member_points,
                confidence: accumulated.confidence,
                evidence: evidence_views(&validated),
            },
            intersection_weight: accumulated.intersection_weight,
        });
    }
    surfaces.sort_by(|left, right| {
        right
            .intersection_weight
            .total_cmp(&left.intersection_weight)
            .then_with(|| right.view.confidence.total_cmp(&left.view.confidence))
            .then_with(|| left.view.surface_id.cmp(&right.view.surface_id))
    });
    Ok(surfaces)
}

async fn query_surface_rows(
    pool: &SqlitePool,
    scope_hash: &str,
    active_points: &BTreeSet<PointId>,
    effective_at: DateTime<Utc>,
) -> Result<Vec<SqliteRow>, StorageError> {
    let effective_at_us = effective_at.timestamp_micros();
    let mut query = QueryBuilder::<Sqlite>::new(
        "SELECT s.id, s.version, s.status, s.confidence, s.source_evidence_json,
                sm.point_id, sm.member_weight, p.scope_key AS point_scope_key
         FROM surfaces s
         JOIN surface_members sm ON sm.surface_id = s.id AND sm.version = s.version
         LEFT JOIN points p ON p.id = sm.point_id
         WHERE s.scope_key = ",
    );
    query.push_bind(scope_hash);
    query.push(" AND s.status IN ('active', 'frozen') AND s.valid_from_us <= ");
    query.push_bind(effective_at_us);
    query.push(" AND (s.valid_to_us IS NULL OR ");
    query.push_bind(effective_at_us);
    query.push(" < s.valid_to_us) AND s.version = (SELECT MAX(v.version) FROM surfaces v WHERE v.id = s.id AND v.scope_key = s.scope_key AND v.valid_from_us <= ");
    query.push_bind(effective_at_us);
    query.push(" AND (v.valid_to_us IS NULL OR ");
    query.push_bind(effective_at_us);
    query.push(" < v.valid_to_us)) AND EXISTS (SELECT 1 FROM surface_members relevant WHERE relevant.surface_id = s.id AND relevant.version = s.version AND relevant.point_id IN (");
    {
        let mut separated = query.separated(", ");
        for point_id in active_points {
            separated.push_bind(point_id.to_string());
        }
    }
    query.push(") ) ORDER BY s.id, s.version, sm.point_id");
    Ok(query.build().fetch_all(pool).await?)
}

fn parse_surface_status(raw: &str, scope_hash: &str) -> Result<SurfaceStatus, StorageError> {
    match raw {
        "active" => Ok(SurfaceStatus::Active),
        "frozen" => Ok(SurfaceStatus::Frozen),
        "inactive" => Ok(SurfaceStatus::Inactive),
        "merged" => Ok(SurfaceStatus::Merged),
        "split" => Ok(SurfaceStatus::Split),
        _ => Err(corrupt("surface_status", scope_hash)),
    }
}

fn decode_nested_evidence(
    value: &Value,
    scope_hash: &str,
) -> Result<Vec<ArtifactEvidence>, StorageError> {
    fn collect(value: &Value, output: &mut Vec<ArtifactEvidence>) -> Result<(), ()> {
        match value {
            Value::Array(items) => {
                for item in items {
                    collect(item, output)?;
                }
            }
            Value::Object(object) => {
                if ["event_id", "fragment_id", "scope", "source_id"]
                    .iter()
                    .any(|field| object.contains_key(*field))
                {
                    let evidence = serde_json::from_value::<ArtifactEvidence>(value.clone())
                        .map_err(|_| ())?;
                    output.push(evidence);
                } else {
                    for nested in object.values() {
                        collect(nested, output)?;
                    }
                }
            }
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
        }
        Ok(())
    }
    let mut evidence = Vec::new();
    collect(value, &mut evidence).map_err(|()| corrupt("surface_evidence", scope_hash))?;
    if evidence.is_empty() {
        return Err(corrupt("surface_evidence", scope_hash));
    }
    Ok(evidence)
}

async fn load_artifacts(
    pool: &SqlitePool,
    scope_hash: &str,
) -> Result<Vec<ArtifactRow>, StorageError> {
    let rows = sqlx::query(
        "SELECT id, kind, subject_key, source_cursor, payload_json, evidence_json
         FROM consolidation_artifacts
         WHERE scope_key = ?1 AND kind IN ('procedure_experience', 'semantic_promotion')
         ORDER BY kind, subject_key, id",
    )
    .bind(scope_hash)
    .fetch_all(pool)
    .await?;
    rows.into_iter()
        .map(|row| {
            let raw_cursor: i64 = row.try_get("source_cursor")?;
            let payload_json: String = row.try_get("payload_json")?;
            let evidence_json: String = row.try_get("evidence_json")?;
            Ok(ArtifactRow {
                kind: row.try_get("kind")?,
                subject_key: row.try_get("subject_key")?,
                source_cursor: u64::try_from(raw_cursor)
                    .map_err(|_| corrupt("source_cursor", scope_hash))?,
                payload: parse_json(&payload_json, "payload_json", scope_hash)?,
                evidence: parse_json(&evidence_json, "evidence_json", scope_hash)?,
            })
        })
        .collect()
}

fn parse_json(raw: &str, field: &'static str, scope_hash: &str) -> Result<Value, StorageError> {
    serde_json::from_str(raw).map_err(|_| corrupt(field, scope_hash))
}

fn validate_cursor(
    row: &ArtifactRow,
    max_sequence: i64,
    scope_hash: &str,
) -> Result<(), StorageError> {
    if max_sequence < 0 || row.source_cursor > u64::try_from(max_sequence).unwrap_or_default() {
        return Err(corrupt("source_cursor", scope_hash));
    }
    Ok(())
}

fn decode_evidence(
    row: &ArtifactRow,
    scope_hash: &str,
) -> Result<Vec<ArtifactEvidence>, StorageError> {
    let evidence: Vec<ArtifactEvidence> = serde_json::from_value(row.evidence.clone())
        .map_err(|_| corrupt("evidence_json", scope_hash))?;
    if evidence.is_empty() {
        return Err(corrupt("evidence_json", scope_hash));
    }
    Ok(evidence)
}

async fn validate_evidence(
    pool: &SqlitePool,
    scope: &MemoryScope,
    scope_hash: &str,
    evidence: &[ArtifactEvidence],
) -> Result<Vec<ValidatedEvidence>, StorageError> {
    let mut event_ids = BTreeSet::new();
    let mut validated = Vec::with_capacity(evidence.len());
    for item in evidence {
        if &item.scope != scope || !event_ids.insert(item.event_id) {
            return Err(corrupt("evidence_scope_or_identity", scope_hash));
        }
        let row = sqlx::query(
            "SELECT fragment_id, payload_json FROM events
             WHERE id = ?1 AND scope_key = ?2",
        )
        .bind(item.event_id.to_string())
        .bind(scope_hash)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| corrupt("evidence_event", scope_hash))?;
        let stored_fragment_id: String = row.try_get("fragment_id")?;
        if stored_fragment_id != item.fragment_id.to_string() {
            return Err(corrupt("evidence_fragment", scope_hash));
        }
        let payload: CognitiveFragment =
            serde_json::from_str(&row.try_get::<String, _>("payload_json")?)
                .map_err(|_| corrupt("evidence_payload", scope_hash))?;
        if payload.id != item.fragment_id
            || payload.scope != *scope
            || payload.source.source_id != item.source_id
        {
            return Err(corrupt("evidence_payload", scope_hash));
        }
        validated.push(ValidatedEvidence {
            view: EvidenceView {
                fragment_id: item.fragment_id,
                event_id: Some(item.event_id),
                evidence_ref_id: None,
            },
            source_id: item.source_id.clone(),
            kind: payload.kind,
            validity: payload.validity,
        });
    }
    Ok(validated)
}

fn decode_procedure(
    row: &ArtifactRow,
    evidence: &[ArtifactEvidence],
    scope_hash: &str,
) -> Result<ProcedureView, StorageError> {
    let payload: ProcedurePayload = serde_json::from_value(row.payload.clone())
        .map_err(|_| corrupt("procedure_payload", scope_hash))?;
    let sample_count =
        u32::try_from(evidence.len()).map_err(|_| corrupt("procedure_counts", scope_hash))?;
    let success_count = u32::try_from(
        evidence
            .iter()
            .filter(|item| item.succeeded == Some(true))
            .count(),
    )
    .map_err(|_| corrupt("procedure_counts", scope_hash))?;
    if evidence.iter().any(|item| item.succeeded.is_none()) {
        return Err(corrupt("procedure_evidence", scope_hash));
    }
    let failure_count = sample_count.saturating_sub(success_count);
    let success_rate = f64::from(success_count) / f64::from(sample_count);
    if payload.sample_count != sample_count
        || payload.success_count != success_count
        || payload.failure_count != failure_count
        || !payload.success_rate.is_finite()
        || (payload.success_rate - success_rate).abs() > f64::EPSILON
    {
        return Err(corrupt("procedure_counts", scope_hash));
    }
    Ok(ProcedureView {
        procedure_key: row.subject_key.clone(),
        sample_count,
        success_rate,
        evidence: evidence
            .iter()
            .map(|item| EvidenceView {
                fragment_id: item.fragment_id,
                event_id: Some(item.event_id),
                evidence_ref_id: None,
            })
            .collect(),
    })
}

async fn decode_knowledge(
    pool: &SqlitePool,
    row: &ArtifactRow,
    evidence: &[ValidatedEvidence],
    scope_hash: &str,
) -> Result<KnowledgeView, StorageError> {
    let payload: SemanticPayload = serde_json::from_value(row.payload.clone())
        .map_err(|_| corrupt("semantic_payload", scope_hash))?;
    if !payload.stable
        || payload.validity != ValidityStatus::Verified
        || !payload.confidence.is_finite()
        || !(0.0..=1.0).contains(&payload.confidence)
    {
        return Err(corrupt("semantic_validity", scope_hash));
    }
    let source_fragment: Option<String> = sqlx::query_scalar(
        "SELECT source_fragment_id FROM points WHERE id = ?1 AND scope_key = ?2",
    )
    .bind(payload.point_id.to_string())
    .bind(scope_hash)
    .fetch_optional(pool)
    .await?;
    let Some(source_fragment) = source_fragment else {
        return Err(corrupt("semantic_point_id", scope_hash));
    };
    if !evidence
        .iter()
        .any(|item| item.view.fragment_id.to_string() == source_fragment)
    {
        return Err(corrupt("semantic_point_source", scope_hash));
    }
    let event_ids: BTreeSet<_> = evidence
        .iter()
        .filter(|item| {
            item.validity == ValidityStatus::Verified
                && !matches!(
                    item.kind,
                    FragmentKind::Prediction | FragmentKind::Inference | FragmentKind::Plan
                )
        })
        .filter_map(|item| item.view.event_id)
        .collect();
    let sources: BTreeSet<_> = evidence
        .iter()
        .map(|item| item.source_id.as_str())
        .collect();
    if event_ids.len() != evidence.len() || event_ids.len() < 2 || sources.len() < 2 {
        return Err(corrupt("semantic_independent_evidence", scope_hash));
    }
    Ok(KnowledgeView {
        point_id: payload.point_id,
        kind: payload.kind,
        value: payload.content,
        confidence: payload.confidence,
        validity: payload.validity,
        evidence: evidence_views(evidence),
    })
}

fn evidence_views(evidence: &[ValidatedEvidence]) -> Vec<EvidenceView> {
    evidence.iter().map(|item| item.view.clone()).collect()
}

fn corrupt(field: &'static str, scope_hash: &str) -> StorageError {
    StorageError::ProjectionDerivationCorrupt {
        field,
        scope_hash: scope_hash.to_owned(),
    }
}
