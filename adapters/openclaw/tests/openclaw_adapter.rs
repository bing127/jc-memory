//! `OpenClaw` Adapter 的导入、Recall 与历史保留契约测试。

use std::sync::Arc;

use chrono::{TimeZone as _, Utc};
use jc_memory_adapter_openclaw::{
    OpenClawAdapter, OpenClawConfig, OpenClawDocument, OpenClawRecallQuery,
};
use jc_memory_core::{EmbeddedCommandBus, MemoryConfig, MemoryEngine};
use jc_memory_protocol::{AccessContext, Grant, Operation, RecallMode, ValidityStatus};

fn access() -> AccessContext {
    let scope = jc_memory_protocol::MemoryScope::new(jc_memory_protocol::NamespaceId::new());
    let mut access = AccessContext::new("openclaw-test", scope.clone());
    access.grants.extend(
        [Operation::Observe, Operation::Recall, Operation::Mutate]
            .into_iter()
            .map(|operation| Grant::for_scope(operation, &scope)),
    );
    access
}

fn config() -> OpenClawConfig {
    OpenClawConfig::from_yaml(include_str!("fixtures/openclaw-adapter.yaml"))
        .expect("解析 OpenClaw 配置")
}

fn document(content: &str) -> OpenClawDocument {
    OpenClawDocument {
        path: "/workspace/MEMORY.md".to_owned(),
        content: content.to_owned(),
        modified_at: Utc.with_ymd_and_hms(2025, 8, 1, 9, 10, 11).unwrap(),
    }
}

#[tokio::test]
async fn markdown_import_preserves_provenance_is_idempotent_and_keeps_history() {
    let data_dir = tempfile::tempdir().expect("创建引擎目录");
    let engine = MemoryEngine::open(MemoryConfig::local(data_dir.path()))
        .await
        .expect("打开引擎");
    let access = access();
    let adapter = OpenClawAdapter::new(
        Arc::new(EmbeddedCommandBus::new(engine.clone())),
        access.clone(),
        config(),
    );
    let markdown = include_str!("fixtures/MEMORY.md");

    let first = adapter
        .import_markdown(document(markdown))
        .await
        .expect("首次导入");
    assert_eq!(first.entries.len(), 2);
    assert!(first.entries.iter().all(|entry| entry.created));
    assert_eq!(first.entries[0].heading.as_deref(), Some("Preferences"));
    assert_eq!(
        (first.entries[0].line_start, first.entries[0].line_end),
        (1, 3)
    );

    let fragment = engine
        .fragment(&access, first.entries[0].fragment_id)
        .await
        .expect("读取导入 Fragment");
    assert_eq!(fragment.validity, ValidityStatus::Unverified);
    assert_eq!(fragment.metadata["source_path"], "/workspace/MEMORY.md");
    assert_eq!(fragment.metadata["content_sha256"], first.document_sha256);
    assert_eq!(fragment.metadata["heading"], "Preferences");
    assert_eq!(fragment.metadata["line_start"], 1);
    assert_eq!(fragment.metadata["line_end"], 3);

    let repeated = adapter
        .import_markdown(document(markdown))
        .await
        .expect("重复导入");
    assert!(repeated.entries.iter().all(|entry| !entry.created));
    assert_eq!(
        first
            .entries
            .iter()
            .map(|entry| entry.fragment_id)
            .collect::<Vec<_>>(),
        repeated
            .entries
            .iter()
            .map(|entry| entry.fragment_id)
            .collect::<Vec<_>>()
    );

    let changed = adapter
        .import_markdown(document(&markdown.replace("dark mode", "light mode")))
        .await
        .expect("导入变更历史");
    assert_ne!(changed.entries[0].fragment_id, first.entries[0].fragment_id);
    engine
        .fragment(&access, first.entries[0].fragment_id)
        .await
        .expect("原历史仍可读取");
}

#[tokio::test]
async fn hook_and_mcp_recall_return_core_memory_packets() {
    let data_dir = tempfile::tempdir().expect("创建引擎目录");
    let engine = MemoryEngine::open(MemoryConfig::local(data_dir.path()))
        .await
        .expect("打开引擎");
    let access = access();
    let adapter = OpenClawAdapter::new(Arc::new(EmbeddedCommandBus::new(engine)), access, config());
    adapter
        .import_markdown(document(include_str!("fixtures/MEMORY.md")))
        .await
        .expect("导入 Memory.md");
    let query = OpenClawRecallQuery {
        text: "dark mode".to_owned(),
        mode: RecallMode::Exact,
    };

    let hook = adapter
        .hook_recall(query.clone())
        .await
        .expect("Hook Recall");
    let mcp = adapter.mcp_recall(query).await.expect("MCP Recall");
    assert_eq!(hook.intent, RecallMode::Exact);
    assert_eq!(mcp.intent, RecallMode::Exact);
    assert!(!hook.evidence.is_empty());
    assert_eq!(hook.evidence, mcp.evidence);
}

#[test]
fn config_fixture_declares_file_contract_without_runtime_claim() {
    let config = config();
    assert_eq!(config.source_version, "memory.md/v1");
    let contract: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/openclaw-contract.json"))
            .expect("解析契约 Fixture");
    assert_eq!(contract["source_version"], "memory.md/v1");
    assert_eq!(contract["verification"], "fixture_only");
}
