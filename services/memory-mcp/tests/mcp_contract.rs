//! MCP initialize、工具清单和结构化调用契约测试。

use async_trait::async_trait;
use jc_memory_core::{CommandBus, CommandError};
use jc_memory_mcp::McpServer;
use jc_memory_protocol::{
    AccessContext, MemoryRequest, MemoryResponse, ObserveResult, ResponseEnvelope,
};
use serde_json::{Value, json};

#[derive(Clone)]
struct FixtureBus;

#[async_trait]
impl CommandBus for FixtureBus {
    async fn execute(
        &self,
        _access: &AccessContext,
        request: MemoryRequest,
    ) -> Result<ResponseEnvelope, CommandError> {
        Ok(ResponseEnvelope::for_request(
            &request,
            MemoryResponse::Observe(ObserveResult {
                fragment_id: "00000000-0000-7000-8000-000000000001".parse().unwrap(),
                event_id: "00000000-0000-7000-8000-000000000002".parse().unwrap(),
                sequence: 1,
                content_hash: "a".repeat(64).parse().unwrap(),
                created: true,
                quarantined: false,
            }),
        ))
    }
}

#[tokio::test]
async fn initialize_and_tools_list_expose_the_stable_mcp_contract() {
    let server = McpServer::new(FixtureBus);
    let initialized = server
        .handle(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}))
        .await;
    assert_eq!(initialized["result"]["protocolVersion"], "2025-06-18");

    let listed = server
        .handle(json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}))
        .await;
    let tools = listed["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 7);
    assert!(tools.iter().any(|tool| tool["name"] == "memory_commit"));
    assert!(
        tools
            .iter()
            .all(|tool| tool["inputSchema"]["additionalProperties"] == false)
    );
}

#[tokio::test]
async fn tools_call_returns_structured_content_and_unknown_commit_is_explicit() {
    let server = McpServer::new(FixtureBus);
    let request = json!({
        "api_version":"v1",
        "request_id":"mcp-request-1",
        "access_scope":{"namespace_id":"00000000-0000-7000-8000-000000000003"},
        "command":{"command":"observe","payload":{"fragment":{
            "id":"00000000-0000-7000-8000-000000000004",
            "scope":{"namespace_id":"00000000-0000-7000-8000-000000000003"},
            "kind":"observation",
            "content":{"type":"text","text":"mcp fixture"},
            "source":{"source_type":"user","source_id":"fixture"},
            "perspective":"first_person",
            "occurred_at":"2026-07-31T00:00:00Z",
            "observed_at":"2026-07-31T00:00:00Z",
            "confidence":0.9,
            "importance":0.5,
            "validity":"unverified",
            "security_label":"internal",
            "evidence":[],"metadata":{}
        }}}
    });
    let called = server
        .handle(json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"memory_observe","arguments":{"request":request}}}))
        .await;
    assert_eq!(called["result"]["isError"], false);
    assert!(called["result"].get("structuredContent").is_some());

    let commit = server
        .handle(json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"memory_commit","arguments":{}}}))
        .await;
    assert_eq!(commit["result"]["isError"], true);
    assert_eq!(commit["result"]["structuredContent"]["code"], "unsupported");

    let invalid: Value = server
        .handle(json!({"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"memory_observe","arguments":{}}}))
        .await;
    assert_eq!(invalid["error"]["code"], -32602);
}
