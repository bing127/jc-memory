//! Mem0 Adapter 的 Add、Search、历史迁移与清单 round-trip 测试。

use std::sync::Arc;

use chrono::{TimeZone as _, Utc};
use jc_memory_adapter_mem0::{
    Mem0Adapter, Mem0Config, Mem0Record, Mem0SearchQuery, export_manifest, parse_manifest,
};
use jc_memory_core::{EmbeddedCommandBus, MemoryConfig, MemoryEngine};
use jc_memory_protocol::{AccessContext, Grant, Operation, RecallMode, ValidityStatus};

fn access() -> AccessContext {
    let scope = jc_memory_protocol::MemoryScope::new(jc_memory_protocol::NamespaceId::new());
    let mut access = AccessContext::new("mem0-test", scope.clone());
    access.grants.extend(
        [Operation::Observe, Operation::Recall, Operation::Mutate]
            .into_iter()
            .map(|operation| Grant::for_scope(operation, &scope)),
    );
    access
}

fn config() -> Mem0Config {
    Mem0Config::from_yaml(include_str!("fixtures/mem0-adapter.yaml")).expect("解析 Mem0 配置")
}

fn record(memory: &str, updated_day: u32, confidence: f64) -> Mem0Record {
    Mem0Record {
        id: "mem0-42".to_owned(),
        memory: memory.to_owned(),
        created_at: Utc.with_ymd_and_hms(2025, 1, 2, 3, 4, 5).unwrap(),
        updated_at: Utc.with_ymd_and_hms(2025, 1, updated_day, 3, 4, 5).unwrap(),
        user_id: Some("legacy-user".to_owned()),
        agent_id: Some("legacy-agent".to_owned()),
        legacy_confidence: Some(confidence),
        metadata: serde_json::json!({"category": "preference"}),
    }
}

async fn fixture() -> (MemoryEngine, AccessContext, Mem0Adapter) {
    let data_dir = tempfile::tempdir().expect("创建引擎目录");
    let data_path = data_dir.keep();
    let engine = MemoryEngine::open(MemoryConfig::local(data_path))
        .await
        .expect("打开引擎");
    let access = access();
    let adapter = Mem0Adapter::new(
        Arc::new(EmbeddedCommandBus::new(engine.clone())),
        access.clone(),
        config(),
    );
    (engine, access, adapter)
}

#[tokio::test]
async fn add_maps_legacy_memory_to_unverified_fragment() {
    let (engine, access, adapter) = fixture().await;
    let receipt = adapter
        .add(record("User prefers dark mode", 2, 0.91))
        .await
        .expect("添加 Mem0 记录");
    assert_eq!(receipt.manifest.entries.len(), 1);
    let entry = &receipt.manifest.entries[0];
    assert!(entry.created);

    let fragment = engine
        .fragment(&access, entry.fragment_id)
        .await
        .expect("读取迁移 Fragment");
    assert_eq!(fragment.validity, ValidityStatus::Unverified);
    assert!(fragment.confidence.abs() < f32::EPSILON);
    assert_eq!(fragment.metadata["mem0_id"], "mem0-42");
    assert_eq!(fragment.metadata["legacy_confidence"], 0.91);
    assert_eq!(
        fragment.metadata["legacy_metadata"]["category"],
        "preference"
    );
}

#[tokio::test]
async fn search_maps_to_bounded_core_recall() {
    let (_engine, _access, adapter) = fixture().await;
    adapter
        .add(record("User prefers dark mode", 2, 0.91))
        .await
        .expect("添加 Mem0 记录");
    let packet = adapter
        .search(Mem0SearchQuery {
            query: "dark mode".to_owned(),
            mode: RecallMode::Exact,
        })
        .await
        .expect("执行 Search");
    assert_eq!(packet.intent, RecallMode::Exact);
    assert!(!packet.evidence.is_empty());
}

#[tokio::test]
async fn history_migration_is_idempotent_and_manifest_round_trips() {
    let (engine, access, adapter) = fixture().await;
    let old = record("User prefers dark mode", 2, 0.91);
    let current = record("User prefers light mode", 3, 0.72);
    let receipt = adapter
        .import_history(vec![old.clone(), current])
        .await
        .expect("迁移历史");
    assert_eq!(receipt.manifest.entries.len(), 2);
    assert_ne!(
        receipt.manifest.entries[0].fragment_id,
        receipt.manifest.entries[1].fragment_id
    );
    engine
        .fragment(&access, receipt.manifest.entries[0].fragment_id)
        .await
        .expect("旧历史仍可读取");

    let repeated = adapter
        .import_history(vec![old])
        .await
        .expect("重复迁移历史");
    assert!(!repeated.manifest.entries[0].created);
    let json = export_manifest(&receipt.manifest).expect("导出 ID mapping manifest");
    let restored = parse_manifest(&json).expect("重新解析 ID mapping manifest");
    assert_eq!(restored, receipt.manifest);
}

#[test]
fn fixtures_declare_mem0_contract_without_live_api_claim() {
    assert_eq!(config().source_version, "v1");
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/mem0-v1-history.json"))
            .expect("解析 Mem0 Fixture");
    assert_eq!(fixture["contract_version"], "v1");
    assert_eq!(fixture["verification"], "fixture_only");
}
