//! Consolidation 派生项的精确 Scope 读取契约。

use chrono::{DateTime, Utc};
use jc_memory_events::factory::EventFactory;
use jc_memory_protocol::{
    CognitiveFragment, EventDraft, EvidenceId, EvidenceRef, FragmentContent, FragmentId,
    FragmentKind, FragmentSource, KnowledgeKind, MemoryScope, NamespaceId, Perspective, PointId,
    SecurityLabel, SourceType, ValidityStatus, VerificationMethod,
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

struct Harness {
    store: SqliteStore,
    repository: SqliteMemoryRepository,
    projection: SqliteProjectionStore,
    scope: MemoryScope,
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
