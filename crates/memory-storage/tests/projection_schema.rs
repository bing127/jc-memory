//! 持久化投影边界与精确 Scope Checkpoint 契约。

use chrono::{DateTime, Utc};
use jc_memory_protocol::{MemoryScope, NamespaceId};
use jc_memory_storage::{
    ProjectionCheckpoint, ProjectionTarget, ProjectorKind, SqliteConfig, SqliteProjectionStore,
    SqliteStore,
};

#[test]
fn projection_types_preserve_u64_sequences_and_projector_identity() {
    let updated_at = DateTime::parse_from_rfc3339("2026-08-02T10:00:00Z")
        .expect("解析 Checkpoint 时间")
        .with_timezone(&Utc);
    for projector in [ProjectorKind::State, ProjectorKind::Graph] {
        let checkpoint = ProjectionCheckpoint {
            projector,
            sequence: u64::MAX,
            updated_at,
        };
        assert_eq!(checkpoint.projector, projector);
        assert_eq!(checkpoint.sequence, u64::MAX);
        assert_eq!(checkpoint.updated_at, updated_at);
    }
    assert_eq!(ProjectionTarget { sequence: 42 }.sequence, 42);
}

#[tokio::test]
async fn empty_scope_has_zero_target_and_independent_projector_checkpoints() {
    let directory = tempfile::tempdir().expect("创建投影 Schema 测试目录");
    let store = SqliteStore::open(&SqliteConfig::new(
        directory.path().join("projection.sqlite3"),
    ))
    .await
    .expect("打开投影 Schema 测试数据库");
    let projection = SqliteProjectionStore::from_store(&store);
    let scope = MemoryScope::new(NamespaceId::new());

    assert_eq!(
        projection.target(&scope).await.expect("读取空 Scope 目标"),
        ProjectionTarget { sequence: 0 }
    );
    for projector in [ProjectorKind::State, ProjectorKind::Graph] {
        let checkpoint = projection
            .checkpoint(&scope, projector)
            .await
            .expect("读取空 Scope Checkpoint");
        assert_eq!(checkpoint.projector, projector);
        assert_eq!(checkpoint.sequence, 0);
        assert_eq!(checkpoint.updated_at, DateTime::<Utc>::UNIX_EPOCH);
    }
}
