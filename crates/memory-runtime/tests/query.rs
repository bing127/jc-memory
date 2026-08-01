//! Runtime 只读查询的 Scope、分页、时间线与控制检查点契约测试。

use chrono::{Duration, Utc};
use jc_memory_protocol::{AccessContext, Grant, MemoryScope, NamespaceId, Operation, TraceId};
use jc_memory_runtime::{
    ControlState, RuntimeCheckpoint, RuntimeCheckpointStore, RuntimeEvent, RuntimeReadStore,
    RuntimeRunQuery, RuntimeRunner, RuntimeState, TransitionMetadata,
};
use jc_memory_storage::{SqliteConfig, SqliteStore};
use serde_json::json;
use tempfile::tempdir;

fn access(scope: &MemoryScope, operations: impl IntoIterator<Item = Operation>) -> AccessContext {
    let mut access = AccessContext::new("runtime-query", scope.clone());
    access.grants = operations
        .into_iter()
        .map(|operation| Grant::for_scope(operation, scope))
        .collect();
    access
}

#[tokio::test]
async fn list_is_bounded_filtered_and_scope_isolated() {
    let directory = tempdir().unwrap();
    let store = SqliteStore::open(&SqliteConfig::new(directory.path().join("memory.db")))
        .await
        .unwrap();
    let scope_a = MemoryScope::new(NamespaceId::new());
    let scope_b = MemoryScope::new(NamespaceId::new());
    let runner = RuntimeRunner::from_store(&store);
    let access_a = access(&scope_a, [Operation::Mutate, Operation::Recall]);
    let access_b = access(&scope_b, [Operation::Mutate]);
    let first = runner.start(&access_a, &scope_a).await.unwrap();
    let recalled = runner
        .transition(
            &access_a,
            &first,
            RuntimeEvent::Recall,
            &metadata("observe"),
        )
        .await
        .unwrap();
    runner.start(&access_b, &scope_b).await.unwrap();

    let page = RuntimeReadStore::from_store(&store)
        .list(
            &access_a,
            &scope_a,
            RuntimeRunQuery {
                state: Some(RuntimeState::Recalling),
                limit: 1,
                offset: 0,
            },
        )
        .await
        .unwrap();

    assert_eq!(page.total, 1);
    assert_eq!(page.limit, 1);
    assert_eq!(page.offset, 0);
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].run_id, recalled.run_id);
    assert_eq!(page.items[0].scope, scope_a);
    assert_eq!(page.items[0].state, RuntimeState::Recalling);
}

#[tokio::test]
async fn detail_returns_ordered_transitions_and_control_checkpoint() {
    let directory = tempdir().unwrap();
    let store = SqliteStore::open(&SqliteConfig::new(directory.path().join("memory.db")))
        .await
        .unwrap();
    let scope = MemoryScope::new(NamespaceId::new());
    let access = access(&scope, [Operation::Mutate, Operation::Recall]);
    let runner = RuntimeRunner::from_store(&store);
    let started = runner.start(&access, &scope).await.unwrap();
    let recalling = runner
        .transition(
            &access,
            &started,
            RuntimeEvent::Recall,
            &metadata("observe"),
        )
        .await
        .unwrap();
    let reasoning = runner
        .transition(
            &access,
            &recalling,
            RuntimeEvent::Reason,
            &metadata("recall"),
        )
        .await
        .unwrap();
    RuntimeCheckpointStore::from_store(&store)
        .save(&RuntimeCheckpoint {
            run_id: reasoning.run_id,
            state: ControlState::Paused,
            resume_state: RuntimeState::Reasoning,
            user_input: None,
            next_attempt_at: None,
            attempt: 2,
        })
        .await
        .unwrap();

    let detail = RuntimeReadStore::from_store(&store)
        .get(&access, &scope, reasoning.run_id)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(detail.run.run_id, reasoning.run_id);
    assert_eq!(detail.transitions.len(), 2);
    assert_eq!(detail.transitions[0].step, 0);
    assert_eq!(detail.transitions[0].payload, json!({"phase":"observe"}));
    assert_eq!(detail.transitions[1].step, 1);
    assert_eq!(detail.transitions[1].payload, json!({"phase":"recall"}));
    let control = detail.run.control.unwrap();
    assert_eq!(control.state, ControlState::Paused);
    assert_eq!(control.resume_state, RuntimeState::Reasoning);
    assert_eq!(control.attempt, 2);
}

#[tokio::test]
async fn queries_deny_missing_recall_grant_and_cross_scope_access() {
    let directory = tempdir().unwrap();
    let store = SqliteStore::open(&SqliteConfig::new(directory.path().join("memory.db")))
        .await
        .unwrap();
    let scope_a = MemoryScope::new(NamespaceId::new());
    let scope_b = MemoryScope::new(NamespaceId::new());
    let denied = access(&scope_a, [Operation::Mutate]);
    let cross_scope = access(&scope_a, [Operation::Recall]);
    let reader = RuntimeReadStore::from_store(&store);

    assert!(
        reader
            .list(&denied, &scope_a, RuntimeRunQuery::default())
            .await
            .is_err()
    );
    assert!(
        reader
            .list(&cross_scope, &scope_b, RuntimeRunQuery::default())
            .await
            .is_err()
    );
}

fn metadata(phase: &str) -> TransitionMetadata {
    TransitionMetadata {
        input_refs: Vec::new(),
        output_refs: Vec::new(),
        attempt: 1,
        deadline: Utc::now() + Duration::minutes(1),
        trace_id: TraceId::new(),
        payload: json!({"phase": phase}),
    }
}
