//! MCP initialize、工具清单和结构化调用契约测试。

use std::sync::Arc;

use async_trait::async_trait;
use jc_memory_core::{CommandBus, CommandError, EmbeddedCommandBus, MemoryConfig, MemoryEngine};
use jc_memory_mcp::{McpServer, SidecarCommandBus};
use jc_memory_protocol::{
    AccessContext, CommitResult, MemoryCommand, MemoryRequest, MemoryResponse, ObserveResult,
    ResponseEnvelope,
};
use jc_memory_sidecar::{DaemonCommand, SidecarConfig, SidecarDaemon, TokenStore, send_command};
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
        let response = match request.command {
            MemoryCommand::Commit(_) => MemoryResponse::Commit(CommitResult {
                receipts: Vec::new(),
            }),
            _ => MemoryResponse::Observe(ObserveResult {
                fragment_id: "00000000-0000-7000-8000-000000000001".parse().unwrap(),
                event_id: "00000000-0000-7000-8000-000000000002".parse().unwrap(),
                sequence: 1,
                content_hash: "a".repeat(64).parse().unwrap(),
                created: true,
                quarantined: false,
            }),
        };
        Ok(ResponseEnvelope::for_request(&request, response))
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
    for tool in tools {
        let required = tool["inputSchema"]["properties"]["request"]["required"]
            .as_array()
            .expect("工具必须声明请求必填字段");
        assert!(required.iter().any(|field| field == "access_scope"));
        assert!(required.iter().any(|field| field == "command"));
        assert_eq!(
            tool["inputSchema"]["properties"]["request"]["properties"]["command"]["properties"]["command"]
                ["const"],
            tool["name"]
                .as_str()
                .unwrap()
                .strip_prefix("memory_")
                .unwrap()
        );
    }
}

#[tokio::test]
async fn tools_call_returns_structured_content_for_observe_and_commit() {
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

    let mut commit_request = request.clone();
    commit_request["command"] = json!({
        "command":"commit",
        "payload":{"fragments":[request["command"]["payload"]["fragment"].clone()]}
    });
    let commit = server
        .handle(json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"memory_commit","arguments":{"request":commit_request}}}))
        .await;
    assert_eq!(commit["result"]["isError"], false);
    assert_eq!(
        commit["result"]["structuredContent"]["response"]["response"],
        "commit"
    );

    let mismatch = server
        .handle(json!({
            "jsonrpc":"2.0","id":9,"method":"tools/call",
            "params":{"name":"memory_stats","arguments":{"request":request}}
        }))
        .await;
    assert_eq!(mismatch["error"]["code"], -32602);

    let invalid: Value = server
        .handle(json!({"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"memory_observe","arguments":{}}}))
        .await;
    assert_eq!(invalid["error"]["code"], -32602);
}

#[tokio::test]
async fn mcp_recall_preserves_non_empty_current_and_historical_state() {
    let directory = tempfile::tempdir().expect("创建 MCP State 目录");
    let engine = MemoryEngine::open(MemoryConfig::local(directory.path()))
        .await
        .expect("打开 MCP State 引擎");
    let server = McpServer::new(EmbeddedCommandBus::new(engine));
    let blue_fragment_id = "00000000-0000-7000-8000-000000000211";
    let canary_evidence_id = "00000000-0000-7000-8000-000000000251";

    let blue = mcp_call(
        &server,
        "memory_observe",
        state_observe_request(
            "mcp-state-blue",
            blue_fragment_id,
            "blue_green",
            "生产环境先使用蓝绿发布",
            "2026-07-30T00:00:00Z",
            None,
        ),
    )
    .await;
    let canary = mcp_call(
        &server,
        "memory_observe",
        state_observe_request(
            "mcp-state-canary",
            "00000000-0000-7000-8000-000000000212",
            "canary",
            "验证通过后切换为金丝雀发布",
            "2026-07-31T00:00:00Z",
            Some((canary_evidence_id, blue_fragment_id)),
        ),
    )
    .await;
    let blue_event_id = blue["response"]["payload"]["event_id"]
        .as_str()
        .expect("blue_green Observe 返回 Event ID");
    let canary_event_id = canary["response"]["payload"]["event_id"]
        .as_str()
        .expect("canary Observe 返回 Event ID");
    let recalled = mcp_call(&server, "memory_recall", state_recall_request()).await;
    let packet = &recalled["response"]["payload"];

    assert_eq!(
        packet["current_states"][0]["key"],
        "project.deployment_strategy"
    );
    assert_eq!(packet["current_states"][0]["value"], "canary");
    assert_eq!(packet["historical_states"][0]["value"], "blue_green");
    assert_eq!(
        packet["current_states"][0]["reason"],
        "验证通过后切换为金丝雀发布"
    );
    assert_eq!(
        packet["current_states"][0]["source_event_id"],
        canary_event_id
    );
    assert_eq!(
        packet["historical_states"][0]["source_event_id"],
        blue_event_id
    );
    assert_eq!(
        packet["current_states"][0]["evidence"][0]["evidence_ref_id"],
        canary_evidence_id
    );
    assert_eq!(
        contract_state_values("recall-current-state.json"),
        json!(["canary", "blue_green"])
    );
}

#[tokio::test]
async fn configured_auth_token_is_required_for_tool_calls() {
    let server = McpServer::with_auth_token(FixtureBus, "mcp-fixture-token")
        .expect("创建带认证的 MCP Server");
    let request = json!({
        "api_version":"v1",
        "request_id":"mcp-auth-request",
        "access_scope":{"namespace_id":"00000000-0000-7000-8000-000000000003"},
        "command":{"command":"stats","payload":null}
    });
    let call = |auth_token: Option<&str>| {
        let mut params = json!({
            "name":"memory_stats",
            "arguments":{"request":request.clone()}
        });
        if let Some(auth_token) = auth_token {
            params["authToken"] = json!(auth_token);
        }
        params
    };

    let missing = server
        .handle(json!({
            "jsonrpc":"2.0","id":6,"method":"tools/call","params":call(None)
        }))
        .await;
    assert_eq!(missing["error"]["code"], -32001);

    let wrong = server
        .handle(json!({
            "jsonrpc":"2.0","id":7,"method":"tools/call","params":call(Some("wrong"))
        }))
        .await;
    assert_eq!(wrong["error"]["code"], -32001);

    let accepted = server
        .handle(json!({
            "jsonrpc":"2.0","id":8,"method":"tools/call","params":call(Some("mcp-fixture-token"))
        }))
        .await;
    assert_eq!(accepted["result"]["isError"], false);
}

#[tokio::test]
async fn mcp_can_forward_a_call_through_the_authenticated_sidecar() {
    let directory = tempfile::tempdir().expect("创建 MCP Sidecar 目录");
    let config = SidecarConfig::local(directory.path());
    let engine = MemoryEngine::open(MemoryConfig::local(directory.path()))
        .await
        .expect("打开 MCP Sidecar 引擎");
    let daemon =
        SidecarDaemon::bind_with_bus(config.clone(), Arc::new(EmbeddedCommandBus::new(engine)))
            .await
            .expect("绑定 MCP Sidecar");
    let token = daemon.token().clone();
    let task = tokio::spawn(daemon.run());
    let server = McpServer::new(SidecarCommandBus::new(config.clone(), token));
    let request = json!({
        "api_version":"v1",
        "request_id":"mcp-sidecar-stats",
        "access_scope":{"namespace_id":"00000000-0000-7000-8000-000000000003"},
        "command":{"command":"stats","payload":null}
    });
    let response = server
        .handle(json!({
            "jsonrpc":"2.0",
            "id":10,
            "method":"tools/call",
            "params":{"name":"memory_stats","arguments":{"request":request}}
        }))
        .await;
    assert_eq!(response["result"]["isError"], false);
    assert_eq!(
        response["result"]["structuredContent"]["response"]["response"],
        "stats"
    );
    send_command(
        &config,
        &TokenStore::load_or_create(&config.token_file)
            .await
            .unwrap(),
        DaemonCommand::Stop,
    )
    .await
    .expect("停止 MCP Sidecar");
    task.await
        .expect("等待 MCP Sidecar")
        .expect("MCP Sidecar 正常退出");
}

async fn mcp_call<B>(server: &McpServer<B>, tool: &str, request: Value) -> Value
where
    B: CommandBus + 'static,
{
    let response = server
        .handle(json!({
            "jsonrpc":"2.0",
            "id":11,
            "method":"tools/call",
            "params":{"name":tool,"arguments":{"request":request}}
        }))
        .await;
    assert_eq!(response["result"]["isError"], false, "{response}");
    response["result"]["structuredContent"].clone()
}

fn state_observe_request(
    request_id: &str,
    fragment_id: &str,
    value: &str,
    reason: &str,
    occurred_at: &str,
    evidence: Option<(&str, &str)>,
) -> Value {
    let evidence = evidence
        .into_iter()
        .map(|(id, fragment_id)| {
            json!({
                "id": id,
                "fragment_id": fragment_id,
                "event_id": null,
                "method": "manual",
                "note": "人工确认部署策略变更"
            })
        })
        .collect::<Vec<_>>();
    json!({
        "api_version":"v1",
        "request_id":request_id,
        "access_scope":{"namespace_id":"00000000-0000-7000-8000-000000000203"},
        "command":{"command":"observe","payload":{"fragment":{
            "id":fragment_id,
            "scope":{"namespace_id":"00000000-0000-7000-8000-000000000203"},
            "kind":"fact",
            "content":{"type":"text","text":format!("部署策略 {value}")},
            "source":{"source_type":"user","source_id":"mcp-state-contract"},
            "perspective":"first_person",
            "occurred_at":occurred_at,
            "observed_at":occurred_at,
            "confidence":1.0,
            "importance":0.5,
            "validity":"unverified",
            "security_label":"internal",
            "evidence":evidence,
            "metadata":{"state":{
                "key":"project.deployment_strategy",
                "value":value,
                "reason":reason
            }}
        }}}
    })
}

fn state_recall_request() -> Value {
    json!({
        "api_version":"v1",
        "request_id":"mcp-state-recall",
        "access_scope":{"namespace_id":"00000000-0000-7000-8000-000000000203"},
        "command":{"command":"recall","payload":{
            "mode":"current_state",
            "text":"project.deployment_strategy",
            "entities":[],
            "at_time":null,
            "limits":{"max_seeds":4,"max_neighbors":4,"max_hops":2,"max_nodes":8,"timeout_ms":500}
        }}
    })
}

fn contract_state_values(name: &str) -> Value {
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../sdk/contracts/fixtures")
        .join(name);
    let fixture: Value = serde_json::from_slice(&std::fs::read(source).unwrap()).unwrap();
    json!([
        fixture["response"]["response"]["payload"]["current_states"][0]["value"],
        fixture["response"]["response"]["payload"]["historical_states"][0]["value"],
    ])
}
