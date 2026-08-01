//! Runtime Run 与 Transition 的 HTTP 只读契约测试。

use async_trait::async_trait;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use chrono::{Duration, Utc};
use http_body_util::BodyExt as _;
use jc_memory_core::{CommandBus, CommandError};
use jc_memory_protocol::{
    AccessContext, CommandErrorCode, Grant, MemoryRequest, MemoryScope, NamespaceId, Operation,
    ResponseEnvelope, TraceId,
};
use jc_memory_runtime::{RuntimeEvent, RuntimeReadStore, RuntimeRunner, TransitionMetadata};
use jc_memory_server::{ApiKey, ServerState, router};
use jc_memory_storage::{SqliteConfig, SqliteStore};
use serde_json::{Value, json};
use tempfile::tempdir;
use tower::ServiceExt as _;

#[derive(Clone)]
struct UnusedBus;

#[async_trait]
impl CommandBus for UnusedBus {
    async fn execute(
        &self,
        _access: &AccessContext,
        _request: MemoryRequest,
    ) -> Result<ResponseEnvelope, CommandError> {
        Err(CommandError::transport(
            CommandErrorCode::Internal,
            "unused",
        ))
    }
}

#[tokio::test]
async fn list_and_detail_are_bound_to_the_api_key_scope() {
    let directory = tempdir().unwrap();
    let store = SqliteStore::open(&SqliteConfig::new(directory.path().join("memory.db")))
        .await
        .unwrap();
    let scope = MemoryScope::new(NamespaceId::new());
    let other_scope = MemoryScope::new(NamespaceId::new());
    let mut access = AccessContext::new("runtime-api", scope.clone());
    access.grants = [Operation::Mutate, Operation::Recall]
        .into_iter()
        .map(|operation| Grant::for_scope(operation, &scope))
        .collect();
    let runner = RuntimeRunner::from_store(&store);
    let started = runner.start(&access, &scope).await.unwrap();
    let recalled = runner
        .transition(
            &access,
            &started,
            RuntimeEvent::Recall,
            &TransitionMetadata {
                input_refs: Vec::new(),
                output_refs: Vec::new(),
                attempt: 1,
                deadline: Utc::now() + Duration::minutes(1),
                trace_id: TraceId::new(),
                payload: json!({"phase":"observe"}),
            },
        )
        .await
        .unwrap();
    let mut other_access = AccessContext::new("other", other_scope.clone());
    other_access
        .grants
        .insert(Grant::for_scope(Operation::Mutate, &other_scope));
    let other = runner.start(&other_access, &other_scope).await.unwrap();
    let app = router(
        ServerState::new(UnusedBus, [ApiKey::new("runtime-key", access)])
            .with_runtime_read_store(RuntimeReadStore::from_store(&store)),
    );

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/runtime/runs?state=recalling&limit=10&offset=0")
                .header("x-api-key", "runtime-key")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let page: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(page["total"], 1);
    assert_eq!(page["items"][0]["run_id"], recalled.run_id.to_string());
    assert!(!String::from_utf8_lossy(&body).contains(&other.run_id.to_string()));

    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/v1/runtime/runs/{}", recalled.run_id))
                .header("x-api-key", "runtime-key")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let detail: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(detail["run"]["run_id"], recalled.run_id.to_string());
    assert_eq!(detail["transitions"][0]["step"], 0);
    assert_eq!(detail["transitions"][0]["payload"]["phase"], "observe");
}

#[tokio::test]
async fn runtime_routes_require_authentication_and_configured_store() {
    let scope = MemoryScope::new(NamespaceId::new());
    let access = AccessContext::new("runtime-api", scope);
    let app = router(ServerState::new(
        UnusedBus,
        [ApiKey::new("runtime-key", access)],
    ));

    let unauthorized = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/runtime/runs")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

    let unavailable = app
        .oneshot(
            Request::builder()
                .uri("/v1/runtime/runs")
                .header("x-api-key", "runtime-key")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unavailable.status(), StatusCode::SERVICE_UNAVAILABLE);
}
