//! 真实 Server 二进制的启动、持久化和优雅重启验收。
#![cfg(unix)]

use std::{
    net::{SocketAddr, TcpListener as StdTcpListener},
    path::Path,
    process::{Child, Command, ExitStatus, Stdio},
    time::Duration,
};

use jc_memory_protocol::{
    API_VERSION_V1, AccessScope, MemoryCommand, MemoryRequest, NamespaceId, StatsCommand,
};
use serde_json::{Value, json};

const API_KEY: &str = "sk-jcmemory-87s92kdp5vqz4mnh3cr0xfbj6gwy1aue7t";
const NAMESPACE_ID: &str = "00000000-0000-7000-8000-000000000003";

struct ServerProcess {
    child: Option<Child>,
}

impl ServerProcess {
    fn spawn(data_dir: &Path, http: SocketAddr, grpc: SocketAddr) -> Self {
        let child = Command::new(env!("CARGO_BIN_EXE_jc-memory-server"))
            .env("JC_MEMORY_BIND", http.to_string())
            .env("JC_MEMORY_GRPC_BIND", grpc.to_string())
            .env("JC_MEMORY_DATA_DIR", data_dir)
            .env("JC_MEMORY_API_KEY", API_KEY)
            .env("JC_MEMORY_NAMESPACE_ID", NAMESPACE_ID)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("启动真实 Server 二进制");
        Self { child: Some(child) }
    }

    async fn stop(&mut self) {
        let mut child = self.child.take().expect("Server 进程仍在运行");
        let status = Command::new("kill")
            .arg("-INT")
            .arg(child.id().to_string())
            .status()
            .expect("向 Server 发送 SIGINT");
        if !status.success() {
            terminate(&mut child);
            panic!("SIGINT 发送失败");
        }
        match tokio::time::timeout(Duration::from_secs(2), wait_for_exit(&mut child)).await {
            Ok(Ok(status)) => assert!(status.success(), "Server 未干净退出: {status}"),
            Ok(Err(error)) => {
                terminate(&mut child);
                panic!("等待 Server 退出失败: {error}");
            }
            Err(_) => {
                terminate(&mut child);
                panic!("Server 未在优雅关闭期限内退出");
            }
        }
    }

    fn try_wait(&mut self) -> std::io::Result<Option<ExitStatus>> {
        self.child.as_mut().expect("Server 进程仍在运行").try_wait()
    }
}

impl Drop for ServerProcess {
    fn drop(&mut self) {
        if let Some(child) = &mut self.child {
            terminate(child);
        }
    }
}

#[tokio::test]
async fn real_server_binary_persists_authority_across_clean_restart() {
    let directory = tempfile::tempdir().expect("创建二进制 E2E 数据目录");
    let (mut first, http) = start_ready_server(directory.path()).await;

    let observe = observe_request();
    let (status, response) = http_json(http, "POST", "/v1/memory/observe", Some(&observe)).await;
    assert_eq!(status, 200);
    assert_eq!(response["response"]["response"], "observe");
    first.stop().await;

    let (mut restarted, http) = start_ready_server(directory.path()).await;
    let stats_body = serde_json::to_value(stats_request()).expect("序列化 Stats 请求");
    let (status, response) = http_json(http, "POST", "/v1/memory/stats", Some(&stats_body)).await;
    assert_eq!(status, 200);
    assert_eq!(response["response"]["response"], "stats");
    assert_eq!(response["response"]["payload"]["fragment_count"], 1);
    assert_eq!(response["response"]["payload"]["event_count"], 1);
    restarted.stop().await;
}

#[tokio::test]
async fn real_server_binary_returns_non_empty_current_and_historical_state() {
    let directory = tempfile::tempdir().expect("创建二进制 State E2E 数据目录");
    let (mut server, http) = start_ready_server(directory.path()).await;
    let blue_fragment_id = "00000000-0000-7000-8000-000000000311";
    let canary_evidence_id = "00000000-0000-7000-8000-000000000351";

    let (status, blue) = http_json(
        http,
        "POST",
        "/v1/memory/observe",
        Some(&state_observe_request(
            "binary-state-blue",
            blue_fragment_id,
            "blue_green",
            "生产环境先使用蓝绿发布",
            "2026-07-30T00:00:00Z",
            None,
        )),
    )
    .await;
    assert_eq!(status, 200);
    let blue_event_id = blue["response"]["payload"]["event_id"]
        .as_str()
        .expect("blue_green Observe 返回 Event ID");
    let (status, canary) = http_json(
        http,
        "POST",
        "/v1/memory/observe",
        Some(&state_observe_request(
            "binary-state-canary",
            "00000000-0000-7000-8000-000000000312",
            "canary",
            "验证通过后切换为金丝雀发布",
            "2026-07-31T00:00:00Z",
            Some((canary_evidence_id, blue_fragment_id)),
        )),
    )
    .await;
    assert_eq!(status, 200);
    let canary_event_id = canary["response"]["payload"]["event_id"]
        .as_str()
        .expect("canary Observe 返回 Event ID");
    let (status, recalled) = http_json(
        http,
        "POST",
        "/v1/memory/recall",
        Some(&state_recall_request()),
    )
    .await;
    assert_eq!(status, 200);
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
    server.stop().await;
}

async fn start_ready_server(data_dir: &Path) -> (ServerProcess, SocketAddr) {
    let mut last_error = String::new();
    for _ in 0..5 {
        let (http_listener, grpc_listener) = available_listeners();
        let http = http_listener.local_addr().expect("读取 HTTP 测试端口");
        let grpc = grpc_listener.local_addr().expect("读取 gRPC 测试端口");
        drop(http_listener);
        drop(grpc_listener);
        let mut process = ServerProcess::spawn(data_dir, http, grpc);
        match wait_until_ready(&mut process, http).await {
            Ok(()) => return (process, http),
            Err(error) => {
                last_error = error;
                if let Some(child) = &mut process.child {
                    terminate(child);
                }
                process.child = None;
            }
        }
    }
    panic!("真实 Server 二进制启动重试耗尽: {last_error}");
}

fn available_listeners() -> (StdTcpListener, StdTcpListener) {
    let http = StdTcpListener::bind("127.0.0.1:0").expect("分配 HTTP 测试端口");
    let grpc = StdTcpListener::bind("127.0.0.1:0").expect("分配 gRPC 测试端口");
    (http, grpc)
}

async fn wait_until_ready(process: &mut ServerProcess, address: SocketAddr) -> Result<(), String> {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if let Some(status) = process.try_wait().map_err(|error| error.to_string())? {
                return Err(format!("Server 在就绪前退出: {status}"));
            }
            if let Ok((200, _)) = try_http_json(address, "GET", "/healthz", None).await {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .map_err(|_| "Server 未在期限内就绪".to_owned())?
}

async fn http_json(
    address: SocketAddr,
    method: &str,
    path: &str,
    body: Option<&Value>,
) -> (u16, Value) {
    try_http_json(address, method, path, body)
        .await
        .expect("真实 Server HTTP 请求成功")
}

async fn try_http_json(
    address: SocketAddr,
    method: &str,
    path: &str,
    body: Option<&Value>,
) -> Result<(u16, Value), Box<dyn std::error::Error>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(1))
        .build()?;
    let url = format!("http://{address}{path}");
    let mut request = match method {
        "GET" => client.get(url),
        "POST" => client.post(url),
        _ => return Err(format!("不支持的测试 HTTP 方法: {method}").into()),
    }
    .header("x-api-key", API_KEY);
    if let Some(body) = body {
        request = request.json(body);
    }
    let response = request.send().await?;
    let status = response.status().as_u16();
    let body = response.json::<Value>().await?;
    Ok((status, body))
}

async fn wait_for_exit(child: &mut Child) -> std::io::Result<ExitStatus> {
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn terminate(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn observe_request() -> Value {
    json!({
        "api_version": API_VERSION_V1,
        "request_id": "binary-e2e-observe",
        "access_scope": scope_json(),
        "command": {
            "command": "observe",
            "payload": {
                "fragment": {
                    "id": "00000000-0000-7000-8000-000000000004",
                    "scope": scope_json(),
                    "session_id": null,
                    "trace_id": null,
                    "kind": "observation",
                    "content": {"type": "text", "text": "真实二进制重启持久化"},
                    "source": {
                        "source_type": "user",
                        "source_id": "binary-e2e",
                        "actor_id": null,
                        "observer_id": null,
                        "tool_id": null,
                        "uri": null
                    },
                    "perspective": "first_person",
                    "occurred_at": "2026-07-31T00:00:00Z",
                    "observed_at": "2026-07-31T00:00:00Z",
                    "confidence": 1.0,
                    "importance": 0.5,
                    "validity": "unverified",
                    "security_label": "internal",
                    "evidence": [],
                    "metadata": {}
                }
            }
        }
    })
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
        "api_version": API_VERSION_V1,
        "request_id": request_id,
        "access_scope": scope_json(),
        "command": {
            "command": "observe",
            "payload": {
                "fragment": {
                    "id": fragment_id,
                    "scope": scope_json(),
                    "session_id": null,
                    "trace_id": null,
                    "kind": "fact",
                    "content": {"type": "text", "text": format!("部署策略 {value}")},
                    "source": {
                        "source_type": "user",
                        "source_id": "binary-state-e2e",
                        "actor_id": null,
                        "observer_id": null,
                        "tool_id": null,
                        "uri": null
                    },
                    "perspective": "first_person",
                    "occurred_at": occurred_at,
                    "observed_at": occurred_at,
                    "confidence": 1.0,
                    "importance": 0.5,
                    "validity": "unverified",
                    "security_label": "internal",
                    "evidence": evidence,
                    "metadata": {"state": {
                        "key": "project.deployment_strategy",
                        "value": value,
                        "reason": reason
                    }}
                }
            }
        }
    })
}

fn state_recall_request() -> Value {
    json!({
        "api_version": API_VERSION_V1,
        "request_id": "binary-state-recall",
        "access_scope": scope_json(),
        "command": {
            "command": "recall",
            "payload": {
                "mode": "current_state",
                "text": "project.deployment_strategy",
                "entities": [],
                "at_time": null,
                "limits": {
                    "max_seeds": 4,
                    "max_neighbors": 4,
                    "max_hops": 2,
                    "max_nodes": 8,
                    "timeout_ms": 500
                }
            }
        }
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

fn stats_request() -> MemoryRequest {
    MemoryRequest {
        api_version: API_VERSION_V1.into(),
        request_id: "binary-e2e-stats".into(),
        access_scope: AccessScope {
            namespace_id: NAMESPACE_ID.parse::<NamespaceId>().expect("解析 Namespace"),
            tenant_id: None,
            user_id: None,
            agent_id: None,
            project_id: None,
        },
        command: MemoryCommand::Stats(StatsCommand),
    }
}

fn scope_json() -> Value {
    json!({
        "namespace_id": NAMESPACE_ID,
        "tenant_id": null,
        "user_id": null,
        "agent_id": null,
        "project_id": null
    })
}
