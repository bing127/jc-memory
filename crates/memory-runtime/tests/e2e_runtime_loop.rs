//! 确定性模型、工具和 Runtime 状态机的完整循环验收。

use std::{
    collections::BTreeSet,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use async_trait::async_trait;
use chrono::{TimeZone as _, Utc};
use jc_memory_protocol::{AccessContext, Grant, MemoryScope, NamespaceId, Operation, TraceId};
use jc_memory_runtime::{
    ApprovalStore, ModelDescriptor, ModelError, ModelMessage, ModelOutput, ModelProvider,
    ModelRequest, ModelResponse, PredictionRelation, RiskLevel, RuntimeEvent, RuntimeRunner,
    RuntimeState, ToolDescriptor, ToolError, ToolExecutor, ToolIdempotency, ToolInvocation,
    ToolLoop, ToolRegistry, TransitionMetadata, parse_model_output,
};
use jc_memory_storage::{SqliteConfig, SqliteStore};
use serde_json::{Value, json};
use tempfile::tempdir;
use tokio_util::sync::CancellationToken;

struct DeterministicProvider;

#[async_trait]
impl ModelProvider for DeterministicProvider {
    async fn invoke(
        &self,
        request: ModelRequest,
        _cancellation: CancellationToken,
    ) -> Result<ModelResponse, ModelError> {
        let input = request
            .messages
            .last()
            .map_or_else(String::new, |message| message.content.clone());
        let raw_text = if input == "执行工具" {
            r#"{"kind":"tool_request","payload":{"capability_id":"fixture.tool","arguments":{"value":1}}}"#
        } else {
            r#"{"kind":"final_response","payload":{"text":"工具失败，保留否定证据"}}"#
        };
        Ok(ModelResponse {
            output: parse_model_output(
                serde_json::from_str(raw_text)
                    .map_err(|error| ModelError::InvalidResponse(error.to_string()))?,
            )
            .map_err(|error| ModelError::InvalidResponse(error.to_string()))?,
            raw_text: raw_text.into(),
        })
    }

    fn descriptor(&self) -> ModelDescriptor {
        ModelDescriptor {
            provider: "deterministic".into(),
            model: "fixture-v1".into(),
            supports_streaming: false,
        }
    }
}

struct FailingTool {
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl ToolExecutor for FailingTool {
    async fn execute(
        &self,
        _arguments: Value,
        _cancellation: CancellationToken,
    ) -> Result<Value, ToolError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Err(ToolError::Execution("fixture failure".into()))
    }
}

fn access(scope: &MemoryScope) -> AccessContext {
    let mut access = AccessContext::new("runtime-e2e", scope.clone());
    access
        .grants
        .insert(Grant::for_scope(Operation::Mutate, scope));
    access
}

fn metadata() -> TransitionMetadata {
    TransitionMetadata {
        input_refs: Vec::new(),
        output_refs: Vec::new(),
        attempt: 1,
        deadline: Utc.timestamp_micros(1_900_000_000_000_000).unwrap(),
        trace_id: TraceId::new(),
        payload: json!({"fixture":"deterministic-runtime-loop"}),
    }
}

#[tokio::test]
async fn deterministic_model_tool_failure_completes_without_fact_promotion() {
    let scope = MemoryScope::new(NamespaceId::new());
    let access = access(&scope);
    let provider = DeterministicProvider;
    let model_request = ModelRequest {
        messages: vec![ModelMessage::user("执行工具")],
        stream: false,
    };
    let model_response = provider
        .invoke(model_request, CancellationToken::new())
        .await
        .expect("确定性模型应返回 ToolRequest");
    assert!(matches!(model_response.output, ModelOutput::ToolRequest(_)));

    let calls = Arc::new(AtomicUsize::new(0));
    let registry = ToolRegistry::new();
    registry
        .register(
            ToolDescriptor {
                id: "fixture.tool".into(),
                version: "1.0.0".parse().unwrap(),
                permissions: BTreeSet::new(),
                risk: RiskLevel::Low,
                requires_approval: false,
                idempotency: ToolIdempotency::Idempotent,
                input_schema: json!({"type":"object"}),
                output_schema: json!({"type":"object"}),
            },
            Arc::new(FailingTool {
                calls: Arc::clone(&calls),
            }),
        )
        .unwrap();
    let commit = ToolLoop::new(registry, ApprovalStore::new())
        .execute(
            &access,
            ToolInvocation {
                id: "fixture.tool".into(),
                version: "^1".parse().unwrap(),
                arguments: json!({"value":1}),
                prediction: Some("测试会通过".into()),
            },
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(!commit.tool_result.success);
    assert_eq!(
        commit.prediction_relation,
        Some(PredictionRelation::DisprovedBy)
    );
    assert!(!commit.writes_fact);

    let store_dir = tempdir().unwrap();
    let store = SqliteStore::open(&SqliteConfig::new(store_dir.path().join("memory.sqlite3")))
        .await
        .unwrap();
    let runner = RuntimeRunner::from_store(&store);
    let mut snapshot = runner.start(&access, &scope).await.unwrap();
    for event in [
        RuntimeEvent::Recall,
        RuntimeEvent::Reason,
        RuntimeEvent::Execute,
        RuntimeEvent::Commit,
        RuntimeEvent::Complete,
    ] {
        snapshot = runner
            .transition(&access, &snapshot, event, &metadata())
            .await
            .unwrap();
    }
    assert_eq!(snapshot.state, RuntimeState::Completed);
    assert_eq!(snapshot.next_step, 5);

    let final_response = provider
        .invoke(
            ModelRequest {
                messages: vec![ModelMessage::user("工具已失败")],
                stream: false,
            },
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(matches!(
        final_response.output,
        ModelOutput::FinalResponse(_)
    ));
}
