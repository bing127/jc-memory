//! REST 与共享服务适配器的一致性测试。

use std::time::Duration;

use async_trait::async_trait;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt as _;
use jc_memory_core::{CommandBus, CommandError};
use jc_memory_protocol::{
    API_VERSION_V1, AccessContext, AccessScope, ExplainResult, MemoryCommand, MemoryPacket,
    MemoryRequest, MemoryResponse, MutationAction, MutationCommand, MutationResult, ObserveResult,
    Operation, RecallCommand, RecallId, RecallLimits, RecallMode, ResponseEnvelope, StatsCommand,
    StatsResult,
};
use jc_memory_server::{ApiKey, GrpcAdapter, ServerError, ServerState, router, serve_grpc};
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

#[derive(Clone)]
struct SlowBus;

#[async_trait]
impl CommandBus for SlowBus {
    async fn execute(
        &self,
        access: &AccessContext,
        request: MemoryRequest,
    ) -> Result<ResponseEnvelope, CommandError> {
        tokio::time::sleep(Duration::from_millis(50)).await;
        FixtureBus.execute(access, request).await
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
                MemoryCommand::Observe(_)
                | MemoryCommand::Stats(_)
                | MemoryCommand::Correct(_)
                | MemoryCommand::Forget(_)
                | MemoryCommand::Explain(_) => {
                    panic!("测试只接受 Recall 命令")
                }
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
async fn stats_route_returns_scope_counts_from_the_shared_envelope() {
    #[derive(Clone)]
    struct StatsBus;
    #[async_trait]
    impl CommandBus for StatsBus {
        async fn execute(
            &self,
            _access: &AccessContext,
            request: MemoryRequest,
        ) -> Result<ResponseEnvelope, CommandError> {
            match request.command {
                MemoryCommand::Stats(StatsCommand) => Ok(ResponseEnvelope::for_request(
                    &request,
                    MemoryResponse::Stats(StatsResult {
                        scope: request.access_scope.clone(),
                        fragment_count: 2,
                        event_count: 3,
                    }),
                )),
                MemoryCommand::Observe(_)
                | MemoryCommand::Recall(_)
                | MemoryCommand::Correct(_)
                | MemoryCommand::Forget(_)
                | MemoryCommand::Explain(_) => {
                    panic!("测试只接受 Stats 命令")
                }
            }
        }
    }

    let mut request = request();
    request.command = MemoryCommand::Stats(StatsCommand);
    let key = ApiKey::new(
        "test-key",
        AccessContext::new("principal", request.access_scope.clone().into()),
    );
    let response = router(ServerState::new(StatsBus, vec![key]))
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/memory/stats")
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
    assert_eq!(envelope["response"]["response"], "stats");
    assert_eq!(envelope["response"]["payload"]["fragment_count"], 2);
}

#[tokio::test]
async fn scoped_api_key_builds_grants_only_on_the_server() {
    #[derive(Clone)]
    struct GrantCheckingBus;
    #[async_trait]
    impl CommandBus for GrantCheckingBus {
        async fn execute(
            &self,
            access: &AccessContext,
            request: MemoryRequest,
        ) -> Result<ResponseEnvelope, CommandError> {
            assert!(
                access
                    .grants
                    .iter()
                    .any(|grant| grant.operation == Operation::Observe)
            );
            assert!(
                access
                    .grants
                    .iter()
                    .any(|grant| grant.operation == Operation::Recall)
            );
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

    let request = request();
    let scope = request.access_scope.clone().into();
    let key = ApiKey::for_scope(
        "test-key",
        "principal",
        scope,
        [Operation::Observe, Operation::Recall],
    );
    let response = router(ServerState::new(GrantCheckingBus, vec![key]))
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
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn mutation_routes_return_correct_and_forget_results() {
    #[derive(Clone)]
    struct MutationBus;
    #[async_trait]
    impl CommandBus for MutationBus {
        async fn execute(
            &self,
            _access: &AccessContext,
            request: MemoryRequest,
        ) -> Result<ResponseEnvelope, CommandError> {
            let action = match request.command {
                MemoryCommand::Correct(_) => MutationAction::Correct,
                MemoryCommand::Forget(_) => MutationAction::Forget,
                MemoryCommand::Observe(_)
                | MemoryCommand::Recall(_)
                | MemoryCommand::Stats(_)
                | MemoryCommand::Explain(_) => {
                    panic!("测试只接受变更命令")
                }
            };
            Ok(ResponseEnvelope::for_request(
                &request,
                MemoryResponse::Mutation(MutationResult {
                    action,
                    target_fragment_id: "00000000-0000-7000-8000-000000000004".parse().unwrap(),
                    fragment_id: "00000000-0000-7000-8000-000000000005".parse().unwrap(),
                    event_id: "00000000-0000-7000-8000-000000000006".parse().unwrap(),
                    sequence: 2,
                    relation: None,
                }),
            ))
        }
    }

    let command = serde_json::from_value::<MutationCommand>(json!({
        "target_fragment_id": "00000000-0000-7000-8000-000000000004",
        "content": {"type": "text", "text": "fixture correction"},
        "source": {
            "source_type": "user",
            "source_id": "fixture",
            "actor_id": null,
            "observer_id": null,
            "tool_id": null,
            "uri": null
        },
        "perspective": "first_person",
        "occurred_at": "2026-07-31T00:00:00Z",
        "verification_method": "manual",
        "reason": "fixture mutation"
    }))
    .unwrap();
    for (path, command_name, expected_action) in [
        ("/v1/memory/correct", "correct", "correct"),
        ("/v1/memory/forget", "forget", "forget"),
    ] {
        let mut request = request();
        request.command = match command_name {
            "correct" => MemoryCommand::Correct(command.clone()),
            "forget" => MemoryCommand::Forget(command.clone()),
            _ => unreachable!(),
        };
        let key = ApiKey::new(
            "test-key",
            AccessContext::new("principal", request.access_scope.clone().into()),
        );
        let response = router(ServerState::new(MutationBus, vec![key]))
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(path)
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
        assert_eq!(envelope["response"]["response"], "mutation");
        assert_eq!(envelope["response"]["payload"]["action"], expected_action);
    }
}

#[tokio::test]
async fn explain_route_returns_structured_trace_envelope() {
    #[derive(Clone)]
    struct ExplainBus;
    #[async_trait]
    impl CommandBus for ExplainBus {
        async fn execute(
            &self,
            _access: &AccessContext,
            request: MemoryRequest,
        ) -> Result<ResponseEnvelope, CommandError> {
            match request.command {
                MemoryCommand::Explain(_) => Ok(ResponseEnvelope::for_request(
                    &request,
                    MemoryResponse::Explain(ExplainResult::default()),
                )),
                MemoryCommand::Observe(_)
                | MemoryCommand::Recall(_)
                | MemoryCommand::Stats(_)
                | MemoryCommand::Correct(_)
                | MemoryCommand::Forget(_) => panic!("测试只接受 Explain 命令"),
            }
        }
    }

    let mut request = request();
    request.command = MemoryCommand::Explain(RecallCommand {
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
    let response = router(ServerState::new(ExplainBus, vec![key]))
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/memory/explain")
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
    assert_eq!(envelope["response"]["response"], "explain");
    assert_eq!(envelope["response"]["payload"]["truncated"], false);
}

#[tokio::test]
async fn tonic_execute_reuses_the_shared_adapter_over_tcp() {
    use jc_memory_protocol::generated::jiangcheng::memory::v1::memory_service_client::MemoryServiceClient;

    let request = request();
    let key = ApiKey::new(
        "test-key",
        AccessContext::new("principal", request.access_scope.clone().into()),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let shutdown = CancellationToken::new();
    let task = tokio::spawn(serve_grpc(
        listener,
        ServerState::new(FixtureBus, vec![key]),
        shutdown.clone().cancelled_owned(),
    ));

    let endpoint = tonic::transport::Endpoint::from_shared(format!("http://{address}"))
        .unwrap()
        .connect_timeout(std::time::Duration::from_secs(2));
    let mut client = MemoryServiceClient::connect(endpoint).await.unwrap();
    let wire_request: jc_memory_protocol::generated::jiangcheng::memory::v1::RequestEnvelope =
        request.clone().try_into().unwrap();
    let mut grpc_request = tonic::Request::new(wire_request);
    grpc_request
        .metadata_mut()
        .insert("x-api-key", "test-key".parse().unwrap());
    let response = client.execute(grpc_request).await.unwrap();
    let domain: ResponseEnvelope = response.into_inner().try_into().unwrap();
    assert_eq!(domain.request_id, request.request_id);
    assert!(matches!(domain.response, MemoryResponse::Observe(_)));

    shutdown.cancel();
    task.await.unwrap().unwrap();
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
async fn grpc_adapter_obeys_the_same_concurrency_limit_as_http() {
    let request = request();
    let key = ApiKey::new(
        "test-key",
        AccessContext::new("principal", request.access_scope.clone().into()),
    );
    let adapter = GrpcAdapter::new(ServerState::with_max_in_flight(FixtureBus, vec![key], 0));

    let error = adapter.execute("test-key", request).await.unwrap_err();

    assert!(matches!(error, ServerError::RateLimited));
}

#[tokio::test]
async fn http_rejects_requests_above_the_api_key_rate_limit() {
    let request = request();
    let key = ApiKey::new(
        "test-key",
        AccessContext::new("principal", request.access_scope.clone().into()),
    );
    let state = ServerState::with_request_limits(FixtureBus, vec![key], 64, 1);

    for expected in [StatusCode::OK, StatusCode::TOO_MANY_REQUESTS] {
        let response = router(state.clone())
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
        assert_eq!(response.status(), expected);
    }
}

#[tokio::test]
async fn grpc_rate_limit_is_isolated_by_api_key() {
    let request = request();
    let scope: jc_memory_protocol::MemoryScope = request.access_scope.clone().into();
    let state = ServerState::with_request_limits(
        FixtureBus,
        [
            ApiKey::new("first-key", AccessContext::new("first", scope.clone())),
            ApiKey::new("second-key", AccessContext::new("second", scope)),
        ],
        64,
        1,
    );
    let adapter = GrpcAdapter::new(state);

    adapter.execute("first-key", request.clone()).await.unwrap();
    let error = adapter
        .execute("first-key", request.clone())
        .await
        .unwrap_err();
    adapter.execute("second-key", request).await.unwrap();

    assert!(matches!(error, ServerError::RateLimited));
}

#[tokio::test]
async fn http_returns_gateway_timeout_when_command_exceeds_deadline() {
    let request = request();
    let key = ApiKey::new(
        "test-key",
        AccessContext::new("principal", request.access_scope.clone().into()),
    );
    let state = ServerState::with_request_limits_and_timeout(SlowBus, vec![key], 64, 128, 5);

    let response = router(state)
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

    assert_eq!(response.status(), StatusCode::GATEWAY_TIMEOUT);
}

#[tokio::test]
async fn grpc_returns_deadline_exceeded_when_command_exceeds_deadline() {
    let request = request();
    let key = ApiKey::new(
        "test-key",
        AccessContext::new("principal", request.access_scope.clone().into()),
    );
    let adapter = GrpcAdapter::new(ServerState::with_request_limits_and_timeout(
        SlowBus,
        vec![key],
        64,
        128,
        5,
    ));

    let error = adapter.execute("test-key", request).await.unwrap_err();

    assert!(matches!(error, ServerError::Timeout { .. }));
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
