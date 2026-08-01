//! Runtime Start HTTP 契约测试。

use std::sync::Arc;

use async_trait::async_trait;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt as _;
use jc_memory_core::{CommandBus, CommandError};
use jc_memory_protocol::{
    AccessContext, Grant, MemoryRequest, MemoryScope, NamespaceId, Operation, ResponseEnvelope,
};
use jc_memory_runtime::{
    ApprovalStore, FinalResponse, ModelDescriptor, ModelError, ModelOutput, ModelProvider,
    ModelRequest, ModelResponse, RuntimeBackend, RuntimeCommit, RuntimeReadStore, RuntimeRecall,
    RuntimeRunner, RuntimeService, ToolLoop, ToolRegistry,
};
use jc_memory_server::{ApiKey, ServerState, router};
use jc_memory_storage::{SqliteConfig, SqliteStore};
use serde_json::{Value, json};
use tempfile::tempdir;
use tokio_util::sync::CancellationToken;
use tower::ServiceExt as _;

struct UnusedBus;

#[async_trait]
impl CommandBus for UnusedBus {
    async fn execute(
        &self,
        _access: &AccessContext,
        _request: MemoryRequest,
    ) -> Result<ResponseEnvelope, CommandError> {
        panic!("memory command bus is not used by this fixture")
    }
}

struct FinalProvider;

#[async_trait]
impl ModelProvider for FinalProvider {
    async fn invoke(
        &self,
        _request: ModelRequest,
        _cancellation: CancellationToken,
    ) -> Result<ModelResponse, ModelError> {
        Ok(ModelResponse {
            output: ModelOutput::FinalResponse(FinalResponse {
                text: "运行完成".into(),
            }),
            raw_text: "fixture".into(),
        })
    }

    fn descriptor(&self) -> ModelDescriptor {
        ModelDescriptor {
            provider: "fixture".into(),
            model: "final-v1".into(),
            supports_streaming: false,
        }
    }
}

struct EmptyBackend;

#[async_trait]
impl RuntimeBackend for EmptyBackend {
    async fn observe(
        &self,
        _: &AccessContext,
        _: &MemoryScope,
        _: &str,
    ) -> Result<Vec<jc_memory_protocol::EvidenceRef>, String> {
        Ok(Vec::new())
    }
    async fn recall(
        &self,
        _: &AccessContext,
        _: &MemoryScope,
        _: &str,
    ) -> Result<RuntimeRecall, String> {
        Ok(RuntimeRecall {
            recommended_context: "{}".into(),
            transition_payload: json!({}),
            evidence_refs: Vec::new(),
        })
    }
    async fn commit(
        &self,
        _: &AccessContext,
        _: &MemoryScope,
        _: RuntimeCommit,
    ) -> Result<Vec<jc_memory_protocol::EvidenceRef>, String> {
        Ok(Vec::new())
    }
}

#[tokio::test]
async fn starts_completed_run_in_api_key_scope_and_rejects_client_scope() {
    let directory = tempdir().unwrap();
    let store = SqliteStore::open(&SqliteConfig::new(directory.path().join("memory.sqlite3")))
        .await
        .unwrap();
    let scope = MemoryScope::new(NamespaceId::new());
    let mut access = AccessContext::new("runtime-start", scope.clone());
    access
        .grants
        .insert(Grant::for_scope(Operation::Mutate, &scope));
    access
        .grants
        .insert(Grant::for_scope(Operation::Recall, &scope));
    let service = RuntimeService::new(
        RuntimeRunner::from_store(&store),
        Arc::new(FinalProvider),
        Arc::new(EmptyBackend),
        ToolLoop::new(ToolRegistry::default(), ApprovalStore::default()),
    );
    let app = router(
        ServerState::new(UnusedBus, [ApiKey::new("runtime-key", access)])
            .with_runtime_read_store(RuntimeReadStore::from_store(&store))
            .with_runtime_service(service),
    );

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/runtime/runs")
                .header("x-api-key", "runtime-key")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"prompt":"请总结"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let body: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(body["outcome"], "completed");
    assert_eq!(body["state"], "completed");
    assert_eq!(body["response"]["text"], "运行完成");

    let rejected = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/runtime/runs")
                .header("x-api-key", "runtime-key")
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"prompt":"x","scope":{{"namespace_id":"{}"}}}}"#,
                    NamespaceId::new()
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(rejected.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn returns_unavailable_without_runtime_service() {
    let scope = MemoryScope::new(NamespaceId::new());
    let access = AccessContext::new("runtime-start", scope);
    let app = router(ServerState::new(
        UnusedBus,
        [ApiKey::new("runtime-key", access)],
    ));
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/runtime/runs")
                .header("x-api-key", "runtime-key")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"prompt":"请总结"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}
