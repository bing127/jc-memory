//! 保守经验学习的端到端契约测试。

use std::{collections::BTreeSet, str::FromStr as _, sync::Arc, time::Duration};

use async_trait::async_trait;
use chrono::Utc;
use jc_memory_consolidation::{
    AuthorityEvent, BatchContext, BatchOutcome, ChangeMagnitude, ColdTierMovement,
    ConflictDetection, ConsolidationJobStore, ConsolidationTask, Deduplication, EdgeReinforcement,
    EdgeWeakening, EdgeWrite, ExperienceSample, JobError, JobKind, JobRunner, JobSpec, PointWrite,
    Rebuild, Replay, ResourceLimits, SemanticPromotion, StaleStateDetection, SurfaceDiscovery,
    SurfaceMerge, SurfaceSplit, SurfaceVersionWrite,
};
use jc_memory_events::factory::EventFactory;
use jc_memory_protocol::{
    CognitiveFragment, EdgeId, EventId, EvidenceId, EvidenceRef, FragmentContent, FragmentId,
    FragmentKind, FragmentSource, MemoryScope, NamespaceId, Perspective, PointId, SecurityLabel,
    SourceType, SurfaceId, ValidityStatus, VerificationMethod,
};
use jc_memory_storage::{MemoryRepository, SqliteConfig, SqliteMemoryRepository, SqliteStore};

#[test]
fn repeated_small_successes_and_large_failures_produce_qualified_experience() {
    let scope = MemoryScope::new(NamespaceId::new());
    let samples = [
        sample(&scope, ChangeMagnitude::Small, true),
        sample(&scope, ChangeMagnitude::Small, true),
        sample(&scope, ChangeMagnitude::Small, true),
        sample(&scope, ChangeMagnitude::Small, true),
        sample(&scope, ChangeMagnitude::Large, false),
        sample(&scope, ChangeMagnitude::Large, false),
    ];

    let learning = SemanticPromotion::learn_procedure(&scope, "refactor.module", &samples)
        .expect("独立的权威样本应产生保守经验");

    assert_eq!(learning.sample_count, 6);
    assert_eq!(learning.evidence.len(), 6);
    assert!(learning.confidence_interval.lower >= 0.0);
    assert!(learning.confidence_interval.upper <= 1.0);
    assert!(learning.confidence_interval.lower < learning.confidence_interval.upper);
    assert!(learning.statement.contains("当前 6 个样本"));
    assert!(learning.statement.contains("更可能"));
    assert!(!learning.statement.contains("总是"));
    assert!(!learning.statement.contains("绝不"));
}

#[test]
fn large_success_advantage_reverses_direction_and_interval() {
    let scope = MemoryScope::new(NamespaceId::new());
    let samples = [
        sample(&scope, ChangeMagnitude::Small, false),
        sample(&scope, ChangeMagnitude::Small, false),
        sample(&scope, ChangeMagnitude::Small, false),
        sample(&scope, ChangeMagnitude::Small, false),
        sample(&scope, ChangeMagnitude::Large, true),
        sample(&scope, ChangeMagnitude::Large, true),
        sample(&scope, ChangeMagnitude::Large, true),
        sample(&scope, ChangeMagnitude::Large, true),
    ];

    let learning = SemanticPromotion::learn_procedure(&scope, "refactor.module", &samples)
        .expect("两组独立样本应产生有方向的保守经验");

    assert!(learning.statement.contains("大改更可能成功"));
    assert!(!learning.statement.contains("小改更可能成功"));
    assert!((learning.confidence_interval.lower - 0.510_100).abs() < 0.000_001);
    assert!((learning.confidence_interval.upper - 1.0).abs() < 0.000_001);
}

#[test]
fn one_magnitude_group_cannot_produce_directional_experience() {
    let scope = MemoryScope::new(NamespaceId::new());
    let samples = [
        sample(&scope, ChangeMagnitude::Small, true),
        sample(&scope, ChangeMagnitude::Small, true),
        sample(&scope, ChangeMagnitude::Small, true),
        sample(&scope, ChangeMagnitude::Small, true),
    ];

    let learning = SemanticPromotion::learn_procedure(&scope, "refactor.module", &samples)
        .expect("单组独立样本应保留为无方向经验");

    assert!(learning.statement.contains("不能形成规模方向性结论"));
    assert!(!learning.statement.contains("更可能成功"));
    assert!((learning.confidence_interval.lower - 0.510_100).abs() < 0.000_001);
    assert!((learning.confidence_interval.upper - 1.0).abs() < 0.000_001);
}

#[test]
fn directional_experience_requires_independent_sources_in_each_group() {
    let scope = MemoryScope::new(NamespaceId::new());
    let mut samples = [
        sample(&scope, ChangeMagnitude::Small, true),
        sample(&scope, ChangeMagnitude::Small, true),
        sample(&scope, ChangeMagnitude::Large, false),
        sample(&scope, ChangeMagnitude::Large, false),
    ];
    samples[0].source_id = "shared-small-source".into();
    samples[1].source_id = "shared-small-source".into();

    let learning = SemanticPromotion::learn_procedure(&scope, "refactor.module", &samples)
        .expect("全局证据足够时应保留无方向经验");

    assert!(learning.statement.contains("不能形成规模方向性结论"));
    assert!(!learning.statement.contains("更可能成功"));
}

#[test]
fn equal_group_success_rates_produce_neutral_experience() {
    let scope = MemoryScope::new(NamespaceId::new());
    let samples = [
        sample(&scope, ChangeMagnitude::Small, true),
        sample(&scope, ChangeMagnitude::Small, false),
        sample(&scope, ChangeMagnitude::Large, true),
        sample(&scope, ChangeMagnitude::Large, false),
    ];

    let learning = SemanticPromotion::learn_procedure(&scope, "refactor.module", &samples)
        .expect("成功率相同的独立样本应产生无方向经验");

    assert!(learning.statement.contains("不能形成规模方向性结论"));
    assert!(!learning.statement.contains("更可能成功"));
    assert!((learning.confidence_interval.lower - 0.150_036).abs() < 0.000_001);
    assert!((learning.confidence_interval.upper - 0.849_964).abs() < 0.000_001);
}

fn sample(
    scope: &MemoryScope,
    change_magnitude: ChangeMagnitude,
    succeeded: bool,
) -> ExperienceSample {
    ExperienceSample {
        scope: scope.clone(),
        event_id: EventId::new(),
        fragment_id: FragmentId::new(),
        source_id: format!("source-{}", EventId::new()),
        change_magnitude,
        succeeded,
    }
}

#[tokio::test]
async fn dedup_uses_exact_content_without_inventing_causality() {
    let directory = tempfile::tempdir().expect("创建测试目录");
    let store = SqliteStore::open(&SqliteConfig::new(directory.path().join("memory.sqlite3")))
        .await
        .expect("打开测试存储");
    let repository = SqliteMemoryRepository::from_store(&store);
    let scope = MemoryScope::new(NamespaceId::new());
    for source_id in ["source-a", "source-b"] {
        repository
            .append(&fact(&scope, source_id, "数据库采用 SQLite"))
            .await
            .expect("写入权威事实");
    }

    run_task(&store, &scope, JobKind::Replay, Replay).await;
    run_task(&store, &scope, JobKind::Deduplication, Deduplication).await;

    let kinds: Vec<String> = sqlx::query_scalar("SELECT kind FROM edges ORDER BY kind")
        .fetch_all(store.pool())
        .await
        .expect("读取派生关系");
    assert_eq!(kinds, vec!["same_entity"]);
    assert!(!kinds.iter().any(|kind| kind == "causes"));
}

#[test]
fn every_consolidation_handler_implements_the_bounded_task_contract() {
    fn assert_task<Task: jc_memory_consolidation::ConsolidationTask>() {}

    assert_task::<Replay>();
    assert_task::<Deduplication>();
    assert_task::<EdgeReinforcement>();
    assert_task::<EdgeWeakening>();
    assert_task::<SurfaceDiscovery>();
    assert_task::<SurfaceMerge>();
    assert_task::<SurfaceSplit>();
    assert_task::<SemanticPromotion>();
    assert_task::<ConflictDetection>();
    assert_task::<StaleStateDetection>();
    assert_task::<ColdTierMovement>();
    assert_task::<Rebuild>();
}

#[tokio::test]
async fn canonical_ids_and_recall_artifacts_are_persisted() {
    let directory = tempfile::tempdir().expect("创建规范派生测试目录");
    let store = SqliteStore::open(&SqliteConfig::new(directory.path().join("memory.sqlite3")))
        .await
        .expect("打开规范派生测试存储");
    let repository = SqliteMemoryRepository::from_store(&store);
    let scope = MemoryScope::new(NamespaceId::new());
    let support_a = fact(&scope, "support-a", "约束证据 A");
    let support_b = fact(&scope, "support-b", "约束证据 B");
    repository.append(&support_a).await.expect("写入支持证据 A");
    repository.append(&support_b).await.expect("写入支持证据 B");
    let constraint_a = verified_fragment(
        &scope,
        "constraint-a",
        "部署前必须通过回归测试",
        FragmentKind::Constraint,
        &support_a,
    );
    let constraint_b = verified_fragment(
        &scope,
        "constraint-b",
        "部署前必须通过回归测试",
        FragmentKind::Constraint,
        &support_b,
    );
    repository
        .append(&constraint_a)
        .await
        .expect("写入独立约束 A");
    repository
        .append(&constraint_b)
        .await
        .expect("写入独立约束 B");
    for (index, succeeded) in [true, true, false, false].into_iter().enumerate() {
        repository
            .append(&procedure_event(
                &scope,
                &format!("procedure-source-{index}"),
                if index < 2 { "small" } else { "large" },
                succeeded,
            ))
            .await
            .expect("写入 Procedure 样本");
    }

    run_task(&store, &scope, JobKind::Replay, Replay).await;
    run_task(&store, &scope, JobKind::Deduplication, Deduplication).await;
    run_task(&store, &scope, JobKind::SurfaceDiscovery, SurfaceDiscovery).await;
    run_task(
        &store,
        &scope,
        JobKind::SemanticPromotion,
        SemanticPromotion,
    )
    .await;

    let points: Vec<(String, String)> =
        sqlx::query_as("SELECT id, source_fragment_id FROM points ORDER BY source_fragment_id")
            .fetch_all(store.pool())
            .await
            .expect("读取规范 Point");
    assert!(
        points
            .iter()
            .all(|(id, fragment_id)| { PointId::from_str(id).is_ok() && id == fragment_id })
    );
    let edge_ids: Vec<String> = sqlx::query_scalar("SELECT id FROM edges")
        .fetch_all(store.pool())
        .await
        .expect("读取规范 Edge ID");
    assert!(edge_ids.iter().all(|id| EdgeId::from_str(id).is_ok()));
    let surface_ids: Vec<String> = sqlx::query_scalar("SELECT DISTINCT id FROM surfaces")
        .fetch_all(store.pool())
        .await
        .expect("读取规范 Surface ID");
    assert!(surface_ids.iter().all(|id| SurfaceId::from_str(id).is_ok()));

    let procedure_payload: String = sqlx::query_scalar(
        "SELECT payload_json FROM consolidation_artifacts
         WHERE kind = 'procedure_experience' AND subject_key = 'refactor.module'",
    )
    .fetch_one(store.pool())
    .await
    .expect("读取 Procedure Artifact");
    let procedure: serde_json::Value =
        serde_json::from_str(&procedure_payload).expect("解析 Procedure Artifact");
    assert_eq!(procedure["sample_count"], 4);
    assert_eq!(procedure["success_count"], 2);
    assert_eq!(procedure["failure_count"], 2);
    assert_eq!(procedure["success_rate"], 0.5);

    let knowledge_payload: String = sqlx::query_scalar(
        "SELECT payload_json FROM consolidation_artifacts
         WHERE kind = 'semantic_promotion' LIMIT 1",
    )
    .fetch_one(store.pool())
    .await
    .expect("读取 Knowledge Artifact");
    let knowledge: serde_json::Value =
        serde_json::from_str(&knowledge_payload).expect("解析 Knowledge Artifact");
    assert!(PointId::from_str(knowledge["point_id"].as_str().unwrap()).is_ok());
    assert_eq!(knowledge["kind"], "constraint");
    assert_eq!(knowledge["content"]["text"], "部署前必须通过回归测试");
    assert!(knowledge["confidence"].is_number());
    assert_eq!(knowledge["validity"], "verified");
}

#[tokio::test]
async fn semantic_promotion_requires_independent_verified_sources() {
    let directory = tempfile::tempdir().expect("创建测试目录");
    let store = SqliteStore::open(&SqliteConfig::new(directory.path().join("memory.sqlite3")))
        .await
        .expect("打开测试存储");
    let repository = SqliteMemoryRepository::from_store(&store);
    let scope = MemoryScope::new(NamespaceId::new());
    let support_a = fact(&scope, "evidence-a", "工具证据 A");
    let support_b = fact(&scope, "evidence-b", "工具证据 B");
    repository.append(&support_a).await.expect("写入证据 A");
    repository.append(&support_b).await.expect("写入证据 B");
    for (source_id, support) in [("source-a", &support_a), ("source-b", &support_b)] {
        repository
            .append(&verified_fact(
                &scope,
                source_id,
                "数据库采用 SQLite",
                support,
            ))
            .await
            .expect("写入独立事实");
    }
    repository
        .append(&hypothesis(&scope, "model-a", "未来一定成功"))
        .await
        .expect("写入 Prediction");

    run_task(
        &store,
        &scope,
        JobKind::SemanticPromotion,
        SemanticPromotion,
    )
    .await;

    let artifacts: Vec<String> = sqlx::query_scalar(
        "SELECT evidence_json FROM consolidation_artifacts WHERE kind = 'semantic_promotion'",
    )
    .fetch_all(store.pool())
    .await
    .expect("读取语义提升 Evidence");
    assert_eq!(artifacts.len(), 1);
    assert!(artifacts[0].contains("source-a"));
    assert!(artifacts[0].contains("source-b"));
    assert!(!artifacts[0].contains("model-a"));
    let prediction_points: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM points p JOIN fragments f ON f.id = p.source_fragment_id WHERE f.kind = 'prediction'",
    )
    .fetch_one(store.pool())
    .await
    .expect("检查 Prediction 派生 Point");
    assert_eq!(prediction_points, 0);

    repository
        .append(&verified_fact(
            &scope,
            "source-c",
            "数据库采用 SQLite",
            &support_b,
        ))
        .await
        .expect("写入新增独立来源");
    run_task(
        &store,
        &scope,
        JobKind::SemanticPromotion,
        SemanticPromotion,
    )
    .await;
    let refreshed: Vec<String> = sqlx::query_scalar(
        "SELECT evidence_json FROM consolidation_artifacts WHERE kind = 'semantic_promotion'",
    )
    .fetch_all(store.pool())
    .await
    .expect("读取增量语义 Evidence");
    assert_eq!(refreshed.len(), 1);
    assert!(refreshed[0].contains("source-c"));
}

#[tokio::test]
async fn semantic_job_persists_incremental_procedure_experience() {
    let directory = tempfile::tempdir().expect("创建测试目录");
    let store = SqliteStore::open(&SqliteConfig::new(directory.path().join("memory.sqlite3")))
        .await
        .expect("打开测试存储");
    let repository = SqliteMemoryRepository::from_store(&store);
    let scope = MemoryScope::new(NamespaceId::new());
    for (index, magnitude, succeeded) in [
        (0, "small", true),
        (1, "small", true),
        (2, "small", true),
        (3, "small", true),
        (4, "large", false),
        (5, "large", false),
    ] {
        repository
            .append(&procedure_event(
                &scope,
                &format!("procedure-source-{index}"),
                magnitude,
                succeeded,
            ))
            .await
            .expect("写入 Procedure outcome Event");
    }
    repository
        .append(&event_with_metadata(
            &scope,
            "procedure-prediction",
            "预测重构会成功",
            FragmentKind::Prediction,
            ValidityStatus::Unverified,
            Vec::new(),
            serde_json::Map::from_iter([
                ("procedure_key".into(), "refactor.module".into()),
                ("change_magnitude".into(), "small".into()),
                ("succeeded".into(), true.into()),
            ]),
        ))
        .await
        .expect("写入不得进入经验规则的 Prediction");

    run_task_with_batch(
        &store,
        &scope,
        JobKind::SemanticPromotion,
        SemanticPromotion,
        2,
    )
    .await;

    let (payload_json, evidence_json): (String, String) = sqlx::query_as(
        "SELECT payload_json, evidence_json FROM consolidation_artifacts
         WHERE kind = 'procedure_experience' AND subject_key = 'refactor.module'",
    )
    .fetch_one(store.pool())
    .await
    .expect("读取持久化 Procedure 经验");
    let payload: serde_json::Value = serde_json::from_str(&payload_json).expect("解析经验载荷");
    let evidence: serde_json::Value =
        serde_json::from_str(&evidence_json).expect("解析经验 Evidence");
    assert_eq!(payload["sample_count"], 6);
    assert!(payload["confidence_interval"]["lower"].is_number());
    assert!(payload["confidence_interval"]["upper"].is_number());
    assert!(
        payload["statement"]
            .as_str()
            .unwrap()
            .contains("当前 6 个样本")
    );
    assert_eq!(evidence.as_array().unwrap().len(), 6);
}

#[tokio::test]
async fn surface_changes_append_versions_without_overwriting_history() {
    let directory = tempfile::tempdir().expect("创建测试目录");
    let store = SqliteStore::open(&SqliteConfig::new(directory.path().join("memory.sqlite3")))
        .await
        .expect("打开测试存储");
    let repository = SqliteMemoryRepository::from_store(&store);
    let scope = MemoryScope::new(NamespaceId::new());
    for (source_id, text) in [("surface-a", "事实 A"), ("surface-b", "事实 B")] {
        repository
            .append(&fact(&scope, source_id, text))
            .await
            .expect("写入 Surface 来源");
    }
    run_task(&store, &scope, JobKind::Replay, Replay).await;
    run_task(&store, &scope, JobKind::SurfaceDiscovery, SurfaceDiscovery).await;
    let source_ids: Vec<String> = sqlx::query_scalar("SELECT id FROM surfaces ORDER BY id")
        .fetch_all(store.pool())
        .await
        .expect("读取 Merge 来源 Surface");
    assert_eq!(source_ids.len(), 2);
    run_task(&store, &scope, JobKind::SurfaceMerge, SurfaceMerge).await;

    for source_id in &source_ids {
        let statuses: Vec<String> =
            sqlx::query_scalar("SELECT status FROM surfaces WHERE id = ?1 ORDER BY version")
                .bind(source_id)
                .fetch_all(store.pool())
                .await
                .expect("读取 Merge 来源历史");
        assert_eq!(statuses, vec!["active", "merged"]);
        assert_surface_history_is_contiguous(store.pool(), source_id).await;
    }
    let merged_successor_id: String = sqlx::query_scalar(
        "SELECT id FROM surfaces
         WHERE version = 1 AND status = 'active' AND id NOT IN (?1, ?2)",
    )
    .bind(&source_ids[0])
    .bind(&source_ids[1])
    .fetch_one(store.pool())
    .await
    .expect("读取 Merge 后继");
    let merged_members = surface_members(store.pool(), &merged_successor_id, 1).await;
    assert_eq!(merged_members.len(), 2);

    run_task(&store, &scope, JobKind::SurfaceSplit, SurfaceSplit).await;
    let split_source_statuses: Vec<String> =
        sqlx::query_scalar("SELECT status FROM surfaces WHERE id = ?1 ORDER BY version")
            .bind(&merged_successor_id)
            .fetch_all(store.pool())
            .await
            .expect("读取 Split 来源历史");
    assert_eq!(split_source_statuses, vec!["active", "split"]);
    assert_surface_history_is_contiguous(store.pool(), &merged_successor_id).await;
    let split_successors: Vec<String> = sqlx::query_scalar(
        "SELECT s.id FROM surfaces s
         WHERE s.version = (SELECT max(v.version) FROM surfaces v WHERE v.id = s.id)
           AND s.status = 'active'
         ORDER BY s.id",
    )
    .fetch_all(store.pool())
    .await
    .expect("读取 Split 后继");
    assert!(split_successors.len() >= 2);
    assert!(split_successors.iter().all(|id| id != &merged_successor_id));
    let mut covered = BTreeSet::new();
    for successor_id in &split_successors {
        let members = surface_members(store.pool(), successor_id, 1).await;
        assert!(!members.is_empty());
        for member in members {
            assert!(covered.insert(member), "Split 后继成员不得重叠");
        }
    }
    assert_eq!(covered, merged_members);
}

async fn assert_surface_history_is_contiguous(pool: &sqlx::SqlitePool, surface_id: &str) {
    let boundary: (Option<i64>, i64) = sqlx::query_as(
        "SELECT previous.valid_to_us, current.valid_from_us
         FROM surfaces previous
         JOIN surfaces current ON current.id = previous.id AND current.version = 2
         WHERE previous.id = ?1 AND previous.version = 1",
    )
    .bind(surface_id)
    .fetch_one(pool)
    .await
    .expect("读取连续 Surface 有效期");
    assert_eq!(boundary.0, Some(boundary.1));
}

async fn surface_members(
    pool: &sqlx::SqlitePool,
    surface_id: &str,
    version: i64,
) -> BTreeSet<String> {
    sqlx::query_scalar(
        "SELECT point_id FROM surface_members
         WHERE surface_id = ?1 AND version = ?2 ORDER BY point_id",
    )
    .bind(surface_id)
    .bind(version)
    .fetch_all(pool)
    .await
    .expect("读取 Surface 成员")
    .into_iter()
    .collect()
}

#[tokio::test]
async fn typed_writes_reject_references_from_another_exact_scope() {
    let directory = tempfile::tempdir().expect("创建测试目录");
    let store = SqliteStore::open(&SqliteConfig::new(directory.path().join("memory.sqlite3")))
        .await
        .expect("打开测试存储");
    let repository = SqliteMemoryRepository::from_store(&store);
    let local_scope = MemoryScope::new(NamespaceId::new());
    let foreign_scope = MemoryScope::new(NamespaceId::new());
    let local = fact(&local_scope, "local-source", "本 Scope 事实");
    let foreign = fact(&foreign_scope, "foreign-source", "外 Scope 事实");
    repository.append(&local).await.expect("写入本 Scope 事实");
    repository
        .append(&foreign)
        .await
        .expect("写入外 Scope 事实");
    run_task(&store, &local_scope, JobKind::Replay, Replay).await;
    run_task(&store, &foreign_scope, JobKind::Replay, Replay).await;
    run_task(
        &store,
        &foreign_scope,
        JobKind::SurfaceDiscovery,
        SurfaceDiscovery,
    )
    .await;
    let local_point: String =
        sqlx::query_scalar("SELECT id FROM points WHERE source_fragment_id = ?1")
            .bind(local.fragment_id.to_string())
            .fetch_one(store.pool())
            .await
            .expect("读取本 Scope Point");
    let foreign_point: String =
        sqlx::query_scalar("SELECT id FROM points WHERE source_fragment_id = ?1")
            .bind(foreign.fragment_id.to_string())
            .fetch_one(store.pool())
            .await
            .expect("读取外 Scope Point");
    let foreign_surface: String =
        sqlx::query_scalar("SELECT surface_id FROM surface_members WHERE point_id = ?1 LIMIT 1")
            .bind(&foreign_point)
            .fetch_one(store.pool())
            .await
            .expect("读取外 Scope Surface");
    let foreign_event = authority_event_from_draft(&foreign);

    let point_error = run_task_error(
        &store,
        &local_scope,
        CrossScopeWrite::Point(foreign.fragment_id.to_string()),
    )
    .await;
    assert_eq!(
        point_error,
        JobError::ReferenceScopeMismatch("source_fragment_id")
    );
    let edge_error = run_task_error(
        &store,
        &local_scope,
        CrossScopeWrite::Edge {
            local_point: local_point.clone(),
            foreign_point: foreign_point.clone(),
            local_fragment: local.fragment_id.to_string(),
        },
    )
    .await;
    assert_eq!(
        edge_error,
        JobError::ReferenceScopeMismatch("edge_references")
    );
    let surface_error = run_task_error(
        &store,
        &local_scope,
        CrossScopeWrite::Surface(foreign_point.clone()),
    )
    .await;
    assert_eq!(
        surface_error,
        JobError::ReferenceScopeMismatch("surface_member")
    );
    let surface_version_error = run_task_error(
        &store,
        &local_scope,
        CrossScopeWrite::SurfaceVersion {
            surface_id: foreign_surface,
            local_point,
        },
    )
    .await;
    assert_eq!(
        surface_version_error,
        JobError::ReferenceScopeMismatch("surface_id")
    );
    for task in [
        CrossScopeWrite::ColdTier(foreign_event.clone()),
        CrossScopeWrite::Fts(foreign_event),
    ] {
        assert_eq!(
            run_task_error(&store, &local_scope, task).await,
            JobError::ReferenceScopeMismatch("authority_event")
        );
    }
}

fn authority_event_from_draft(draft: &jc_memory_protocol::EventDraft) -> AuthorityEvent {
    AuthorityEvent {
        sequence: 0,
        event_id: draft.id.to_string(),
        fragment_id: draft.fragment_id.to_string(),
        fragment_kind: "fact".into(),
        source_id: draft.payload.source.source_id.clone(),
        content_hash: draft.content_hash.to_string(),
        content_id: String::new(),
        payload: serde_json::to_value(&draft.payload).expect("序列化外 Scope Event"),
        recorded_at_us: draft.recorded_at.timestamp_micros(),
        confidence: f64::from(draft.payload.confidence),
        validity: "unverified".into(),
    }
}

enum CrossScopeWrite {
    Point(String),
    Edge {
        local_point: String,
        foreign_point: String,
        local_fragment: String,
    },
    Surface(String),
    SurfaceVersion {
        surface_id: String,
        local_point: String,
    },
    ColdTier(AuthorityEvent),
    Fts(AuthorityEvent),
}

#[async_trait]
impl ConsolidationTask for CrossScopeWrite {
    async fn process_batch(
        &self,
        context: &mut BatchContext<'_>,
    ) -> Result<BatchOutcome, JobError> {
        context.consume_items(1)?;
        match self {
            Self::Point(fragment_id) => {
                context
                    .write_point(&PointWrite {
                        id: "cross-scope-point".into(),
                        kind: "fact".into(),
                        source_fragment_id: fragment_id.clone(),
                        valid_from_us: 1,
                        confidence: 0.5,
                        created_at_us: 1,
                    })
                    .await?;
            }
            Self::Edge {
                local_point,
                foreign_point,
                local_fragment,
            } => {
                context
                    .write_edge(&EdgeWrite {
                        id: "cross-scope-edge".into(),
                        source_point_id: local_point.clone(),
                        target_point_id: foreign_point.clone(),
                        kind: "supports".into(),
                        weight: 0.5,
                        confidence: 0.5,
                        source_fragment_id: local_fragment.clone(),
                        valid_from_us: 1,
                        created_at_us: 1,
                    })
                    .await?;
            }
            Self::Surface(point_id) => {
                context
                    .write_surface_version(&SurfaceVersionWrite {
                        id: "cross-scope-surface".into(),
                        kind: "dynamic".into(),
                        status: "active".into(),
                        semantic_center: serde_json::json!([]),
                        activation_threshold: 0.5,
                        confidence: 0.5,
                        version: 1,
                        valid_from_us: 1,
                        evidence: serde_json::json!([]),
                        members: vec![(point_id.clone(), 1.0)],
                    })
                    .await?;
            }
            Self::SurfaceVersion {
                surface_id,
                local_point,
            } => {
                context
                    .write_surface_version(&SurfaceVersionWrite {
                        id: surface_id.clone(),
                        kind: "dynamic".into(),
                        status: "frozen".into(),
                        semantic_center: serde_json::json!([]),
                        activation_threshold: 0.5,
                        confidence: 0.5,
                        version: 2,
                        valid_from_us: 2,
                        evidence: serde_json::json!([]),
                        members: vec![(local_point.clone(), 1.0)],
                    })
                    .await?;
            }
            Self::ColdTier(event) => {
                context
                    .write_cold_tier(event, &serde_json::json!([]))
                    .await?;
            }
            Self::Fts(event) => {
                context.write_fts(event, "外 Scope 文本").await?;
            }
        }
        Ok(BatchOutcome {
            next_cursor: context.checkpoint().cursor.saturating_add(1),
            complete: true,
        })
    }
}

#[tokio::test]
async fn missing_edge_never_produces_adjustment_artifact() {
    let directory = tempfile::tempdir().expect("创建测试目录");
    let store = SqliteStore::open(&SqliteConfig::new(directory.path().join("memory.sqlite3")))
        .await
        .expect("打开测试存储");
    let repository = SqliteMemoryRepository::from_store(&store);
    let scope = MemoryScope::new(NamespaceId::new());
    repository
        .append(&event_with_metadata(
            &scope,
            "missing-edge-source",
            "不存在 Edge 的成功证据",
            FragmentKind::Success,
            ValidityStatus::Unverified,
            Vec::new(),
            serde_json::Map::from_iter([("edge_id".into(), "missing-edge".into())]),
        ))
        .await
        .expect("写入缺失 Edge 的调整 Evidence");

    run_task(
        &store,
        &scope,
        JobKind::EdgeReinforcement,
        EdgeReinforcement,
    )
    .await;

    let artifacts: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM consolidation_artifacts WHERE kind = 'edge_reinforcement'",
    )
    .fetch_one(store.pool())
    .await
    .expect("统计 Edge 调整产物");
    assert_eq!(artifacts, 0);
}

#[tokio::test]
async fn rebuild_changes_only_reconstructable_tables() {
    let directory = tempfile::tempdir().expect("创建测试目录");
    let store = SqliteStore::open(&SqliteConfig::new(directory.path().join("memory.sqlite3")))
        .await
        .expect("打开测试存储");
    let repository = SqliteMemoryRepository::from_store(&store);
    let scope = MemoryScope::new(NamespaceId::new());
    repository
        .append(&fact(&scope, "rebuild-source", "可重建文本"))
        .await
        .expect("写入 Rebuild 来源");
    let before = authoritative_counts(&store).await;

    run_task(&store, &scope, JobKind::Rebuild, Rebuild).await;

    assert_eq!(authoritative_counts(&store).await, before);
    let point_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM points")
        .fetch_one(store.pool())
        .await
        .expect("统计重建 Point");
    let fts_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM fragment_fts")
        .fetch_one(store.pool())
        .await
        .expect("统计重建 FTS");
    assert_eq!((point_count, fts_count), (1, 1));
}

#[tokio::test]
async fn metadata_driven_handlers_persist_derived_results_only() {
    let directory = tempfile::tempdir().expect("创建测试目录");
    let store = SqliteStore::open(&SqliteConfig::new(directory.path().join("memory.sqlite3")))
        .await
        .expect("打开测试存储");
    let repository = SqliteMemoryRepository::from_store(&store);
    let scope = MemoryScope::new(NamespaceId::new());
    let first = fact(&scope, "edge-a", "同一事实");
    let second = fact(&scope, "edge-b", "同一事实");
    repository
        .append(&first)
        .await
        .expect("写入第一个 Point 来源");
    repository
        .append(&second)
        .await
        .expect("写入第二个 Point 来源");
    run_task(&store, &scope, JobKind::Replay, Replay).await;
    run_task(&store, &scope, JobKind::Deduplication, Deduplication).await;
    let edge_id: String = sqlx::query_scalar("SELECT id FROM edges LIMIT 1")
        .fetch_one(store.pool())
        .await
        .expect("读取派生 Edge");

    append_handler_evidence(&repository, &scope, first.fragment_id, &edge_id).await;
    let authoritative_before = authoritative_counts(&store).await;

    run_task(
        &store,
        &scope,
        JobKind::EdgeReinforcement,
        EdgeReinforcement,
    )
    .await;
    let reinforced: f64 = sqlx::query_scalar("SELECT weight FROM edges WHERE id = ?1")
        .bind(&edge_id)
        .fetch_one(store.pool())
        .await
        .expect("读取强化权重");
    assert!((reinforced - 0.55).abs() < f64::EPSILON);
    run_task(&store, &scope, JobKind::EdgeWeakening, EdgeWeakening).await;
    run_task(
        &store,
        &scope,
        JobKind::ConflictDetection,
        ConflictDetection,
    )
    .await;
    run_task(
        &store,
        &scope,
        JobKind::StaleStateDetection,
        StaleStateDetection,
    )
    .await;
    run_task(&store, &scope, JobKind::ColdTierMovement, ColdTierMovement).await;

    let final_weight: f64 = sqlx::query_scalar("SELECT weight FROM edges WHERE id = ?1")
        .bind(&edge_id)
        .fetch_one(store.pool())
        .await
        .expect("读取弱化权重");
    assert!((final_weight - 0.5).abs() < f64::EPSILON);
    let artifact_kinds: Vec<String> =
        sqlx::query_scalar("SELECT DISTINCT kind FROM consolidation_artifacts ORDER BY kind")
            .fetch_all(store.pool())
            .await
            .expect("读取派生产物类别");
    assert_eq!(
        artifact_kinds,
        vec![
            "conflict",
            "edge_reinforcement",
            "edge_weakening",
            "stale_state"
        ]
    );
    let cold_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM cold_tier_entries")
        .fetch_one(store.pool())
        .await
        .expect("统计冷层索引");
    assert_eq!(cold_count, 1);
    assert_eq!(authoritative_counts(&store).await, authoritative_before);
}

async fn append_handler_evidence(
    repository: &SqliteMemoryRepository,
    scope: &MemoryScope,
    target_fragment_id: FragmentId,
    edge_id: &str,
) {
    for (kind, source_id, text) in [
        (FragmentKind::Success, "success-source", "Edge 成功证据"),
        (FragmentKind::Failure, "failure-source", "Edge 失败证据"),
    ] {
        repository
            .append(&event_with_metadata(
                scope,
                source_id,
                text,
                kind,
                ValidityStatus::Unverified,
                Vec::new(),
                serde_json::Map::from_iter([("edge_id".into(), edge_id.into())]),
            ))
            .await
            .expect("写入 Edge 调整 Evidence");
    }
    repository
        .append(&event_with_metadata(
            scope,
            "state-source",
            "状态冲突与替代",
            FragmentKind::Correction,
            ValidityStatus::Unverified,
            Vec::new(),
            serde_json::Map::from_iter([
                (
                    "contradicts_fragment_id".into(),
                    target_fragment_id.to_string().into(),
                ),
                ("supersedes_state_key".into(), "deployment.status".into()),
            ]),
        ))
        .await
        .expect("写入状态 Evidence");
    repository
        .append(&event_with_metadata(
            scope,
            "cold-source",
            "已失效事实",
            FragmentKind::Fact,
            ValidityStatus::Invalidated,
            Vec::new(),
            serde_json::Map::new(),
        ))
        .await
        .expect("写入冷层 Evidence");
}

async fn run_task<Task>(store: &SqliteStore, scope: &MemoryScope, kind: JobKind, task: Task)
where
    Task: jc_memory_consolidation::ConsolidationTask + 'static,
{
    run_task_result(store, scope, kind, task)
        .await
        .expect("运行巩固任务");
}

async fn run_task_with_batch<Task>(
    store: &SqliteStore,
    scope: &MemoryScope,
    kind: JobKind,
    task: Task,
    batch_size: u32,
) where
    Task: jc_memory_consolidation::ConsolidationTask + 'static,
{
    run_task_result_with_batch(store, scope, kind, task, batch_size)
        .await
        .expect("运行小批量巩固任务");
}

async fn run_task_error<Task>(store: &SqliteStore, scope: &MemoryScope, task: Task) -> JobError
where
    Task: jc_memory_consolidation::ConsolidationTask + 'static,
{
    run_task_result(store, scope, JobKind::Replay, task)
        .await
        .expect_err("跨 Scope 引用必须被拒绝")
}

async fn run_task_result<Task>(
    store: &SqliteStore,
    scope: &MemoryScope,
    kind: JobKind,
    task: Task,
) -> Result<jc_memory_consolidation::JobRecord, JobError>
where
    Task: jc_memory_consolidation::ConsolidationTask + 'static,
{
    run_task_result_with_batch(store, scope, kind, task, 16).await
}

async fn run_task_result_with_batch<Task>(
    store: &SqliteStore,
    scope: &MemoryScope,
    kind: JobKind,
    task: Task,
    batch_size: u32,
) -> Result<jc_memory_consolidation::JobRecord, JobError>
where
    Task: jc_memory_consolidation::ConsolidationTask + 'static,
{
    let jobs = ConsolidationJobStore::from_store(store);
    let record = jobs
        .submit(JobSpec {
            kind,
            scope: scope.clone(),
            cursor: 0,
            limits: ResourceLimits {
                max_batch_size: batch_size,
                max_cpu_ms: 1_000,
                max_io_ops: 1_000,
                max_runtime_ms: 5_000,
                max_attempts: 2,
            },
        })
        .await
        .expect("提交巩固任务");
    JobRunner::new(
        jobs,
        Arc::new(task),
        format!("worker-{kind:?}"),
        Duration::from_secs(1),
        Duration::from_millis(1),
    )
    .run(record.id)
    .await
}

fn fact(scope: &MemoryScope, source_id: &str, text: &str) -> jc_memory_protocol::EventDraft {
    event(
        scope,
        source_id,
        text,
        FragmentKind::Fact,
        ValidityStatus::Unverified,
        Vec::new(),
    )
}

fn verified_fact(
    scope: &MemoryScope,
    source_id: &str,
    text: &str,
    support: &jc_memory_protocol::EventDraft,
) -> jc_memory_protocol::EventDraft {
    verified_fragment(scope, source_id, text, FragmentKind::Fact, support)
}

fn verified_fragment(
    scope: &MemoryScope,
    source_id: &str,
    text: &str,
    kind: FragmentKind,
    support: &jc_memory_protocol::EventDraft,
) -> jc_memory_protocol::EventDraft {
    event(
        scope,
        source_id,
        text,
        kind,
        ValidityStatus::Verified,
        vec![EvidenceRef {
            id: EvidenceId::new(),
            fragment_id: Some(support.fragment_id),
            event_id: None,
            method: VerificationMethod::Tool,
            note: Some("测试权威证据".into()),
        }],
    )
}

fn hypothesis(scope: &MemoryScope, source_id: &str, text: &str) -> jc_memory_protocol::EventDraft {
    event(
        scope,
        source_id,
        text,
        FragmentKind::Prediction,
        ValidityStatus::Unverified,
        Vec::new(),
    )
}

fn procedure_event(
    scope: &MemoryScope,
    source_id: &str,
    change_magnitude: &str,
    succeeded: bool,
) -> jc_memory_protocol::EventDraft {
    event_with_metadata(
        scope,
        source_id,
        "重构结果",
        if succeeded {
            FragmentKind::Success
        } else {
            FragmentKind::Failure
        },
        ValidityStatus::Unverified,
        Vec::new(),
        serde_json::Map::from_iter([
            ("procedure_key".into(), "refactor.module".into()),
            ("change_magnitude".into(), change_magnitude.into()),
            ("succeeded".into(), succeeded.into()),
        ]),
    )
}

fn event(
    scope: &MemoryScope,
    source_id: &str,
    text: &str,
    kind: FragmentKind,
    validity: ValidityStatus,
    evidence: Vec<EvidenceRef>,
) -> jc_memory_protocol::EventDraft {
    event_with_metadata(
        scope,
        source_id,
        text,
        kind,
        validity,
        evidence,
        serde_json::Map::new(),
    )
}

fn event_with_metadata(
    scope: &MemoryScope,
    source_id: &str,
    text: &str,
    kind: FragmentKind,
    validity: ValidityStatus,
    evidence: Vec<EvidenceRef>,
    metadata: serde_json::Map<String, serde_json::Value>,
) -> jc_memory_protocol::EventDraft {
    let now = Utc::now();
    EventFactory::new()
        .from_fragment(CognitiveFragment {
            id: FragmentId::new(),
            scope: scope.clone(),
            session_id: None,
            trace_id: None,
            kind,
            content: FragmentContent::Text { text: text.into() },
            source: FragmentSource {
                source_type: SourceType::Tool,
                source_id: source_id.into(),
                actor_id: None,
                observer_id: None,
                tool_id: Some(source_id.into()),
                uri: None,
            },
            perspective: Perspective::Tool,
            occurred_at: now,
            observed_at: now,
            confidence: 0.9,
            importance: 0.8,
            validity,
            security_label: SecurityLabel::Internal,
            evidence,
            metadata,
        })
        .expect("创建事实 Event")
}

async fn authoritative_counts(store: &SqliteStore) -> (i64, i64, i64) {
    let fragments = sqlx::query_scalar("SELECT COUNT(*) FROM fragments")
        .fetch_one(store.pool())
        .await
        .expect("统计 Fragment");
    let events = sqlx::query_scalar("SELECT COUNT(*) FROM events")
        .fetch_one(store.pool())
        .await
        .expect("统计 Event");
    let evidence = sqlx::query_scalar("SELECT COUNT(*) FROM evidence_refs")
        .fetch_one(store.pool())
        .await
        .expect("统计 EvidenceRef");
    (fragments, events, evidence)
}
