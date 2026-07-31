//! Dify Scope、HTTP/MCP 节点和失败边界契约测试。

use std::sync::Arc;

use jc_memory_adapter_dify::{
    DifyAdapter, DifyConfig, DifyInvocation, DifyNodeStatus, DifyNodeType, DifyWorkflowNode,
};
use jc_memory_core::{EmbeddedCommandBus, MemoryConfig, MemoryEngine};
use jc_memory_protocol::{AccessContext, Grant, MemoryPacket, Operation, RecallId, RecallMode};

fn config() -> DifyConfig {
    DifyConfig::from_yaml(include_str!("fixtures/dify-adapter.yaml")).expect("解析 Dify 配置")
}

fn invocation() -> DifyInvocation {
    DifyInvocation {
        user_id: "user-17".to_owned(),
        conversation_id: "conversation-42".to_owned(),
        app_id: "app-3".to_owned(),
        workflow_id: "workflow-8".to_owned(),
        trace_id: "trace-99".to_owned(),
    }
}

#[test]
fn dify_scope_maps_user_conversation_app_workflow_and_trace() {
    let config = config();
    let adapter = DifyAdapter::new(config.clone());
    let mapped = adapter.map_scope(&invocation()).expect("映射 Dify Scope");

    assert_eq!(mapped.scope.namespace_id, config.namespace_id);
    assert_eq!(mapped.scope.user_id.unwrap().to_string().len(), 36);
    assert_eq!(mapped.scope.project_id.unwrap().to_string().len(), 36);
    assert_eq!(mapped.session_id.to_string().len(), 36);
    assert_eq!(mapped.trace_id.to_string().len(), 36);
    assert_eq!(mapped.external.user_id, "user-17");
    assert_eq!(mapped.external.workflow_id, "workflow-8");
}

#[test]
fn http_and_mcp_nodes_parse_structured_memory_packets_and_fail_closed() {
    let adapter = DifyAdapter::new(config());
    let mut packet = MemoryPacket::empty(RecallId::new(), RecallMode::Exact);
    packet.recommended_context = "fixture-context".to_owned();
    let output = serde_json::to_value(&packet).expect("编码 Memory Packet");

    for node_type in [DifyNodeType::Http, DifyNodeType::Mcp] {
        let node = DifyWorkflowNode {
            node_id: "node-1".to_owned(),
            node_type,
            status: DifyNodeStatus::Succeeded,
            output: Some(output.clone()),
            error: None,
        };
        let mapped = adapter.map_workflow_node(node).expect("映射成功节点");
        assert_eq!(mapped.recommended_context, "fixture-context");
    }

    let failed = DifyWorkflowNode {
        node_id: "node-failed".to_owned(),
        node_type: DifyNodeType::Http,
        status: DifyNodeStatus::Failed,
        output: None,
        error: Some("upstream timeout".to_owned()),
    };
    let error = adapter
        .map_workflow_node(failed)
        .expect_err("失败节点必须拒绝输出");
    assert!(error.to_string().contains("node-failed"));
    assert!(error.to_string().contains("upstream timeout"));
}

#[test]
fn unsupported_workflow_node_fails_closed() {
    let adapter = DifyAdapter::new(config());
    let error = adapter
        .map_workflow_node(DifyWorkflowNode {
            node_id: "node-unsupported".to_owned(),
            node_type: DifyNodeType::Unsupported,
            status: DifyNodeStatus::Succeeded,
            output: Some(serde_json::json!({})),
            error: None,
        })
        .expect_err("不支持的节点类型必须拒绝");
    assert!(error.to_string().contains("node-unsupported"));
}

#[tokio::test]
async fn recall_uses_dify_scope_and_core_packet_without_api_keys() {
    let directory = tempfile::tempdir().expect("创建引擎目录");
    let engine = MemoryEngine::open(MemoryConfig::local(directory.path()))
        .await
        .expect("打开引擎");
    let adapter = DifyAdapter::new(config());
    let mapped = adapter.map_scope(&invocation()).expect("映射 Scope");
    let mut access = AccessContext::new("dify-test", mapped.scope.clone());
    access
        .grants
        .insert(Grant::for_scope(Operation::Recall, &mapped.scope));
    let client = adapter.with_bus(Arc::new(EmbeddedCommandBus::new(engine)));
    let packet = client
        .recall(&access, &mapped, "workflow context", RecallMode::Exact)
        .await
        .expect("执行 Dify Recall");
    assert_eq!(packet.intent, RecallMode::Exact);
}

#[test]
fn fixture_declares_dify_contract_without_secret_material() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/dify-v1.json")).expect("解析 Dify Fixture");
    assert_eq!(fixture["contract_version"], "dify/v1");
    assert_eq!(fixture["verification"], "fixture_only");
    assert!(fixture.get("api_key").is_none());
    assert_eq!(config().source_version, "dify/v1");
}
