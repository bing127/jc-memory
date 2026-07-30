//! Runtime 状态转换与 `SQLite` 恢复契约测试。

use chrono::{TimeZone as _, Utc};
use jc_memory_events::factory::EventFactory;
use jc_memory_protocol::{
    AccessContext, AgentId, CognitiveFragment, DenyReason, EventId, EvidenceId, EvidenceRef,
    FragmentContent, FragmentId, FragmentKind, FragmentSource, Grant, MemoryScope, NamespaceId,
    Operation, Perspective, ProjectId, SecurityLabel, SourceType, TenantId, TraceId, UserId,
    ValidityStatus, VerificationMethod,
};
use jc_memory_runtime::{
    RunId, RuntimeError, RuntimeEvent, RuntimeRecovery, RuntimeRunner, RuntimeState,
    TransitionError, TransitionMetadata,
};
use jc_memory_storage::{MemoryRepository, SqliteConfig, SqliteMemoryRepository, SqliteStore};
use tempfile::TempDir;

#[test]
fn executing_cannot_skip_commit_to_completed() {
    assert_eq!(
        RuntimeState::Executing.transition(RuntimeEvent::Complete),
        Err(TransitionError::Invalid)
    );
}

#[test]
fn legal_path_visits_commit_before_completion() {
    let path = [
        (RuntimeEvent::Recall, RuntimeState::Recalling),
        (RuntimeEvent::Reason, RuntimeState::Reasoning),
        (
            RuntimeEvent::RequestApproval,
            RuntimeState::AwaitingApproval,
        ),
        (RuntimeEvent::Approve, RuntimeState::Executing),
        (RuntimeEvent::Commit, RuntimeState::Committing),
        (RuntimeEvent::Complete, RuntimeState::Completed),
    ];
    let mut state = RuntimeState::Observing;
    for (event, expected) in path {
        state = state.transition(event).expect("合法转换应成功");
        assert_eq!(state, expected);
    }
}

#[test]
fn illegal_jump_is_rejected() {
    assert_eq!(
        RuntimeState::Observing.transition(RuntimeEvent::Execute),
        Err(TransitionError::Invalid)
    );
}

#[test]
fn terminal_states_reject_every_event() {
    let events = [
        RuntimeEvent::Recall,
        RuntimeEvent::Reason,
        RuntimeEvent::RequestApproval,
        RuntimeEvent::Approve,
        RuntimeEvent::Execute,
        RuntimeEvent::Commit,
        RuntimeEvent::WaitForUser,
        RuntimeEvent::Complete,
        RuntimeEvent::Fail,
        RuntimeEvent::Cancel,
    ];
    for state in [
        RuntimeState::Completed,
        RuntimeState::Failed,
        RuntimeState::Cancelled,
    ] {
        for event in events {
            assert_eq!(
                state.transition(event),
                Err(TransitionError::Invalid),
                "终态 {state:?} 不得处理 {event:?}"
            );
        }
    }
}

#[tokio::test]
async fn reopening_recovers_last_committed_state_and_next_step() {
    let harness = Harness::new().await;
    let run = harness
        .runner
        .start(&harness.access, &harness.scope)
        .await
        .expect("创建 Runtime run");
    let run_id = run.run_id;
    let recalled = harness
        .runner
        .transition(
            &harness.access,
            &run,
            RuntimeEvent::Recall,
            &harness.metadata(1),
        )
        .await
        .expect("持久化 Recall 转换");
    assert_eq!(recalled.state, RuntimeState::Recalling);
    assert_eq!(recalled.next_step, 1);
    harness.store.pool().close().await;

    let reopened = SqliteStore::open(&SqliteConfig::new(&harness.path))
        .await
        .expect("重开 SQLite");
    let recovery = RuntimeRecovery::from_store(&reopened);
    let recovered = recovery
        .recover(&harness.access, run_id)
        .await
        .expect("恢复 Runtime run")
        .expect("run 应存在");
    assert_eq!(recovered.state, RuntimeState::Recalling);
    assert_eq!(recovered.next_step, 1);
    assert_eq!(recovered.scope, harness.scope);

    let reasoned = RuntimeRunner::from_store(&reopened)
        .transition(
            &harness.access,
            &recovered,
            RuntimeEvent::Reason,
            &harness.metadata(2),
        )
        .await
        .expect("从恢复点继续");
    assert_eq!(reasoned.state, RuntimeState::Reasoning);
    assert_eq!(reasoned.next_step, 2);
}

#[tokio::test]
async fn transition_persists_complete_runtime_fields() {
    let harness = Harness::new().await;
    let run = harness
        .runner
        .start(&harness.access, &harness.scope)
        .await
        .unwrap();
    let metadata = harness.metadata(3);
    harness
        .runner
        .transition(&harness.access, &run, RuntimeEvent::Recall, &metadata)
        .await
        .unwrap();

    let row: (
        String,
        i64,
        String,
        String,
        String,
        String,
        i64,
        i64,
        String,
        i64,
    ) = sqlx::query_as(
        "SELECT run_id, step, from_stage, to_stage, input_refs_json,
                    output_refs_json, attempt, deadline_at_us, trace_id, created_at_us
             FROM transitions WHERE run_id = ?1",
    )
    .bind(run.run_id.to_string())
    .fetch_one(harness.store.pool())
    .await
    .expect("读取 Runtime transition");
    assert_eq!(row.0, run.run_id.to_string());
    assert_eq!(row.1, 0);
    assert_eq!(row.2, "observing");
    assert_eq!(row.3, "recalling");
    assert_eq!(
        serde_json::from_str::<Vec<EvidenceRef>>(&row.4).unwrap(),
        metadata.input_refs
    );
    assert_eq!(
        serde_json::from_str::<Vec<EvidenceRef>>(&row.5).unwrap(),
        metadata.output_refs
    );
    assert_eq!(row.6, i64::from(metadata.attempt));
    assert_eq!(row.7, metadata.deadline.timestamp_micros());
    assert_eq!(row.8, metadata.trace_id.to_string());
    assert!(row.9 > 0);

    let payload: serde_json::Value =
        sqlx::query_scalar("SELECT payload_json FROM transitions WHERE run_id = ?1 AND step = 0")
            .bind(run.run_id.to_string())
            .fetch_one(harness.store.pool())
            .await
            .unwrap();
    assert_eq!(payload, metadata.payload);
}

#[tokio::test]
async fn failed_transaction_leaves_no_partial_transition_or_state_advance() {
    let harness = Harness::new().await;
    let run = harness
        .runner
        .start(&harness.access, &harness.scope)
        .await
        .unwrap();
    sqlx::query(
        "CREATE TRIGGER reject_runtime_state_update
         BEFORE UPDATE ON runtime_runs
         BEGIN SELECT RAISE(ABORT, 'fixture update rejection'); END",
    )
    .execute(harness.store.pool())
    .await
    .expect("安装失败触发器");

    let error = harness
        .runner
        .transition(
            &harness.access,
            &run,
            RuntimeEvent::Recall,
            &harness.metadata(1),
        )
        .await
        .expect_err("Runtime 更新失败时整个事务必须回滚");
    assert!(matches!(error, RuntimeError::Storage(_)));

    let persisted = RuntimeRecovery::from_store(&harness.store)
        .recover(&harness.access, run.run_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(persisted.state, RuntimeState::Observing);
    assert_eq!(persisted.next_step, 0);
    let transition_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM transitions WHERE run_id = ?1")
            .bind(run.run_id.to_string())
            .fetch_one(harness.store.pool())
            .await
            .unwrap();
    assert_eq!(transition_count, 0);
}

#[tokio::test]
async fn concurrent_transitions_from_one_snapshot_allocate_one_step() {
    let harness = Harness::new().await;
    let run = harness
        .runner
        .start(&harness.access, &harness.scope)
        .await
        .unwrap();
    let first_metadata = harness.metadata(1);
    let second_metadata = harness.metadata(1);
    let (first, second) = tokio::join!(
        harness
            .runner
            .transition(&harness.access, &run, RuntimeEvent::Recall, &first_metadata),
        harness.runner.transition(
            &harness.access,
            &run,
            RuntimeEvent::Recall,
            &second_metadata
        ),
    );
    let results = [first, second];
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Err(RuntimeError::ConcurrentModification)))
            .count(),
        1
    );

    let recovered = RuntimeRecovery::from_store(&harness.store)
        .recover(&harness.access, run.run_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(recovered.state, RuntimeState::Recalling);
    assert_eq!(recovered.next_step, 1);
    let steps: Vec<i64> =
        sqlx::query_scalar("SELECT step FROM transitions WHERE run_id = ?1 ORDER BY step")
            .bind(run.run_id.to_string())
            .fetch_all(harness.store.pool())
            .await
            .unwrap();
    assert_eq!(steps, vec![0]);
}

#[tokio::test]
async fn runtime_boundaries_require_exact_mutate_authorization() {
    let harness = Harness::new().await;
    let denied = denied_contexts(&harness.scope);
    for (access, reason) in &denied {
        assert_access_denied(
            &harness
                .runner
                .start(access, &harness.scope)
                .await
                .expect_err("未授权上下文不得创建 run"),
            *reason,
        );
    }
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM runtime_runs")
        .fetch_one(harness.store.pool())
        .await
        .unwrap();
    assert_eq!(count, 0);

    let run = harness
        .runner
        .start(&harness.access, &harness.scope)
        .await
        .expect("精确 Mutate Grant 应允许创建 run");
    for (access, reason) in &denied {
        assert_access_denied(
            &RuntimeRecovery::from_store(&harness.store)
                .recover(access, run.run_id)
                .await
                .expect_err("未授权上下文不得恢复 run"),
            *reason,
        );
        assert_access_denied(
            &harness
                .runner
                .transition(access, &run, RuntimeEvent::Recall, &harness.metadata(1))
                .await
                .expect_err("未授权上下文不得推进 run"),
            *reason,
        );
    }

    let recovered = RuntimeRecovery::from_store(&harness.store)
        .recover(&harness.access, run.run_id)
        .await
        .expect("授权恢复应成功")
        .expect("run 应存在");
    let recalled = harness
        .runner
        .transition(
            &harness.access,
            &recovered,
            RuntimeEvent::Recall,
            &harness.metadata(1),
        )
        .await
        .expect("授权转换应成功");
    assert_eq!(recalled.state, RuntimeState::Recalling);
}

#[tokio::test]
async fn transition_rejects_missing_forged_or_cross_scope_evidence_atomically() {
    let harness = Harness::new().await;
    let run = harness
        .runner
        .start(&harness.access, &harness.scope)
        .await
        .unwrap();
    let valid = harness.evidence[0].clone();
    let mut missing = valid.clone();
    missing.id = EvidenceId::new();
    let mut both_targets = valid.clone();
    both_targets.event_id = Some(EventId::new());
    let mut no_target = valid.clone();
    no_target.fragment_id = None;
    let mut forged_target = valid.clone();
    forged_target.fragment_id = Some(FragmentId::new());
    let mut forged_method = valid.clone();
    forged_method.method = VerificationMethod::Manual;
    let mut forged_note = valid;
    forged_note.note = Some("伪造说明".into());
    let foreign_scope = exact_scope();
    let cross_scope = append_evidence_fixture(&harness.store, &foreign_scope).await;
    let foreign_owner =
        append_foreign_owner_evidence_fixture(&harness.store, &foreign_scope, &harness.scope).await;

    for evidence in [
        missing,
        both_targets,
        no_target,
        forged_target,
        forged_method,
        forged_note,
        cross_scope[0].clone(),
        foreign_owner,
    ] {
        let error = harness
            .runner
            .transition(
                &harness.access,
                &run,
                RuntimeEvent::Recall,
                &metadata(1, &[evidence]),
            )
            .await
            .expect_err("非法输入 EvidenceRef 必须被拒绝");
        assert!(matches!(error, RuntimeError::InvalidEvidenceRef));
    }
    let invalid_output = EvidenceRef {
        id: EvidenceId::new(),
        fragment_id: Some(FragmentId::new()),
        event_id: None,
        method: VerificationMethod::Tool,
        note: None,
    };
    let mut invalid_output_metadata = metadata(1, &[]);
    invalid_output_metadata.output_refs.push(invalid_output);
    assert!(matches!(
        harness
            .runner
            .transition(
                &harness.access,
                &run,
                RuntimeEvent::Recall,
                &invalid_output_metadata,
            )
            .await,
        Err(RuntimeError::InvalidEvidenceRef)
    ));

    assert_snapshot_unchanged(&harness, &run).await;
    let recalled = harness
        .runner
        .transition(
            &harness.access,
            &run,
            RuntimeEvent::Recall,
            &harness.metadata(1),
        )
        .await
        .expect("权威且同 Scope 的 Fragment/Event 引用应通过");
    assert_eq!(recalled.next_step, 1);
}

#[tokio::test]
async fn recovery_rejects_illegal_transition_edge() {
    let harness = Harness::new().await;
    let snapshot = harness.recalled_snapshot().await;
    sqlx::query("DROP TRIGGER IF EXISTS protect_runtime_transition_update")
        .execute(harness.store.pool())
        .await
        .unwrap();
    sqlx::query("UPDATE transitions SET to_stage = 'completed' WHERE run_id = ?1 AND step = 0")
        .bind(snapshot.run_id.to_string())
        .execute(harness.store.pool())
        .await
        .unwrap();
    sqlx::query("UPDATE runtime_runs SET state = 'completed' WHERE run_id = ?1")
        .bind(snapshot.run_id.to_string())
        .execute(harness.store.pool())
        .await
        .unwrap();

    assert!(matches!(
        RuntimeRecovery::from_store(&harness.store)
            .recover(&harness.access, snapshot.run_id)
            .await,
        Err(RuntimeError::Corrupt(_))
    ));
}

#[tokio::test]
async fn recovery_rejects_missing_transition_history() {
    let harness = Harness::new().await;
    let snapshot = harness.reasoned_snapshot().await;
    sqlx::query("DROP TRIGGER IF EXISTS protect_runtime_transition_delete")
        .execute(harness.store.pool())
        .await
        .unwrap();
    sqlx::query("DELETE FROM transitions WHERE run_id = ?1 AND step = 0")
        .bind(snapshot.run_id.to_string())
        .execute(harness.store.pool())
        .await
        .unwrap();

    assert!(matches!(
        RuntimeRecovery::from_store(&harness.store)
            .recover(&harness.access, snapshot.run_id)
            .await,
        Err(RuntimeError::Corrupt(_))
    ));
}

#[tokio::test]
async fn recovery_rejects_non_contiguous_transition_steps() {
    let harness = Harness::new().await;
    let snapshot = harness.reasoned_snapshot().await;
    sqlx::query("DROP TRIGGER IF EXISTS protect_runtime_transition_update")
        .execute(harness.store.pool())
        .await
        .unwrap();
    sqlx::query("UPDATE transitions SET step = 7 WHERE run_id = ?1 AND step = 1")
        .bind(snapshot.run_id.to_string())
        .execute(harness.store.pool())
        .await
        .unwrap();

    assert!(matches!(
        RuntimeRecovery::from_store(&harness.store)
            .recover(&harness.access, snapshot.run_id)
            .await,
        Err(RuntimeError::Corrupt(_))
    ));
}

#[tokio::test]
async fn recovery_rejects_snapshot_state_mismatch() {
    let harness = Harness::new().await;
    let snapshot = harness.recalled_snapshot().await;
    sqlx::query("UPDATE runtime_runs SET state = 'reasoning' WHERE run_id = ?1")
        .bind(snapshot.run_id.to_string())
        .execute(harness.store.pool())
        .await
        .unwrap();

    assert!(matches!(
        RuntimeRecovery::from_store(&harness.store)
            .recover(&harness.access, snapshot.run_id)
            .await,
        Err(RuntimeError::Corrupt(_))
    ));
}

#[tokio::test]
async fn transition_rejects_snapshot_version_mismatch_before_advancing() {
    let harness = Harness::new().await;
    let snapshot = harness.recalled_snapshot().await;
    sqlx::query("UPDATE runtime_runs SET version = 9 WHERE run_id = ?1")
        .bind(snapshot.run_id.to_string())
        .execute(harness.store.pool())
        .await
        .unwrap();

    assert!(matches!(
        RuntimeRecovery::from_store(&harness.store)
            .recover(&harness.access, snapshot.run_id)
            .await,
        Err(RuntimeError::Corrupt(_))
    ));
    assert!(matches!(
        harness
            .runner
            .transition(
                &harness.access,
                &snapshot,
                RuntimeEvent::Reason,
                &harness.metadata(1),
            )
            .await,
        Err(RuntimeError::Corrupt(_))
    ));
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM transitions WHERE run_id = ?1")
        .bind(snapshot.run_id.to_string())
        .fetch_one(harness.store.pool())
        .await
        .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn recovery_rejects_transition_when_zero_steps_are_committed() {
    let harness = Harness::new().await;
    let run = harness
        .runner
        .start(&harness.access, &harness.scope)
        .await
        .unwrap();
    let scope_key: String =
        sqlx::query_scalar("SELECT scope_key FROM runtime_runs WHERE run_id = ?1")
            .bind(run.run_id.to_string())
            .fetch_one(harness.store.pool())
            .await
            .unwrap();
    sqlx::query(
        "INSERT INTO transitions (
            id, scope_key, trace_id, from_stage, to_stage, payload_json, created_at_us,
            run_id, step, input_refs_json, output_refs_json, attempt, deadline_at_us
         ) VALUES (?1, ?2, ?3, 'observing', 'recalling', '{}', 1, ?4, 0, '[]', '[]', 1, 2)",
    )
    .bind(uuid::Uuid::now_v7().to_string())
    .bind(scope_key)
    .bind(TraceId::new().to_string())
    .bind(run.run_id.to_string())
    .execute(harness.store.pool())
    .await
    .unwrap();

    assert!(matches!(
        RuntimeRecovery::from_store(&harness.store)
            .recover(&harness.access, run.run_id)
            .await,
        Err(RuntimeError::Corrupt(_))
    ));
}

#[tokio::test]
async fn malformed_persisted_scope_json_is_classified_as_corrupt() {
    let harness = Harness::new().await;
    let run = harness
        .runner
        .start(&harness.access, &harness.scope)
        .await
        .unwrap();
    sqlx::query("DROP TRIGGER IF EXISTS protect_runtime_run_identity")
        .execute(harness.store.pool())
        .await
        .unwrap();
    let mut connection = harness.store.pool().acquire().await.unwrap();
    sqlx::query("PRAGMA ignore_check_constraints = ON")
        .execute(&mut *connection)
        .await
        .unwrap();
    sqlx::query("UPDATE runtime_runs SET scope_json = '{' WHERE run_id = ?1")
        .bind(run.run_id.to_string())
        .execute(&mut *connection)
        .await
        .unwrap();
    sqlx::query("PRAGMA ignore_check_constraints = OFF")
        .execute(&mut *connection)
        .await
        .unwrap();
    drop(connection);

    assert!(matches!(
        RuntimeRecovery::from_store(&harness.store)
            .recover(&harness.access, run.run_id)
            .await,
        Err(RuntimeError::Corrupt(_))
    ));
}

struct Harness {
    _directory: TempDir,
    path: std::path::PathBuf,
    store: SqliteStore,
    runner: RuntimeRunner,
    scope: MemoryScope,
    access: AccessContext,
    evidence: Vec<EvidenceRef>,
}

impl Harness {
    async fn new() -> Self {
        let directory = tempfile::tempdir().expect("创建 Runtime 测试目录");
        let path = directory.path().join("memory.sqlite3");
        let store = SqliteStore::open(&SqliteConfig::new(&path))
            .await
            .expect("打开 Runtime 测试存储");
        let scope = exact_scope();
        let access = authorized(&scope);
        let evidence = append_evidence_fixture(&store, &scope).await;
        let runner = RuntimeRunner::from_store(&store);
        Self {
            _directory: directory,
            path,
            store,
            runner,
            scope,
            access,
            evidence,
        }
    }

    fn metadata(&self, attempt: u32) -> TransitionMetadata {
        metadata(attempt, &self.evidence)
    }

    async fn recalled_snapshot(&self) -> jc_memory_runtime::RuntimeSnapshot {
        let run = self.runner.start(&self.access, &self.scope).await.unwrap();
        self.runner
            .transition(&self.access, &run, RuntimeEvent::Recall, &self.metadata(1))
            .await
            .unwrap()
    }

    async fn reasoned_snapshot(&self) -> jc_memory_runtime::RuntimeSnapshot {
        let recalled = self.recalled_snapshot().await;
        self.runner
            .transition(
                &self.access,
                &recalled,
                RuntimeEvent::Reason,
                &self.metadata(1),
            )
            .await
            .unwrap()
    }
}

fn metadata(attempt: u32, evidence: &[EvidenceRef]) -> TransitionMetadata {
    TransitionMetadata {
        input_refs: evidence.to_vec(),
        output_refs: evidence.to_vec(),
        attempt,
        deadline: Utc.timestamp_micros(1_900_000_000_000_000).unwrap(),
        trace_id: TraceId::new(),
        payload: serde_json::json!({"reason": "runtime test"}),
    }
}

fn exact_scope() -> MemoryScope {
    MemoryScope {
        namespace_id: NamespaceId::new(),
        tenant_id: Some(TenantId::new()),
        user_id: Some(UserId::new()),
        agent_id: Some(AgentId::new()),
        project_id: Some(ProjectId::new()),
    }
}

fn authorized(scope: &MemoryScope) -> AccessContext {
    let mut access = AccessContext::new("runtime-test", scope.clone());
    access
        .grants
        .insert(Grant::for_scope(Operation::Mutate, scope));
    access
}

fn denied_contexts(scope: &MemoryScope) -> Vec<(AccessContext, DenyReason)> {
    let missing_grant = AccessContext::new("missing-grant", scope.clone());

    let mut namespace_scope = scope.clone();
    namespace_scope.namespace_id = NamespaceId::new();
    let mut namespace = AccessContext::new("other-namespace", namespace_scope);
    namespace
        .grants
        .insert(Grant::for_scope(Operation::Mutate, scope));

    let mut tenant_scope = scope.clone();
    tenant_scope.tenant_id = Some(TenantId::new());
    let mut tenant = AccessContext::new("other-tenant", tenant_scope);
    tenant
        .grants
        .insert(Grant::for_scope(Operation::Mutate, scope));

    let mut project_scope = scope.clone();
    project_scope.project_id = Some(ProjectId::new());
    let project = authorized(&project_scope);

    let mut user_scope = scope.clone();
    user_scope.user_id = Some(UserId::new());
    let mut user = AccessContext::new("other-user", user_scope);
    user.grants
        .insert(Grant::for_scope(Operation::Mutate, scope));

    let mut agent_scope = scope.clone();
    agent_scope.agent_id = Some(AgentId::new());
    let mut agent = AccessContext::new("other-agent", agent_scope);
    agent
        .grants
        .insert(Grant::for_scope(Operation::Mutate, scope));

    vec![
        (missing_grant, DenyReason::MissingGrant),
        (namespace, DenyReason::NamespaceMismatch),
        (tenant, DenyReason::CrossTenantRequiresAdmin),
        (project, DenyReason::MissingGrant),
        (user, DenyReason::SubjectScopeMismatch),
        (agent, DenyReason::SubjectScopeMismatch),
    ]
}

fn assert_access_denied(error: &RuntimeError, expected: DenyReason) {
    assert!(
        matches!(error, RuntimeError::AccessDenied(reason) if *reason == expected),
        "授权拒绝原因不匹配: {error}"
    );
}

async fn assert_snapshot_unchanged(
    harness: &Harness,
    expected: &jc_memory_runtime::RuntimeSnapshot,
) {
    let recovered = RuntimeRecovery::from_store(&harness.store)
        .recover(&harness.access, expected.run_id)
        .await
        .expect("读取未推进快照")
        .expect("run 应存在");
    assert_eq!(&recovered, expected);
    let transition_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM transitions WHERE run_id = ?1")
            .bind(expected.run_id.to_string())
            .fetch_one(harness.store.pool())
            .await
            .unwrap();
    assert_eq!(transition_count, 0);
}

async fn append_evidence_fixture(store: &SqliteStore, scope: &MemoryScope) -> Vec<EvidenceRef> {
    let repository = SqliteMemoryRepository::from_store(store);
    let target = fragment(scope.clone(), Vec::new(), "目标证据");
    let target_draft = EventFactory
        .from_fragment(target.clone())
        .expect("构造目标 Event");
    repository
        .append(&target_draft)
        .await
        .expect("写入目标证据");
    let fragment_evidence = EvidenceRef {
        id: EvidenceId::new(),
        fragment_id: Some(target.id),
        event_id: None,
        method: VerificationMethod::Tool,
        note: Some("可信工具证据".into()),
    };
    let event_evidence = EvidenceRef {
        id: EvidenceId::new(),
        fragment_id: None,
        event_id: Some(target_draft.id),
        method: VerificationMethod::ExternalSource,
        note: Some("可信外部证据".into()),
    };
    let evidence = vec![fragment_evidence, event_evidence];
    let owner = fragment(scope.clone(), evidence.clone(), "证据所有者");
    let owner_draft = EventFactory
        .from_fragment(owner)
        .expect("构造证据所有者 Event");
    repository
        .append(&owner_draft)
        .await
        .expect("写入证据所有者");
    evidence
}

async fn append_foreign_owner_evidence_fixture(
    store: &SqliteStore,
    owner_scope: &MemoryScope,
    target_scope: &MemoryScope,
) -> EvidenceRef {
    let repository = SqliteMemoryRepository::from_store(store);
    let target = fragment(target_scope.clone(), Vec::new(), "当前 Scope 目标证据");
    let target_draft = EventFactory
        .from_fragment(target.clone())
        .expect("构造当前 Scope 目标 Event");
    repository
        .append(&target_draft)
        .await
        .expect("写入当前 Scope 目标证据");
    let evidence = EvidenceRef {
        id: EvidenceId::new(),
        fragment_id: Some(target.id),
        event_id: None,
        method: VerificationMethod::Tool,
        note: Some("外部 Scope owner".into()),
    };
    let owner = fragment(
        owner_scope.clone(),
        vec![evidence.clone()],
        "外部 Scope owner",
    );
    let owner_draft = EventFactory
        .from_fragment(owner)
        .expect("构造外部 Scope owner Event");
    repository
        .append(&owner_draft)
        .await
        .expect("写入外部 Scope owner");
    evidence
}

fn fragment(scope: MemoryScope, evidence: Vec<EvidenceRef>, text: &str) -> CognitiveFragment {
    let now = Utc::now();
    CognitiveFragment {
        id: FragmentId::new(),
        scope,
        session_id: None,
        trace_id: None,
        kind: FragmentKind::Observation,
        content: FragmentContent::Text { text: text.into() },
        source: FragmentSource {
            source_type: SourceType::Tool,
            source_id: "runtime-test".into(),
            actor_id: None,
            observer_id: None,
            tool_id: Some("runtime-test".into()),
            uri: None,
        },
        perspective: Perspective::Tool,
        occurred_at: now,
        observed_at: now,
        confidence: 1.0,
        importance: 0.5,
        validity: ValidityStatus::Unverified,
        security_label: SecurityLabel::Internal,
        evidence,
        metadata: serde_json::Map::new(),
    }
}

#[allow(dead_code)]
fn assert_run_id_is_copy(_: RunId) {}
