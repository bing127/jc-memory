//! 事务性 State Projector 与有界读取契约。

use std::{str::FromStr as _, sync::Arc};

use chrono::{DateTime, Utc};
use jc_memory_events::factory::EventFactory;
use jc_memory_protocol::{
    CognitiveFragment, EventDraft, EvidenceId, EvidenceRef, FragmentContent, FragmentId,
    FragmentKind, FragmentSource, MemoryScope, NamespaceId, Perspective, SecurityLabel, SourceType,
    StateDeclaration, StateId, StateVersionId, ValidityStatus, VerificationMethod,
};
use jc_memory_storage::{
    MemoryRepository, ProjectionFailpoint, ProjectorKind, SqliteConfig, SqliteMemoryRepository,
    SqliteProjectionStore, SqliteStore, StateSelector, StorageError,
};
use tempfile::TempDir;

#[tokio::test]
async fn state_versions_advance_idempotently_and_load_current_history() {
    let harness = Harness::new().await;
    let blue_green = harness
        .append_state("project.deployment_strategy", "blue_green", time(10))
        .await;
    let canary = harness
        .append_state("project.deployment_strategy", "canary", time(20))
        .await;
    let target = harness.projection.target(&harness.scope).await.unwrap();

    let checkpoint = harness
        .projection
        .catch_up_state(&harness.scope, target, 100)
        .await
        .expect("追平 State 投影");
    assert_eq!(checkpoint.sequence, target.sequence);

    let states = harness
        .projection
        .load_states(
            &harness.scope,
            StateSelector::ExactKey("project.deployment_strategy".to_owned()),
            time(30),
            10,
        )
        .await
        .expect("读取当前与历史状态");
    assert_eq!(states.current.len(), 1);
    assert_eq!(states.current[0].value, serde_json::json!("canary"));
    assert_eq!(states.current[0].version_id, version_id(canary.fragment_id));
    assert_eq!(states.current[0].source_event_id, Some(canary.id));
    assert_eq!(states.history.len(), 1);
    assert_eq!(states.history[0].value, serde_json::json!("blue_green"));
    assert_eq!(states.history[0].valid_to, Some(time(20)));
    assert_eq!(states.history[0].validity, ValidityStatus::Superseded);
    assert_eq!(states.history[0].state_id, state_id(blue_green.fragment_id));
    assert_eq!(states.evidence.len(), 2);
    assert!(!states.truncated);

    harness
        .projection
        .catch_up_state(&harness.scope, target, 1)
        .await
        .expect("重复追平保持幂等");
    assert_eq!(harness.count("state_versions").await, 2);
}

#[tokio::test]
async fn same_time_different_values_create_visible_conflict_without_current_pointer() {
    let harness = Harness::new().await;
    harness
        .append_state("project.deployment_strategy", "blue_green", time(10))
        .await;
    harness
        .append_state("project.deployment_strategy", "canary", time(10))
        .await;
    harness.catch_up().await.expect("追平同刻冲突");

    let pointer: Option<String> = sqlx::query_scalar(
        "SELECT current_version_id FROM states WHERE state_key = 'project.deployment_strategy'",
    )
    .fetch_one(harness.store.pool())
    .await
    .expect("读取当前版本指针");
    assert_eq!(pointer, None);
    let validity: Vec<String> =
        sqlx::query_scalar("SELECT validity FROM state_versions ORDER BY id")
            .fetch_all(harness.store.pool())
            .await
            .expect("读取冲突版本有效性");
    assert_eq!(validity, vec!["disputed", "disputed"]);

    let states = harness.load("project.deployment_strategy").await.unwrap();
    assert_eq!(states.current.len(), 2);
    assert_eq!(states.conflicts.len(), 1);
    assert_eq!(states.conflicts[0].point_ids.len(), 2);
}

#[tokio::test]
async fn source_fragment_selector_and_zero_node_limit_are_hard_bounded() {
    let harness = Harness::new().await;
    let deployment = harness
        .append_state("project.deployment_strategy", "blue_green", time(10))
        .await;
    harness
        .append_state("project.database", "sqlite", time(20))
        .await;
    harness.catch_up().await.expect("投影两个独立 State key");

    let selected = harness
        .projection
        .load_states(
            &harness.scope,
            StateSelector::SourceFragments(vec![deployment.fragment_id]),
            time(30),
            1,
        )
        .await
        .expect("按来源 Fragment 读取 State");
    assert_eq!(selected.current.len(), 1);
    assert_eq!(selected.current[0].key, "project.deployment_strategy");
    assert!(!selected.truncated);

    let bounded = harness
        .projection
        .load_states(
            &harness.scope,
            StateSelector::ExactKey("project.deployment_strategy".to_owned()),
            time(30),
            0,
        )
        .await
        .expect("零节点预算返回截断结果");
    assert!(bounded.current.is_empty());
    assert!(bounded.history.is_empty());
    assert!(bounded.truncated);
}

#[tokio::test]
async fn forget_weaken_and_contradict_change_only_derived_state() {
    assert_mutation_result("forget", None, "invalidated", 1, false).await;
    assert_mutation_result("weaken", None, "disputed", 1, false).await;
    assert_mutation_result(
        "contradict",
        Some(("project.deployment_strategy", "canary")),
        "disputed",
        2,
        true,
    )
    .await;
}

#[tokio::test]
async fn state_mutation_rejects_key_and_scope_mismatch_without_partial_rows() {
    let harness = Harness::new().await;
    let target = harness
        .append_state("project.deployment_strategy", "blue_green", time(10))
        .await;
    harness.catch_up().await.expect("投影初始状态");
    let checkpoint = harness.checkpoint().await;
    harness
        .append_mutation(
            "correct",
            target.fragment_id,
            Some(("project.database", "sqlite")),
            time(20),
        )
        .await;
    let error = harness.catch_up().await.expect_err("拒绝不同 State key");
    assert!(matches!(
        error,
        StorageError::ProjectionEventCorrupt {
            field: "state.key",
            ..
        }
    ));
    assert_eq!(harness.count("state_versions").await, 1);
    assert_eq!(harness.checkpoint().await, checkpoint);

    let foreign = Harness::new_in(
        Arc::clone(&harness.directory),
        MemoryScope::new(NamespaceId::new()),
    )
    .await;
    foreign
        .append_mutation(
            "correct",
            target.fragment_id,
            Some(("project.deployment_strategy", "canary")),
            time(30),
        )
        .await;
    let error = foreign
        .catch_up()
        .await
        .expect_err("拒绝跨 Scope State target");
    assert!(matches!(
        error,
        StorageError::ProjectionScopeMismatch {
            projector: ProjectorKind::State,
            ..
        }
    ));
    assert_eq!(harness.count("state_versions").await, 1);
}

#[tokio::test]
async fn failpoint_rolls_back_state_rows_and_checkpoint() {
    let harness = Harness::new().await;
    harness
        .append_state("project.deployment_strategy", "blue_green", time(10))
        .await;
    let projection =
        SqliteProjectionStore::from_store(&harness.store).with_failpoint(Arc::new(AlwaysFail));
    let target = projection.target(&harness.scope).await.unwrap();

    let error = projection
        .catch_up_state(&harness.scope, target, 100)
        .await
        .expect_err("注入提交前失败");
    assert!(matches!(
        error,
        StorageError::InjectedFailure {
            point: "state_before_commit"
        }
    ));
    assert_eq!(harness.count("states").await, 0);
    assert_eq!(harness.count("state_versions").await, 0);
    assert_eq!(harness.checkpoint().await.sequence, 0);
}

#[tokio::test]
async fn quarantined_mutation_remains_authoritative_without_changing_state_projection() {
    let harness = Harness::new().await;
    let target = harness
        .append_state("project.deployment_strategy", "blue_green", time(10))
        .await;
    harness.catch_up().await.expect("投影隔离测试初始状态");
    let mut quarantined = fragment(
        harness.scope.clone(),
        FragmentKind::Correction,
        ValidityStatus::Quarantined,
        time(20),
        Some(StateDeclaration {
            key: "project.deployment_strategy".to_owned(),
            value: serde_json::json!("canary"),
            reason: "隔离内容不得进入投影".to_owned(),
        }),
        Some(("contradict", target.fragment_id)),
    );
    quarantined.content = FragmentContent::Text {
        text: "需要审查的冲突声明".to_owned(),
    };
    harness.append(quarantined).await;

    harness
        .catch_up()
        .await
        .expect("隔离 Event 允许 Checkpoint 前进");
    assert_eq!(harness.count("fragments").await, 2);
    assert_eq!(harness.count("events").await, 2);
    assert_eq!(harness.count("state_versions").await, 1);
    let states = harness.load("project.deployment_strategy").await.unwrap();
    assert_eq!(states.current[0].value, serde_json::json!("blue_green"));
    assert!(states.conflicts.is_empty());
}

#[tokio::test]
async fn corrupt_current_pointer_and_multiple_active_versions_fail_closed() {
    let harness = Harness::new().await;
    let source = harness
        .append_state("project.deployment_strategy", "blue_green", time(10))
        .await;
    harness.catch_up().await.expect("投影初始状态");
    sqlx::query("UPDATE state_versions SET valid_to_us = ?1 WHERE id = ?2")
        .bind(time(20).timestamp_micros())
        .bind(source.fragment_id.to_string())
        .execute(harness.store.pool())
        .await
        .expect("构造指向已关闭版本的损坏指针");
    let error = harness
        .load("project.deployment_strategy")
        .await
        .expect_err("损坏链必须 fail closed");
    assert!(matches!(error, StorageError::StateChainCorrupt { .. }));
    assert!(!error.to_string().contains("blue_green"));

    sqlx::query("UPDATE state_versions SET valid_to_us = NULL WHERE id = ?1")
        .bind(source.fragment_id.to_string())
        .execute(harness.store.pool())
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO state_versions (
            id, state_id, value_json, valid_from_us, valid_to_us, validity,
            source_fragment_id, previous_version_id, created_at_us, reason, source_event_id
         ) SELECT ?1, state_id, '\"shadow\"', valid_from_us + 1, NULL, 'verified',
                  source_fragment_id, id, created_at_us + 1, '损坏 Fixture', source_event_id
           FROM state_versions WHERE id = ?2",
    )
    .bind(FragmentId::new().to_string())
    .bind(source.fragment_id.to_string())
    .execute(harness.store.pool())
    .await
    .expect("构造多个活跃版本");
    let error = harness
        .load("project.deployment_strategy")
        .await
        .expect_err("多个活跃版本必须 fail closed");
    assert!(matches!(error, StorageError::StateChainCorrupt { .. }));
    assert!(!error.to_string().contains("shadow"));
}

#[tokio::test]
async fn concurrent_projectors_serialize_without_duplicate_versions() {
    let harness = Harness::new().await;
    harness
        .append_state("project.deployment_strategy", "blue_green", time(10))
        .await;
    harness
        .append_state("project.deployment_strategy", "canary", time(20))
        .await;
    let first = SqliteProjectionStore::from_store(&harness.store);
    let second = SqliteProjectionStore::from_store(&harness.store);
    let target = first.target(&harness.scope).await.unwrap();
    let (left, right) = tokio::join!(
        first.catch_up_state(&harness.scope, target, 1),
        second.catch_up_state(&harness.scope, target, 1)
    );
    assert_eq!(left.expect("第一个 Projector").sequence, target.sequence);
    assert_eq!(right.expect("第二个 Projector").sequence, target.sequence);
    assert_eq!(harness.count("state_versions").await, 2);
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
        let directory = Arc::new(tempfile::tempdir().expect("创建 State Projector 测试目录"));
        Self::new_in(directory, MemoryScope::new(NamespaceId::new())).await
    }

    async fn new_in(directory: Arc<TempDir>, scope: MemoryScope) -> Self {
        let store = SqliteStore::open(
            &SqliteConfig::new(directory.path().join("memory.sqlite3")).with_max_connections(8),
        )
        .await
        .expect("打开 State Projector 测试数据库");
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

    async fn append_state(&self, key: &str, value: &str, occurred_at: DateTime<Utc>) -> EventDraft {
        let fragment = fragment(
            self.scope.clone(),
            FragmentKind::Fact,
            ValidityStatus::Unverified,
            occurred_at,
            Some(StateDeclaration {
                key: key.to_owned(),
                value: serde_json::json!(value),
                reason: format!("将 {key} 更新为 {value}"),
            }),
            None,
        );
        self.append(fragment).await
    }

    async fn append_mutation(
        &self,
        mutation: &str,
        target: FragmentId,
        state: Option<(&str, &str)>,
        occurred_at: DateTime<Utc>,
    ) -> EventDraft {
        let validity = match mutation {
            "forget" => ValidityStatus::Invalidated,
            "weaken" | "contradict" => ValidityStatus::Disputed,
            _ => ValidityStatus::Verified,
        };
        let state = state.map(|(key, value)| StateDeclaration {
            key: key.to_owned(),
            value: serde_json::json!(value),
            reason: format!("{mutation} 后更新状态"),
        });
        let fragment = fragment(
            self.scope.clone(),
            FragmentKind::Correction,
            validity,
            occurred_at,
            state,
            Some((mutation, target)),
        );
        self.append(fragment).await
    }

    async fn append(&self, fragment: CognitiveFragment) -> EventDraft {
        let draft = EventFactory::new()
            .from_fragment(fragment)
            .expect("创建 State Event Draft");
        self.repository
            .append(&draft)
            .await
            .expect("追加 State Event");
        draft
    }

    async fn catch_up(&self) -> Result<(), StorageError> {
        let target = self.projection.target(&self.scope).await?;
        self.projection
            .catch_up_state(&self.scope, target, 100)
            .await?;
        Ok(())
    }

    async fn load(&self, key: &str) -> Result<jc_memory_storage::ProjectedStateSet, StorageError> {
        self.projection
            .load_states(
                &self.scope,
                StateSelector::ExactKey(key.to_owned()),
                time(100),
                100,
            )
            .await
    }

    async fn checkpoint(&self) -> jc_memory_storage::ProjectionCheckpoint {
        self.projection
            .checkpoint(&self.scope, ProjectorKind::State)
            .await
            .expect("读取 State Checkpoint")
    }

    async fn count(&self, table: &str) -> i64 {
        let statement = format!("SELECT COUNT(*) FROM {table}");
        sqlx::query_scalar(sqlx::AssertSqlSafe(statement))
            .fetch_one(self.store.pool())
            .await
            .expect("读取投影表行数")
    }
}

#[derive(Debug)]
struct AlwaysFail;

impl ProjectionFailpoint for AlwaysFail {
    fn before_commit(
        &self,
        projector: ProjectorKind,
        _next_sequence: u64,
    ) -> Result<(), StorageError> {
        assert_eq!(projector, ProjectorKind::State);
        Err(StorageError::InjectedFailure {
            point: "state_before_commit",
        })
    }
}

async fn assert_mutation_result(
    mutation: &str,
    state: Option<(&str, &str)>,
    expected_validity: &str,
    expected_versions: i64,
    expect_conflict: bool,
) {
    let harness = Harness::new().await;
    let target = harness
        .append_state("project.deployment_strategy", "blue_green", time(10))
        .await;
    harness.catch_up().await.expect("投影 Mutation 前状态");
    harness
        .append_mutation(mutation, target.fragment_id, state, time(20))
        .await;
    harness.catch_up().await.expect("投影 Mutation");

    let validity: String =
        sqlx::query_scalar("SELECT validity FROM state_versions WHERE source_fragment_id = ?1")
            .bind(target.fragment_id.to_string())
            .fetch_one(harness.store.pool())
            .await
            .expect("读取目标状态有效性");
    assert_eq!(validity, expected_validity);
    assert_eq!(harness.count("state_versions").await, expected_versions);
    let states = harness.load("project.deployment_strategy").await.unwrap();
    assert_eq!(!states.conflicts.is_empty(), expect_conflict);
    assert_eq!(harness.count("fragments").await, 2);
    assert_eq!(harness.count("events").await, 2);
}

fn fragment(
    scope: MemoryScope,
    kind: FragmentKind,
    validity: ValidityStatus,
    occurred_at: DateTime<Utc>,
    state: Option<StateDeclaration>,
    mutation: Option<(&str, FragmentId)>,
) -> CognitiveFragment {
    let mut metadata = serde_json::Map::new();
    if let Some(state) = state {
        metadata.insert(
            "state".to_owned(),
            serde_json::to_value(state).expect("序列化 State metadata"),
        );
    }
    let mut evidence = Vec::new();
    if let Some((kind, target)) = mutation {
        metadata.insert("mutation".to_owned(), serde_json::json!(kind));
        metadata.insert(
            "target_fragment_id".to_owned(),
            serde_json::json!(target.to_string()),
        );
        evidence.push(EvidenceRef {
            id: EvidenceId::new(),
            fragment_id: Some(target),
            event_id: None,
            method: VerificationMethod::Manual,
            note: Some("Mutation 测试证据".to_owned()),
        });
    }
    CognitiveFragment {
        id: FragmentId::new(),
        scope,
        session_id: None,
        trace_id: None,
        kind,
        content: FragmentContent::Text {
            text: "权威 State Fragment".to_owned(),
        },
        source: FragmentSource {
            source_type: SourceType::User,
            source_id: "state-projector-test".to_owned(),
            actor_id: Some("operator".to_owned()),
            observer_id: None,
            tool_id: None,
            uri: None,
        },
        perspective: Perspective::FirstPerson,
        occurred_at,
        observed_at: occurred_at,
        confidence: 1.0,
        importance: 0.8,
        validity,
        security_label: SecurityLabel::Internal,
        evidence,
        metadata,
    }
}

fn state_id(fragment_id: FragmentId) -> StateId {
    StateId::from_str(&fragment_id.to_string()).expect("FragmentId 转 StateId")
}

fn version_id(fragment_id: FragmentId) -> StateVersionId {
    StateVersionId::from_str(&fragment_id.to_string()).expect("FragmentId 转 StateVersionId")
}

fn time(seconds: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(seconds, 0).expect("构造 State 测试时间")
}
