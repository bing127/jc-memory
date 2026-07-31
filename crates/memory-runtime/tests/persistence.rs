//! Runtime 控制检查点跨重开恢复测试。

use std::time::{Duration, SystemTime};

use jc_memory_protocol::{AccessContext, Grant, MemoryScope, NamespaceId, Operation};
use jc_memory_runtime::{
    ControlState, RuntimeCheckpoint, RuntimeCheckpointStore, RuntimeRunner, RuntimeState,
};
use jc_memory_storage::{SqliteConfig, SqliteStore};
use tempfile::tempdir;

fn access(scope: &MemoryScope) -> AccessContext {
    let mut access = AccessContext::new("principal", scope.clone());
    access
        .grants
        .insert(Grant::for_scope(Operation::Mutate, scope));
    access
}

#[tokio::test]
async fn control_checkpoint_survives_store_reopen() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("memory.db");
    let store = SqliteStore::open(&SqliteConfig::new(&path)).await.unwrap();
    let scope = MemoryScope::new(NamespaceId::new());
    let run = RuntimeRunner::from_store(&store)
        .start(&access(&scope), &scope)
        .await
        .unwrap();
    let next_attempt_at = SystemTime::now() + Duration::from_secs(30);
    let expected = RuntimeCheckpoint {
        run_id: run.run_id,
        state: ControlState::WaitingUser,
        resume_state: RuntimeState::Reasoning,
        user_input: Some("等待补充".into()),
        next_attempt_at: Some(next_attempt_at),
        attempt: 2,
    };
    RuntimeCheckpointStore::from_store(&store)
        .save(&expected)
        .await
        .unwrap();
    store.pool().close().await;

    let reopened = SqliteStore::open(&SqliteConfig::new(&path)).await.unwrap();
    let actual = RuntimeCheckpointStore::from_store(&reopened)
        .load(run.run_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(actual.run_id, expected.run_id);
    assert_eq!(actual.state, ControlState::WaitingUser);
    assert_eq!(actual.resume_state, RuntimeState::Reasoning);
    assert_eq!(actual.user_input, expected.user_input);
    assert_eq!(actual.attempt, 2);
    let actual_time = actual.next_attempt_at.unwrap();
    assert!(actual_time >= next_attempt_at - Duration::from_millis(1));
    assert!(actual_time <= next_attempt_at + Duration::from_millis(1));
}
