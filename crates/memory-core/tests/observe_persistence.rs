//! Embedded observe and health contract tests.

mod common;

use jc_memory_core::{HealthStatus, MemoryConfig, MemoryEngine};
use jc_memory_protocol::ValidityStatus;

#[tokio::test]
async fn observe_returns_authoritative_event_and_survives_reopen() {
    let data_dir = tempfile::tempdir().expect("create engine data directory");
    let engine = MemoryEngine::open(MemoryConfig::local(data_dir.path()))
        .await
        .expect("open memory engine");
    let fragment = common::observation("2018 project failed because migration timed out");
    let context = common::access_context(&fragment.scope);

    let receipt = engine
        .observe(&context, fragment.clone())
        .await
        .expect("observe fragment");

    assert_eq!(receipt.fragment_id, fragment.id);
    assert_eq!(receipt.sequence, 0);
    assert!(receipt.created);
    assert!(!receipt.quarantined);
    drop(engine);

    let reopened = MemoryEngine::open(MemoryConfig::local(data_dir.path()))
        .await
        .expect("reopen memory engine");
    assert_eq!(
        reopened
            .fragment(&context, receipt.fragment_id)
            .await
            .expect("read persisted fragment"),
        fragment
    );
    let health = reopened.health(&context).await.expect("read engine health");
    assert_eq!(health.status, HealthStatus::Healthy);
    assert!(health.database_ok);
    assert!(health.blob_store_ok);
    assert_eq!(health.schema_version, 2);
}

#[tokio::test]
async fn repeated_observe_reuses_the_authoritative_event_across_reopen() {
    let data_dir = tempfile::tempdir().expect("create engine data directory");
    let fragment = common::observation("idempotent embedded observation");
    let context = common::access_context(&fragment.scope);
    let engine = MemoryEngine::open(MemoryConfig::local(data_dir.path()))
        .await
        .expect("open memory engine");
    let first = engine
        .observe(&context, fragment.clone())
        .await
        .expect("first observe");
    let duplicate = engine
        .observe(&context, fragment.clone())
        .await
        .expect("duplicate observe");
    assert!(!duplicate.created);
    assert_eq!(duplicate.fragment_id, first.fragment_id);
    assert_eq!(duplicate.event_id, first.event_id);
    assert_eq!(duplicate.sequence, first.sequence);
    assert_eq!(duplicate.content_hash, first.content_hash);
    drop(engine);

    let reopened = MemoryEngine::open(MemoryConfig::local(data_dir.path()))
        .await
        .expect("reopen memory engine");
    let after_reopen = reopened
        .observe(&context, fragment)
        .await
        .expect("observe after reopen");
    assert!(!after_reopen.created);
    assert_eq!(after_reopen.event_id, first.event_id);
    assert_eq!(after_reopen.sequence, first.sequence);
}

#[tokio::test]
async fn observe_reports_an_explicit_quarantine_status() {
    let data_dir = tempfile::tempdir().expect("create engine data directory");
    let engine = MemoryEngine::open(MemoryConfig::local(data_dir.path()))
        .await
        .expect("open memory engine");
    let mut fragment = common::observation("untrusted imported observation");
    fragment.validity = ValidityStatus::Quarantined;
    let context = common::access_context(&fragment.scope);

    let receipt = engine
        .observe(&context, fragment)
        .await
        .expect("observe quarantine");

    assert!(receipt.quarantined);
}
