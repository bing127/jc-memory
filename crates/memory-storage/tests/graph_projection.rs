//! 事务性 Graph Projector 与有界读取契约。

use std::{str::FromStr as _, sync::Arc};

use chrono::{DateTime, Utc};
use jc_memory_events::factory::EventFactory;
use jc_memory_protocol::{
    CognitiveFragment, EdgeId, EdgeKind, EventDraft, EvidenceId, EvidenceRef, FragmentContent,
    FragmentId, FragmentKind, FragmentSource, MemoryScope, NamespaceId, Perspective, PointId,
    SecurityLabel, SourceType, ValidityStatus, VerificationMethod,
};
use jc_memory_storage::{
    MemoryRepository, ProjectorKind, SqliteConfig, SqliteMemoryRepository, SqliteProjectionStore,
    SqliteStore, StorageError,
};
use tempfile::TempDir;

#[tokio::test]
async fn mutation_graph_uses_deterministic_ids_and_survives_restart() {
    let harness = Harness::new().await;
    let target = harness
        .append(point_fragment(
            harness.scope.clone(),
            FragmentKind::Fact,
            "生产数据库使用 SQLite",
            time(10),
        ))
        .await;
    let cases = [
        ("correct", EdgeKind::Supersedes, VerificationMethod::Manual),
        ("reinforce", EdgeKind::Supports, VerificationMethod::Tool),
        (
            "weaken",
            EdgeKind::DisprovedBy,
            VerificationMethod::ExternalSource,
        ),
        (
            "supersede",
            EdgeKind::Supersedes,
            VerificationMethod::CrossEvidence,
        ),
        (
            "contradict",
            EdgeKind::Contradicts,
            VerificationMethod::Manual,
        ),
    ];
    let mut mutations = Vec::new();
    for (mutation, relation, method) in cases {
        mutations.push(
            harness
                .append(mutation_fragment(
                    harness.scope.clone(),
                    target.fragment_id,
                    mutation,
                    relation,
                    method,
                    time(20 + i64::try_from(mutations.len()).unwrap()),
                ))
                .await,
        );
    }

    let target_sequence = harness.catch_up().await.expect("追平 Graph 投影");
    let seed_fragments = std::iter::once(target.fragment_id)
        .chain(mutations.iter().map(|event| event.fragment_id))
        .collect::<Vec<_>>();
    let allowed = cases.map(|(_, relation, _)| relation);
    let before_restart = harness
        .projection
        .load_graph(&harness.scope, &seed_fragments, &allowed, time(100), 20)
        .await
        .expect("读取持久化 Graph");

    assert_projected_mutations(&before_restart, &target, &mutations);

    let state_checkpoint = harness
        .projection
        .checkpoint(&harness.scope, ProjectorKind::State)
        .await
        .unwrap();
    assert_eq!(state_checkpoint.sequence, 0);
    assert_eq!(
        harness
            .projection
            .checkpoint(&harness.scope, ProjectorKind::Graph)
            .await
            .unwrap()
            .sequence,
        target_sequence
    );

    let reopened = harness.reopen().await;
    let after_restart = reopened
        .projection
        .load_graph(&reopened.scope, &seed_fragments, &allowed, time(100), 20)
        .await
        .expect("重启后读取持久化 Graph");
    assert_eq!(after_restart, before_restart);
}

#[tokio::test]
async fn mutation_graph_skips_relation_when_target_cannot_form_a_point() {
    let harness = Harness::new().await;
    let target = harness
        .append(point_fragment(
            harness.scope.clone(),
            FragmentKind::Observation,
            "仅作为权威观察保留",
            time(10),
        ))
        .await;
    let mutation = harness
        .append(mutation_fragment(
            harness.scope.clone(),
            target.fragment_id,
            "correct",
            EdgeKind::Supersedes,
            VerificationMethod::Manual,
            time(20),
        ))
        .await;

    harness
        .catch_up()
        .await
        .expect("追平非 Point 目标的 Graph 投影");
    let graph = harness
        .load(
            &[mutation.fragment_id],
            &[EdgeKind::Supersedes],
            time(100),
            10,
        )
        .await;

    assert_eq!(graph.points.len(), 1);
    assert_eq!(graph.points[0].point.id, point_id(mutation.fragment_id));
    assert!(graph.edges.is_empty());
}

fn assert_projected_mutations(
    graph: &jc_memory_storage::ProjectedGraph,
    target: &EventDraft,
    mutations: &[EventDraft],
) {
    assert_eq!(graph.points.len(), 6);
    assert_eq!(graph.edges.len(), 5);
    assert_eq!(graph.points[0].point.id, point_id(target.fragment_id));
    assert_eq!(graph.points[0].text, "生产数据库使用 SQLite");
    assert_eq!(graph.points[0].entities, vec!["SQLite"]);
    for mutation in mutations {
        assert!(
            graph
                .points
                .iter()
                .any(|item| item.point.id == point_id(mutation.fragment_id))
        );
        assert!(
            graph
                .edges
                .iter()
                .any(|edge| edge.id == edge_id(mutation.id))
        );
    }
    assert_eq!(
        graph
            .edges
            .iter()
            .filter(|edge| edge.human_confirmed)
            .count(),
        2
    );
    assert_eq!(
        graph.edges.iter().filter(|edge| edge.tool_verified).count(),
        1
    );
    assert!(
        graph
            .edges
            .iter()
            .all(|edge| edge.source_point_id != edge.target_point_id)
    );
}

#[tokio::test]
async fn graph_projection_rejects_cross_scope_target_atomically() {
    let directory = Arc::new(tempfile::tempdir().expect("创建跨 Scope Graph 测试目录"));
    let owner = Harness::new_in(Arc::clone(&directory), MemoryScope::new(NamespaceId::new())).await;
    let target = owner
        .append(point_fragment(
            owner.scope.clone(),
            FragmentKind::Fact,
            "Scope A 的事实",
            time(10),
        ))
        .await;
    owner.catch_up().await.expect("投影 Scope A");

    let foreign = Harness::new_in(directory, MemoryScope::new(NamespaceId::new())).await;
    foreign
        .append(mutation_fragment(
            foreign.scope.clone(),
            target.fragment_id,
            "contradict",
            EdgeKind::Contradicts,
            VerificationMethod::Manual,
            time(20),
        ))
        .await;
    let error = foreign
        .catch_up()
        .await
        .expect_err("拒绝跨 Scope Graph 端点");

    assert!(matches!(
        error,
        StorageError::ProjectionScopeMismatch {
            projector: ProjectorKind::Graph,
            ..
        }
    ));
    assert_eq!(foreign.count("points").await, 1);
    assert_eq!(foreign.count("edges").await, 0);
    assert_eq!(
        foreign
            .projection
            .checkpoint(&foreign.scope, ProjectorKind::Graph)
            .await
            .unwrap()
            .sequence,
        0
    );
}

#[tokio::test]
async fn graph_loading_binds_seed_kind_time_and_node_budget() {
    let harness = Harness::new().await;
    let target = harness
        .append(point_fragment(
            harness.scope.clone(),
            FragmentKind::Fact,
            "稳定目标",
            time(10),
        ))
        .await;
    harness
        .append(mutation_fragment(
            harness.scope.clone(),
            target.fragment_id,
            "reinforce",
            EdgeKind::Supports,
            VerificationMethod::Tool,
            time(20),
        ))
        .await;
    harness
        .append(mutation_fragment(
            harness.scope.clone(),
            target.fragment_id,
            "contradict",
            EdgeKind::Contradicts,
            VerificationMethod::Manual,
            time(30),
        ))
        .await;
    harness.catch_up().await.expect("投影有界加载 Fixture");

    let support = harness
        .load(&[target.fragment_id], &[EdgeKind::Supports], time(25), 10)
        .await;
    assert_eq!(support.edges.len(), 1);
    assert_eq!(support.edges[0].kind, EdgeKind::Supports);
    assert_eq!(support.points.len(), 2);

    let before_edges = harness
        .load(
            &[target.fragment_id],
            &[EdgeKind::Supports, EdgeKind::Contradicts],
            time(15),
            10,
        )
        .await;
    assert_eq!(before_edges.points.len(), 1);
    assert!(before_edges.edges.is_empty());

    let bounded = harness
        .load(
            &[target.fragment_id],
            &[EdgeKind::Supports, EdgeKind::Contradicts],
            time(100),
            2,
        )
        .await;
    assert_eq!(bounded.points.len(), 2);
    assert_eq!(bounded.edges.len(), 1);
    assert!(bounded.points.len() <= 2);

    let no_seed = harness
        .load(&[FragmentId::new()], &[EdgeKind::Supports], time(100), 10)
        .await;
    assert!(no_seed.points.is_empty());
    assert!(no_seed.edges.is_empty());
}

#[tokio::test]
async fn co_occurrence_does_not_invent_causal_edges() {
    let harness = Harness::new().await;
    let first = harness
        .append(point_fragment(
            harness.scope.clone(),
            FragmentKind::Fact,
            "同一批次事实一",
            time(10),
        ))
        .await;
    let second = harness
        .append(point_fragment(
            harness.scope.clone(),
            FragmentKind::Fact,
            "同一批次事实二",
            time(10),
        ))
        .await;
    harness.catch_up().await.expect("投影共同出现的事实");

    let graph = harness
        .load(
            &[first.fragment_id, second.fragment_id],
            &[EdgeKind::Causes],
            time(100),
            10,
        )
        .await;
    assert_eq!(graph.points.len(), 2);
    assert!(graph.edges.is_empty());
    assert_eq!(harness.count("edges").await, 0);
}

#[tokio::test]
async fn graph_point_accepts_authoritative_nanosecond_time_at_sqlite_precision() {
    let harness = Harness::new().await;
    let occurred_at = DateTime::from_timestamp(10, 123_456_789).expect("构造纳秒时间");
    let event = harness
        .append(point_fragment(
            harness.scope.clone(),
            FragmentKind::Fact,
            "保留权威纳秒时间",
            occurred_at,
        ))
        .await;
    harness.catch_up().await.expect("投影纳秒时间 Point");

    let graph = harness
        .load(&[event.fragment_id], &[EdgeKind::Supports], time(100), 1)
        .await;
    assert_eq!(graph.points.len(), 1);
    assert_eq!(
        graph.points[0].point.occurred_at.timestamp_micros(),
        occurred_at.timestamp_micros()
    );
}

#[tokio::test]
async fn graph_edge_loads_the_evidence_ref_that_proves_its_target() {
    let harness = Harness::new().await;
    let unrelated = harness
        .append(point_fragment(
            harness.scope.clone(),
            FragmentKind::Fact,
            "无关证据",
            time(5),
        ))
        .await;
    let target = harness
        .append(point_fragment(
            harness.scope.clone(),
            FragmentKind::Fact,
            "关系目标",
            time(10),
        ))
        .await;
    let unrelated_evidence = evidence_id("00000000-0000-7000-8000-000000000001");
    let target_evidence = evidence_id("00000000-0000-7000-8000-000000000002");
    let mut mutation = mutation_fragment(
        harness.scope.clone(),
        target.fragment_id,
        "contradict",
        EdgeKind::Contradicts,
        VerificationMethod::Manual,
        time(20),
    );
    mutation.evidence = vec![
        EvidenceRef {
            id: unrelated_evidence,
            fragment_id: Some(unrelated.fragment_id),
            event_id: None,
            method: VerificationMethod::Tool,
            note: Some("不得绑定到 Edge 的无关证据".to_owned()),
        },
        EvidenceRef {
            id: target_evidence,
            fragment_id: Some(target.fragment_id),
            event_id: None,
            method: VerificationMethod::Manual,
            note: Some("证明 Edge 目标的证据".to_owned()),
        },
    ];
    harness.append(mutation).await;
    harness.catch_up().await.expect("投影多证据 Mutation");

    let graph = harness
        .load(
            &[target.fragment_id],
            &[EdgeKind::Contradicts],
            time(100),
            3,
        )
        .await;
    assert_eq!(graph.edges.len(), 1);
    assert_eq!(
        graph.edges[0].evidence[0].evidence_ref_id,
        Some(target_evidence)
    );
    assert!(graph.edges[0].human_confirmed);
    assert!(!graph.edges[0].tool_verified);
}

struct Harness {
    directory: Arc<TempDir>,
    store: SqliteStore,
    repository: SqliteMemoryRepository,
    projection: SqliteProjectionStore,
    scope: MemoryScope,
}

impl Harness {
    async fn new() -> Self {
        let directory = Arc::new(tempfile::tempdir().expect("创建 Graph Projector 测试目录"));
        Self::new_in(directory, MemoryScope::new(NamespaceId::new())).await
    }

    async fn new_in(directory: Arc<TempDir>, scope: MemoryScope) -> Self {
        let store = SqliteStore::open(
            &SqliteConfig::new(directory.path().join("memory.sqlite3")).with_max_connections(8),
        )
        .await
        .expect("打开 Graph Projector 测试数据库");
        let repository = SqliteMemoryRepository::from_store(&store);
        let projection = SqliteProjectionStore::from_store(&store);
        Self {
            directory,
            store,
            repository,
            projection,
            scope,
        }
    }

    async fn reopen(self) -> Self {
        let directory = Arc::clone(&self.directory);
        let scope = self.scope.clone();
        self.store.pool().close().await;
        drop(self);
        Self::new_in(directory, scope).await
    }

    async fn append(&self, fragment: CognitiveFragment) -> EventDraft {
        let draft = EventFactory::new()
            .from_fragment(fragment)
            .expect("创建 Graph Event Draft");
        self.repository
            .append(&draft)
            .await
            .expect("追加 Graph Event");
        draft
    }

    async fn catch_up(&self) -> Result<u64, StorageError> {
        let target = self.projection.target(&self.scope).await?;
        let checkpoint = self
            .projection
            .catch_up_graph(&self.scope, target, 2)
            .await?;
        Ok(checkpoint.sequence)
    }

    async fn load(
        &self,
        seeds: &[FragmentId],
        allowed_kinds: &[EdgeKind],
        effective_at: DateTime<Utc>,
        max_nodes: usize,
    ) -> jc_memory_storage::ProjectedGraph {
        self.projection
            .load_graph(&self.scope, seeds, allowed_kinds, effective_at, max_nodes)
            .await
            .expect("有界读取 Graph")
    }

    async fn count(&self, table: &str) -> i64 {
        let statement = format!("SELECT COUNT(*) FROM {table}");
        sqlx::query_scalar(sqlx::AssertSqlSafe(statement))
            .fetch_one(self.store.pool())
            .await
            .expect("读取 Graph 投影表行数")
    }
}

fn point_fragment(
    scope: MemoryScope,
    kind: FragmentKind,
    text: &str,
    occurred_at: DateTime<Utc>,
) -> CognitiveFragment {
    let mut fragment = base_fragment(scope, kind, text, occurred_at);
    fragment
        .metadata
        .insert("entities".to_owned(), serde_json::json!(["SQLite"]));
    fragment
}

fn mutation_fragment(
    scope: MemoryScope,
    target: FragmentId,
    mutation: &str,
    relation: EdgeKind,
    method: VerificationMethod,
    occurred_at: DateTime<Utc>,
) -> CognitiveFragment {
    let mut fragment = base_fragment(
        scope,
        FragmentKind::Correction,
        &format!("{mutation} 目标记忆"),
        occurred_at,
    );
    fragment.validity = match mutation {
        "weaken" | "contradict" => ValidityStatus::Disputed,
        _ => ValidityStatus::Verified,
    };
    fragment.metadata.insert(
        "mutation".to_owned(),
        serde_json::Value::String(mutation.to_owned()),
    );
    fragment.metadata.insert(
        "target_fragment_id".to_owned(),
        serde_json::Value::String(target.to_string()),
    );
    fragment.metadata.insert(
        "relation".to_owned(),
        serde_json::to_value(relation).expect("序列化 EdgeKind"),
    );
    fragment.evidence.push(EvidenceRef {
        id: EvidenceId::new(),
        fragment_id: Some(target),
        event_id: None,
        method,
        note: Some("Graph Mutation 测试证据".to_owned()),
    });
    fragment
}

fn base_fragment(
    scope: MemoryScope,
    kind: FragmentKind,
    text: &str,
    occurred_at: DateTime<Utc>,
) -> CognitiveFragment {
    CognitiveFragment {
        id: FragmentId::new(),
        scope,
        session_id: None,
        trace_id: None,
        kind,
        content: FragmentContent::Text {
            text: text.to_owned(),
        },
        source: FragmentSource {
            source_type: SourceType::User,
            source_id: "graph-projector-test".to_owned(),
            actor_id: Some("operator".to_owned()),
            observer_id: None,
            tool_id: None,
            uri: None,
        },
        perspective: Perspective::FirstPerson,
        occurred_at,
        observed_at: occurred_at,
        confidence: 0.9,
        importance: 0.8,
        validity: ValidityStatus::Unverified,
        security_label: SecurityLabel::Internal,
        evidence: Vec::new(),
        metadata: serde_json::Map::new(),
    }
}

fn point_id(fragment_id: FragmentId) -> PointId {
    PointId::from_str(&fragment_id.to_string()).expect("FragmentId 转 PointId")
}

fn edge_id(event_id: jc_memory_protocol::EventId) -> EdgeId {
    EdgeId::from_str(&event_id.to_string()).expect("EventId 转 EdgeId")
}

fn evidence_id(raw: &str) -> EvidenceId {
    EvidenceId::from_str(raw).expect("解析 EvidenceId Fixture")
}

fn time(seconds: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(seconds, 0).expect("构造 Graph 测试时间")
}
