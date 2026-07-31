//! REST 与共享服务适配器的一致性测试。

use async_trait::async_trait;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt as _;
use jc_memory_core::{CommandBus, CommandError};
use jc_memory_protocol::{
    API_VERSION_V1, AccessContext, AccessScope, MemoryCommand, MemoryPacket, MemoryRequest,
    MemoryResponse, ObserveResult, RecallCommand, RecallId, RecallLimits, RecallMode,
    ResponseEnvelope,
};
use jc_memory_server::{ApiKey, GrpcAdapter, ServerState, router};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::TcpListener,
};
use tokio_util::sync::CancellationToken;
use tower::ServiceExt as _;

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

fn request() -> MemoryRequest {
    MemoryRequest {
        api_version: API_VERSION_V1.into(),
        request_id: "request-1".into(),
        access_scope: AccessScope {
            namespace_id: "00000000-0000-7000-8000-000000000003".parse().unwrap(),
            tenant_id: None,
            user_id: None,
            agent_id: None,
            project_id: None,
        },
        command: MemoryCommand::Observe(serde_json::from_value(json!({
            "fragment": {
                "id": "00000000-0000-7000-8000-000000000004",
                "scope": {"namespace_id": "00000000-0000-7000-8000-000000000003", "tenant_id": null, "user_id": null, "agent_id": null, "project_id": null},
                "session_id": null,
                "trace_id": null,
                "kind": "observation",
                "content": {"type": "text", "text": "fixture"},
                "source": {"source_type": "user", "source_id": "fixture", "actor_id": null, "observer_id": null, "tool_id": null, "uri": null},
                "perspective": "first_person",
                "occurred_at": "2026-07-31T00:00:00Z",
                "observed_at": "2026-07-31T00:00:00Z",
                "confidence": 0.9,
                "importance": 0.5,
                "validity": "unverified",
                "security_label": "internal",
                "evidence": [],
                "metadata": {}
            }
        })).unwrap()),
    }
}

#[tokio::test]
async fn rest_and_grpc_adapter_return_the_same_canonical_envelope() {
    let key = ApiKey::new(
        "test-key",
        AccessContext::new("principal", request().access_scope.clone().into()),
    );
    let state = ServerState::new(FixtureBus, vec![key]);
    let body = serde_json::to_vec(&request()).unwrap();
    let response = router(state.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/memory/observe")
                .header("content-type", "application/json")
                .header("x-api-key", "test-key")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let rest: Value = serde_json::from_slice(&bytes).unwrap();
    let grpc = GrpcAdapter::new(state)
        .execute("test-key", request())
        .await
        .unwrap();
    assert_eq!(rest, serde_json::to_value(grpc).unwrap());
}

#[tokio::test]
async fn generic_command_route_reuses_the_same_core_envelope() {
    let request = request();
    let key = ApiKey::new(
        "test-key",
        AccessContext::new("principal", request.access_scope.clone().into()),
    );
    let response = router(ServerState::new(FixtureBus, vec![key]))
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/memory/command")
                .header("content-type", "application/json")
                .header("x-api-key", "test-key")
                .body(Body::from(serde_json::to_vec(&request).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let envelope: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(envelope["request_id"], "request-1");
}

#[tokio::test]
async fn recall_route_returns_the_shared_recall_envelope() {
    #[derive(Clone)]
    struct RecallBus;
    #[async_trait]
    impl CommandBus for RecallBus {
        async fn execute(
            &self,
            _access: &AccessContext,
            request: MemoryRequest,
        ) -> Result<ResponseEnvelope, CommandError> {
            let query_id = match request.command {
                MemoryCommand::Recall(_) => RecallId::new(),
                MemoryCommand::Observe(_) => panic!("测试只接受 Recall 命令"),
            };
            Ok(ResponseEnvelope::for_request(
                &request,
                MemoryResponse::Recall(MemoryPacket::empty(query_id, RecallMode::Exact)),
            ))
        }
    }

    let mut request = request();
    request.command = MemoryCommand::Recall(RecallCommand {
        mode: RecallMode::Exact,
        text: "fixture".into(),
        entities: Vec::new(),
        at_time: None,
        limits: RecallLimits {
            max_seeds: 4,
            max_neighbors: 4,
            max_hops: 2,
            max_nodes: 8,
            timeout_ms: 100,
        },
    });
    let key = ApiKey::new(
        "test-key",
        AccessContext::new("principal", request.access_scope.clone().into()),
    );
    let response = router(ServerState::new(RecallBus, vec![key]))
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/memory/recall")
                .header("content-type", "application/json")
                .header("x-api-key", "test-key")
                .body(Body::from(serde_json::to_vec(&request).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let envelope: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(envelope["response"]["response"], "recall");
}

#[tokio::test]
async fn missing_api_key_is_rejected_without_calling_bus() {
    let state = ServerState::new(FixtureBus, Vec::new());
    let response = router(state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/memory/observe")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&request()).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn core_error_keeps_request_id_in_the_stable_error_body() {
    #[derive(Clone)]
    struct FailingBus;
    #[async_trait]
    impl CommandBus for FailingBus {
        async fn execute(
            &self,
            _access: &AccessContext,
            _request: MemoryRequest,
        ) -> Result<ResponseEnvelope, CommandError> {
            Err(CommandError::RequestIdMismatch)
        }
    }
    let request = request();
    let key = ApiKey::new(
        "test-key",
        AccessContext::new("principal", request.access_scope.clone().into()),
    );
    let response = router(ServerState::new(FailingBus, vec![key]))
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/memory/command")
                .header("content-type", "application/json")
                .header("x-api-key", "test-key")
                .body(Body::from(serde_json::to_vec(&request).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["code"], "invalid_argument");
    assert_eq!(body["request_id"], "request-1");
}

#[tokio::test]
async fn metrics_is_public_and_concurrency_limit_is_stable() {
    let metrics_response = router(ServerState::new(FixtureBus, Vec::new()))
        .oneshot(
            Request::builder()
                .uri("/metrics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(metrics_response.status(), StatusCode::OK);
    let metrics = metrics_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    assert!(String::from_utf8_lossy(&metrics).contains("jc_memory_server_ready 1"));

    let request = request();
    let key = ApiKey::new(
        "test-key",
        AccessContext::new("principal", request.access_scope.clone().into()),
    );
    let limited = router(ServerState::with_max_in_flight(FixtureBus, vec![key], 0))
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/memory/observe")
                .header("content-type", "application/json")
                .header("x-api-key", "test-key")
                .body(Body::from(serde_json::to_vec(&request).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(limited.status(), StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn serve_stops_accepting_after_graceful_shutdown_signal() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let shutdown = CancellationToken::new();
    let task = tokio::spawn(jc_memory_server::serve(
        listener,
        ServerState::new(FixtureBus, Vec::new()),
        shutdown.clone().cancelled_owned(),
    ));
    let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
    stream
        .write_all(b"GET /healthz HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut response = Vec::new();
    stream.read_to_end(&mut response).await.unwrap();
    assert!(String::from_utf8_lossy(&response).starts_with("HTTP/1.1 200"));
    shutdown.cancel();
    task.await.unwrap().unwrap();
}
