//! Consolidation 派生项的精确 Scope 读取契约。

use chrono::{DateTime, Utc};
use jc_memory_events::factory::EventFactory;
use jc_memory_protocol::{
    CognitiveFragment, EventDraft, EvidenceId, EvidenceRef, FragmentContent, FragmentId,
    FragmentKind, FragmentSource, KnowledgeKind, MemoryScope, NamespaceId, Perspective, PointId,
    SecurityLabel, SourceType, SurfaceId, SurfaceStatus, ValidityStatus, VerificationMethod,
};
use jc_memory_storage::{
    MemoryRepository, SqliteConfig, SqliteMemoryRepository, SqliteProjectionStore, SqliteStore,
    StorageError,
};

#[tokio::test]
async fn procedure_and_knowledge_are_loaded_from_authoritative_evidence() {
    let harness = Harness::new().await;
    let support_a = harness
        .append(FragmentKind::Fact, "support-a", "证据 A", false)
        .await;
    let support_b = harness
        .append(FragmentKind::Fact, "support-b", "证据 B", false)
        .await;
    let constraint_a = harness
        .append_verified(
            FragmentKind::Constraint,
            "constraint-a",
            "部署前必须通过回归测试",
            support_a.fragment_id,
        )
        .await;
    let constraint_b = harness
        .append_verified(
            FragmentKind::Constraint,
            "constraint-b",
            "部署前必须通过回归测试",
            support_b.fragment_id,
        )
        .await;
    let preference_a = harness
        .append_verified(
            FragmentKind::Preference,
            "preference-a",
            "优先使用小批量发布",
            support_a.fragment_id,
        )
        .await;
    let preference_b = harness
        .append_verified(
            FragmentKind::Preference,
            "preference-b",
            "优先使用小批量发布",
            support_b.fragment_id,
        )
        .await;
    let succeeded = harness
        .append(FragmentKind::Success, "procedure-a", "发布成功", false)
        .await;
    let failed = harness
        .append(FragmentKind::Failure, "procedure-b", "发布失败", false)
        .await;
    harness
        .insert_point(constraint_a.fragment_id, FragmentKind::Constraint)
        .await;
    harness
        .insert_point(preference_a.fragment_id, FragmentKind::Preference)
        .await;
    harness
        .insert_knowledge_artifact(
            "constraint-artifact",
            "constraint",
            constraint_a.fragment_id,
            &[&constraint_a, &constraint_b],
            "部署前必须通过回归测试",
        )
        .await;
    harness
        .insert_knowledge_artifact(
            "preference-artifact",
            "preference",
            preference_a.fragment_id,
            &[&preference_a, &preference_b],
            "优先使用小批量发布",
        )
        .await;
    harness.insert_procedure_artifact(&succeeded, &failed).await;

    let projected = harness
        .projection
        .load_recall_derivations(
            &harness.scope,
            &[
                point_id(constraint_a.fragment_id),
                point_id(preference_a.fragment_id),
            ],
            &[succeeded.fragment_id, failed.fragment_id],
            Some("deploy.guard"),
            Utc::now(),
            8,
        )
        .await
        .expect("读取可追溯经验派生项");

    assert_eq!(projected.procedures.len(), 1);
    assert_eq!(projected.procedures[0].procedure_key, "deploy.guard");
    assert_eq!(projected.procedures[0].sample_count, 2);
    assert!((projected.procedures[0].success_rate - 0.5).abs() < f64::EPSILON);
    assert_eq!(projected.constraints.len(), 1);
    assert_eq!(projected.constraints[0].kind, KnowledgeKind::Constraint);
    assert_eq!(projected.preferences.len(), 1);
    assert_eq!(projected.preferences[0].kind, KnowledgeKind::Preference);
    assert_eq!(projected.evidence.len(), 6);
    assert!(!projected.truncated);
}

#[tokio::test]
async fn inconsistent_procedure_counts_fail_closed() {
    let harness = Harness::new().await;
    let succeeded = harness
        .append(FragmentKind::Success, "procedure-a", "发布成功", false)
        .await;
    let failed = harness
        .append(FragmentKind::Failure, "procedure-b", "发布失败", false)
        .await;
    harness.insert_procedure_artifact(&succeeded, &failed).await;
    sqlx::query(
        "UPDATE consolidation_artifacts
         SET payload_json = json_set(payload_json, '$.success_count', 99)
         WHERE kind = 'procedure_experience'",
    )
    .execute(harness.store.pool())
    .await
    .expect("破坏 Procedure 计数");

    assert!(matches!(
        harness
            .projection
            .load_recall_derivations(
                &harness.scope,
                &[],
                &[succeeded.fragment_id, failed.fragment_id],
                Some("deploy.guard"),
                Utc::now(),
                8,
            )
            .await,
        Err(StorageError::ProjectionDerivationCorrupt {
            field: "procedure_counts",
            ..
        })
    ));
}

#[tokio::test]
async fn artifact_declaring_another_scope_fails_closed() {
    let harness = Harness::new().await;
    let succeeded = harness
        .append(FragmentKind::Success, "procedure-a", "发布成功", false)
        .await;
    let failed = harness
        .append(FragmentKind::Failure, "procedure-b", "发布失败", false)
        .await;
    harness.insert_procedure_artifact(&succeeded, &failed).await;
    sqlx::query(
        "UPDATE consolidation_artifacts
         SET evidence_json = json_set(evidence_json, '$[0].scope.namespace_id', ?1)
         WHERE kind = 'procedure_experience'",
    )
    .bind(NamespaceId::new().to_string())
    .execute(harness.store.pool())
    .await
    .expect("把 Artifact Evidence 改为外部 Scope");

    assert!(matches!(
        harness
            .projection
            .load_recall_derivations(
                &harness.scope,
                &[],
                &[succeeded.fragment_id, failed.fragment_id],
                Some("deploy.guard"),
                Utc::now(),
                8,
            )
            .await,
        Err(StorageError::ProjectionDerivationCorrupt {
            field: "evidence_scope_or_identity",
            ..
        })
    ));
}

#[tokio::test]
async fn surfaces_require_active_members_effective_time_and_complete_budget() {
    let harness = Harness::new().await;
    let source = harness
        .append(FragmentKind::Fact, "surface-source", "发布上下文", false)
        .await;
    harness
        .insert_point(source.fragment_id, FragmentKind::Fact)
        .await;
    let now = Utc::now();
    harness
        .insert_surface(SurfaceStatus::Active, &source, now, None)
        .await;
    harness
        .insert_surface(SurfaceStatus::Frozen, &source, now, None)
        .await;
    harness
        .insert_surface(SurfaceStatus::Inactive, &source, now, None)
        .await;
    harness
        .insert_surface(SurfaceStatus::Merged, &source, now, None)
        .await;
    harness
        .insert_surface(SurfaceStatus::Split, &source, now, None)
        .await;
    harness
        .insert_surface(
            SurfaceStatus::Active,
            &source,
            now + chrono::Duration::minutes(5),
            None,
        )
        .await;

    let activated_point = point_id(source.fragment_id);
    let projected = harness
        .projection
        .load_recall_derivations(
            &harness.scope,
            &[activated_point],
            &[source.fragment_id],
            None,
            now,
            8,
        )
        .await
        .expect("读取相关 Surface");
    assert_eq!(projected.surfaces.len(), 2);
    assert!(projected.surfaces.iter().all(|surface| {
        matches!(
            surface.status,
            SurfaceStatus::Active | SurfaceStatus::Frozen
        )
    }));
    assert!(
        projected
            .surfaces
            .iter()
            .all(|surface| surface.member_points.contains_key(&activated_point))
    );
    let limited = harness
        .projection
        .load_recall_derivations(
            &harness.scope,
            &[activated_point],
            &[source.fragment_id],
            None,
            now,
            1,
        )
        .await
        .expect("按完整 Surface 扣减预算");
    assert!(limited.surfaces.is_empty());
    assert!(limited.truncated);
}

#[tokio::test]
async fn surface_query_returns_the_latest_effective_version() {
    let harness = Harness::new().await;
    let source = harness
        .append(FragmentKind::Fact, "surface-version", "版本化上下文", false)
        .await;
    harness
        .insert_point(source.fragment_id, FragmentKind::Fact)
        .await;
    let now = Utc::now();
    let current_version_id = SurfaceId::new();
    harness
        .insert_surface_version(SurfaceFixture {
            surface_id: current_version_id,
            version: 1,
            status: SurfaceStatus::Active,
            source: &source,
            valid_from: now - chrono::Duration::minutes(2),
            valid_to: None,
            members: &[source.fragment_id],
        })
        .await;
    harness
        .insert_surface_version(SurfaceFixture {
            surface_id: current_version_id,
            version: 2,
            status: SurfaceStatus::Frozen,
            source: &source,
            valid_from: now - chrono::Duration::minutes(1),
            valid_to: None,
            members: &[source.fragment_id],
        })
        .await;
    let future_version_id = SurfaceId::new();
    harness
        .insert_surface_version(SurfaceFixture {
            surface_id: future_version_id,
            version: 1,
            status: SurfaceStatus::Active,
            source: &source,
            valid_from: now - chrono::Duration::minutes(1),
            valid_to: None,
            members: &[source.fragment_id],
        })
        .await;
    harness
        .insert_surface_version(SurfaceFixture {
            surface_id: future_version_id,
            version: 2,
            status: SurfaceStatus::Frozen,
            source: &source,
            valid_from: now + chrono::Duration::minutes(5),
            valid_to: None,
            members: &[source.fragment_id],
        })
        .await;

    let projected = harness
        .projection
        .load_recall_derivations(
            &harness.scope,
            &[point_id(source.fragment_id)],
            &[source.fragment_id],
            None,
            now,
            4,
        )
        .await
        .expect("读取查询时刻的最新 Surface 版本");

    assert_eq!(projected.surfaces.len(), 2);
    assert!(projected.surfaces.iter().any(|surface| {
        surface.surface_id == current_version_id
            && surface.version == 2
            && surface.status == SurfaceStatus::Frozen
    }));
    assert!(projected.surfaces.iter().any(|surface| {
        surface.surface_id == future_version_id
            && surface.version == 1
            && surface.status == SurfaceStatus::Active
    }));
}

#[tokio::test]
async fn surface_budget_stops_at_the_first_incomplete_candidate() {
    let harness = Harness::new().await;
    let source = harness
        .append(
            FragmentKind::Fact,
            "surface-budget-a",
            "发布上下文 A",
            false,
        )
        .await;
    let related = harness
        .append(
            FragmentKind::Fact,
            "surface-budget-b",
            "发布上下文 B",
            false,
        )
        .await;
    harness
        .insert_point(source.fragment_id, FragmentKind::Fact)
        .await;
    harness
        .insert_point(related.fragment_id, FragmentKind::Fact)
        .await;
    let now = Utc::now();
    harness
        .insert_surface_with_members(
            SurfaceStatus::Active,
            &source,
            now,
            &[source.fragment_id, related.fragment_id],
        )
        .await;
    harness
        .insert_surface(SurfaceStatus::Active, &source, now, None)
        .await;

    let limited = harness
        .projection
        .load_recall_derivations(
            &harness.scope,
            &[point_id(source.fragment_id), point_id(related.fragment_id)],
            &[source.fragment_id, related.fragment_id],
            None,
            now,
            2,
        )
        .await
        .expect("遇到无法完整容纳的优先 Surface 后停止");

    assert!(limited.surfaces.is_empty());
    assert!(limited.truncated);
}

#[tokio::test]
async fn malformed_surface_evidence_fails_closed() {
    let harness = Harness::new().await;
    let source = harness
        .append(FragmentKind::Fact, "surface-evidence", "证据上下文", false)
        .await;
    harness
        .insert_point(source.fragment_id, FragmentKind::Fact)
        .await;
    let now = Utc::now();
    harness
        .insert_surface(SurfaceStatus::Active, &source, now, None)
        .await;
    let damaged = serde_json::json!([
        {
            "scope": harness.scope,
            "event_id": source.id,
            "fragment_id": source.fragment_id,
            "source_id": source.payload.source.source_id,
        },
        {
            "scope": harness.scope,
            "event_id": "不是规范事件标识",
            "fragment_id": source.fragment_id,
            "source_id": source.payload.source.source_id,
        },
    ]);
    sqlx::query("UPDATE surfaces SET source_evidence_json = ?1")
        .bind(damaged.to_string())
        .execute(harness.store.pool())
        .await
        .expect("破坏 Surface Evidence");

    assert!(matches!(
        harness
            .projection
            .load_recall_derivations(
                &harness.scope,
                &[point_id(source.fragment_id)],
                &[source.fragment_id],
                None,
                now,
                2,
            )
            .await,
        Err(StorageError::ProjectionDerivationCorrupt {
            field: "surface_evidence",
            ..
        })
    ));
}

struct Harness {
    store: SqliteStore,
    repository: SqliteMemoryRepository,
    projection: SqliteProjectionStore,
    scope: MemoryScope,
}

struct SurfaceFixture<'fixture> {
    surface_id: SurfaceId,
    version: u64,
    status: SurfaceStatus,
    source: &'fixture EventDraft,
    valid_from: DateTime<Utc>,
    valid_to: Option<DateTime<Utc>>,
    members: &'fixture [FragmentId],
}

impl Harness {
    async fn new() -> Self {
        let directory = tempfile::tempdir().expect("创建派生读取测试目录");
        let path = directory.keep();
        let store = SqliteStore::open(&SqliteConfig::new(path.join("memory.sqlite3")))
            .await
            .expect("打开派生读取测试存储");
        Self {
            repository: SqliteMemoryRepository::from_store(&store),
            projection: SqliteProjectionStore::from_store(&store),
            store,
            scope: MemoryScope::new(NamespaceId::new()),
        }
    }

    async fn append(
        &self,
        kind: FragmentKind,
        source_id: &str,
        text: &str,
        verified: bool,
    ) -> EventDraft {
        let draft = EventFactory::new()
            .from_fragment(fragment(
                self.scope.clone(),
                kind,
                source_id,
                text,
                verified,
                Vec::new(),
            ))
            .expect("创建派生读取 Event");
        self.repository
            .append(&draft)
            .await
            .expect("写入权威 Event");
        draft
    }

    async fn append_verified(
        &self,
        kind: FragmentKind,
        source_id: &str,
        text: &str,
        support: FragmentId,
    ) -> EventDraft {
        let evidence = vec![EvidenceRef {
            id: EvidenceId::new(),
            fragment_id: Some(support),
            event_id: None,
            method: VerificationMethod::Tool,
            note: Some("派生读取测试证据".into()),
        }];
        let draft = EventFactory::new()
            .from_fragment(fragment(
                self.scope.clone(),
                kind,
                source_id,
                text,
                true,
                evidence,
            ))
            .expect("创建 Verified Event");
        self.repository
            .append(&draft)
            .await
            .expect("写入 Verified Event");
        draft
    }

    async fn scope_key(&self, fragment_id: FragmentId) -> String {
        sqlx::query_scalar("SELECT scope_key FROM fragments WHERE id = ?1")
            .bind(fragment_id.to_string())
            .fetch_one(self.store.pool())
            .await
            .expect("读取 Scope key")
    }

    async fn insert_point(&self, fragment_id: FragmentId, kind: FragmentKind) {
        let scope_key = self.scope_key(fragment_id).await;
        sqlx::query(
            "INSERT INTO points (
                id, scope_key, kind, source_fragment_id, valid_from_us, confidence, created_at_us
             ) VALUES (?1, ?2, ?3, ?1, 1, 0.9, 1)",
        )
        .bind(fragment_id.to_string())
        .bind(scope_key)
        .bind(kind_name(kind))
        .execute(self.store.pool())
        .await
        .expect("写入派生 Point");
    }

    async fn insert_knowledge_artifact(
        &self,
        id: &str,
        kind: &str,
        point_fragment_id: FragmentId,
        evidence: &[&EventDraft],
        text: &str,
    ) {
        let scope_key = self.scope_key(point_fragment_id).await;
        let evidence = evidence
            .iter()
            .enumerate()
            .map(|(sequence, event)| {
                serde_json::json!({
                    "scope": self.scope,
                    "event_id": event.id,
                    "fragment_id": event.fragment_id,
                    "source_id": event.payload.source.source_id,
                    "sequence": sequence,
                })
            })
            .collect::<Vec<_>>();
        sqlx::query(
            "INSERT INTO consolidation_artifacts (
                id, scope_key, kind, subject_key, source_cursor,
                payload_json, evidence_json, created_at_us
             ) VALUES (?1, ?2, 'semantic_promotion', ?3, 0, ?4, ?5, 1)",
        )
        .bind(id)
        .bind(scope_key)
        .bind(format!("{kind}-subject"))
        .bind(
            serde_json::json!({
                "point_id": point_fragment_id,
                "kind": kind,
                "content": {"type": "text", "text": text},
                "confidence": 0.9,
                "validity": "verified",
                "stable": true,
            })
            .to_string(),
        )
        .bind(serde_json::Value::Array(evidence).to_string())
        .execute(self.store.pool())
        .await
        .expect("写入 Knowledge Artifact");
    }

    async fn insert_procedure_artifact(&self, succeeded: &EventDraft, failed: &EventDraft) {
        let scope_key = self.scope_key(succeeded.fragment_id).await;
        let evidence = serde_json::json!([
            procedure_sample(&self.scope, succeeded, true),
            procedure_sample(&self.scope, failed, false),
        ]);
        sqlx::query(
            "INSERT INTO consolidation_artifacts (
                id, scope_key, kind, subject_key, source_cursor,
                payload_json, evidence_json, created_at_us
             ) VALUES ('procedure-artifact', ?1, 'procedure_experience', 'deploy.guard', 0, ?2, ?3, 1)",
        )
        .bind(scope_key)
        .bind(
            serde_json::json!({
                "sample_count": 2,
                "success_count": 1,
                "failure_count": 1,
                "success_rate": 0.5,
                "confidence_interval": {"lower": 0.1, "upper": 0.9},
                "statement": "当前 2 个样本中成功 1 次，仍需结合新证据判断。",
            })
            .to_string(),
        )
        .bind(evidence.to_string())
        .execute(self.store.pool())
        .await
        .expect("写入 Procedure Artifact");
    }

    async fn insert_surface(
        &self,
        status: SurfaceStatus,
        source: &EventDraft,
        valid_from: DateTime<Utc>,
        valid_to: Option<DateTime<Utc>>,
    ) {
        self.insert_surface_version(SurfaceFixture {
            surface_id: SurfaceId::new(),
            version: 1,
            status,
            source,
            valid_from,
            valid_to,
            members: &[source.fragment_id],
        })
        .await;
    }

    async fn insert_surface_with_members(
        &self,
        status: SurfaceStatus,
        source: &EventDraft,
        valid_from: DateTime<Utc>,
        members: &[FragmentId],
    ) {
        self.insert_surface_version(SurfaceFixture {
            surface_id: SurfaceId::new(),
            version: 1,
            status,
            source,
            valid_from,
            valid_to: None,
            members,
        })
        .await;
    }

    async fn insert_surface_version(&self, fixture: SurfaceFixture<'_>) {
        let SurfaceFixture {
            surface_id,
            version,
            status,
            source,
            valid_from,
            valid_to,
            members,
        } = fixture;
        let scope_key = self.scope_key(source.fragment_id).await;
        let evidence = serde_json::json!([{
            "scope": self.scope,
            "event_id": source.id,
            "fragment_id": source.fragment_id,
            "source_id": source.payload.source.source_id,
            "sequence": 0,
        }]);
        sqlx::query(
            "INSERT INTO surfaces (
                id, scope_key, kind, status, semantic_center_json,
                activation_threshold, confidence, version, valid_from_us,
                valid_to_us, source_evidence_json, created_at_us
             ) VALUES (?1, ?2, 'dynamic', ?3, '{}', 0.5, 0.9, ?4, ?5, ?6, ?7, ?5)",
        )
        .bind(surface_id.to_string())
        .bind(&scope_key)
        .bind(surface_status_name(status))
        .bind(i64::try_from(version).expect("Surface 版本可写入 SQLite"))
        .bind(valid_from.timestamp_micros())
        .bind(valid_to.map(|value| value.timestamp_micros()))
        .bind(evidence.to_string())
        .execute(self.store.pool())
        .await
        .expect("写入 Surface 版本");
        for member in members {
            sqlx::query(
                "INSERT INTO surface_members (
                    surface_id, point_id, member_weight, version, created_at_us
                 ) VALUES (?1, ?2, 1.0, ?3, ?4)",
            )
            .bind(surface_id.to_string())
            .bind(member.to_string())
            .bind(i64::try_from(version).expect("Surface 版本可写入 SQLite"))
            .bind(valid_from.timestamp_micros())
            .execute(self.store.pool())
            .await
            .expect("写入 Surface 成员");
        }
    }
}

fn procedure_sample(scope: &MemoryScope, event: &EventDraft, succeeded: bool) -> serde_json::Value {
    serde_json::json!({
        "scope": scope,
        "event_id": event.id,
        "fragment_id": event.fragment_id,
        "source_id": event.payload.source.source_id,
        "change_magnitude": "small",
        "succeeded": succeeded,
    })
}

fn point_id(fragment_id: FragmentId) -> PointId {
    fragment_id
        .to_string()
        .parse()
        .expect("FragmentId 转 PointId")
}

const fn kind_name(kind: FragmentKind) -> &'static str {
    match kind {
        FragmentKind::Constraint => "constraint",
        FragmentKind::Preference => "preference",
        _ => "fact",
    }
}

const fn surface_status_name(status: SurfaceStatus) -> &'static str {
    match status {
        SurfaceStatus::Active => "active",
        SurfaceStatus::Frozen => "frozen",
        SurfaceStatus::Inactive => "inactive",
        SurfaceStatus::Merged => "merged",
        SurfaceStatus::Split => "split",
    }
}

fn fragment(
    scope: MemoryScope,
    kind: FragmentKind,
    source_id: &str,
    text: &str,
    verified: bool,
    evidence: Vec<EvidenceRef>,
) -> CognitiveFragment {
    let now: DateTime<Utc> = Utc::now();
    CognitiveFragment {
        id: FragmentId::new(),
        scope,
        session_id: None,
        trace_id: None,
        kind,
        content: FragmentContent::Text { text: text.into() },
        source: FragmentSource {
            source_type: SourceType::Tool,
            source_id: source_id.into(),
            actor_id: None,
            observer_id: Some("storage-test".into()),
            tool_id: Some("storage-test".into()),
            uri: None,
        },
        perspective: Perspective::Tool,
        occurred_at: now,
        observed_at: now,
        confidence: 0.9,
        importance: 0.7,
        validity: if verified {
            ValidityStatus::Verified
        } else {
            ValidityStatus::Unverified
        },
        security_label: SecurityLabel::Internal,
        evidence,
        metadata: serde_json::Map::new(),
    }
}
