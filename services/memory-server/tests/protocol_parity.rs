//! REST 与共享服务适配器的一致性测试。

use async_trait::async_trait;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt as _;
use jc_memory_core::{CommandBus, CommandError};
use jc_memory_protocol::{
    API_VERSION_V1, AccessContext, AccessScope, MemoryCommand, MemoryRequest, MemoryResponse,
    ObserveResult, ResponseEnvelope,
};
use jc_memory_server::{ApiKey, GrpcAdapter, ServerState, router};
use serde_json::{Value, json};
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
