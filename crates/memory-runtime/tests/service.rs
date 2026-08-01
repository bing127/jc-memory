//! Runtime 服务门面编排契约测试。

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::{TimeZone as _, Utc};
use jc_memory_protocol::{AccessContext, Grant, MemoryScope, NamespaceId, Operation};
use jc_memory_runtime::{
    ApprovalStore, FinalResponse, ModelDescriptor, ModelError, ModelOutput, ModelProvider,
    ModelRequest, ModelResponse, ModelRole, RuntimeBackend, RuntimeRecall, RuntimeRequest,
    RuntimeResult, RuntimeRunner, RuntimeService, ToolLoop, ToolRegistry,
};
use jc_memory_storage::{SqliteConfig, SqliteStore};
use tempfile::tempdir;
use tokio_util::sync::CancellationToken;

struct FinalProvider {
    requests: Arc<Mutex<Vec<ModelRequest>>>,
}

#[async_trait]
impl ModelProvider for FinalProvider {
    async fn invoke(
        &self,
        request: ModelRequest,
        _cancellation: CancellationToken,
    ) -> Result<ModelResponse, ModelError> {
        self.requests.lock().unwrap().push(request);
        Ok(ModelResponse {
            output: ModelOutput::FinalResponse(FinalResponse {
                text: "已完成".to_owned(),
            }),
            raw_text: "fixture".to_owned(),
        })
    }

    fn descriptor(&self) -> ModelDescriptor {
        ModelDescriptor {
            provider: "fixture".to_owned(),
            model: "final-v1".to_owned(),
            supports_streaming: false,
        }
    }
}

struct RecordingBackend {
    phases: Arc<Mutex<Vec<&'static str>>>,
}

#[async_trait]
impl RuntimeBackend for RecordingBackend {
    async fn observe(
        &self,
        _access: &AccessContext,
        _scope: &MemoryScope,
        _input: &str,
    ) -> Result<Vec<jc_memory_protocol::EvidenceRef>, String> {
        self.phases.lock().unwrap().push("observe");
        Ok(Vec::new())
    }

    async fn recall(
        &self,
        _access: &AccessContext,
        _scope: &MemoryScope,
        _input: &str,
    ) -> Result<RuntimeRecall, String> {
        self.phases.lock().unwrap().push("recall");
        Ok(RuntimeRecall {
            recommended_context: "来自 Memory Packet 的上下文".to_owned(),
            transition_payload: serde_json::json!({
                "query_id": "00000000-0000-7000-8000-000000000901",
                "coverage": { "evidence_count": 2, "truncated": false }
            }),
            evidence_refs: Vec::new(),
        })
    }

    async fn commit(
        &self,
        _access: &AccessContext,
        _scope: &MemoryScope,
        _commit: jc_memory_runtime::RuntimeCommit,
    ) -> Result<Vec<jc_memory_protocol::EvidenceRef>, String> {
        self.phases.lock().unwrap().push("commit");
        Ok(Vec::new())
    }
}

#[tokio::test]
async fn service_orchestrates_final_response_and_persists_completed_run() {
    let directory = tempdir().unwrap();
    let store = SqliteStore::open(&SqliteConfig::new(directory.path().join("memory.sqlite3")))
        .await
        .unwrap();
    let scope = MemoryScope::new(NamespaceId::new());
    let mut access = AccessContext::new("runtime-service", scope.clone());
    access
        .grants
        .insert(Grant::for_scope(Operation::Mutate, &scope));
    let phases = Arc::new(Mutex::new(Vec::new()));
    let requests = Arc::new(Mutex::new(Vec::new()));
    let service = RuntimeService::new(
        RuntimeRunner::from_store(&store),
        Arc::new(FinalProvider {
            requests: Arc::clone(&requests),
        }),
        Arc::new(RecordingBackend {
            phases: Arc::clone(&phases),
        }),
        ToolLoop::new(ToolRegistry::default(), ApprovalStore::default()),
    );

    let result = service
        .run(
            &access,
            RuntimeRequest {
                scope,
                prompt: "请总结".to_owned(),
                deadline: Utc.timestamp_micros(1_900_000_000_000_000).unwrap(),
                cancellation: CancellationToken::new(),
            },
        )
        .await
        .unwrap();

    let run_id = match &result {
        RuntimeResult::Completed { snapshot, .. }
        | RuntimeResult::AwaitingApproval { snapshot, .. } => snapshot.run_id.to_string(),
    };
    match result {
        RuntimeResult::Completed { response, snapshot } => {
            assert_eq!(response.text, "已完成");
            assert_eq!(snapshot.state, jc_memory_runtime::RuntimeState::Completed);
        }
        RuntimeResult::AwaitingApproval { .. } => panic!("unexpected approval result"),
    }
    assert_eq!(*phases.lock().unwrap(), vec!["observe", "recall", "commit"]);
    {
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].messages.len(), 2);
        assert_eq!(requests[0].messages[0].role, ModelRole::System);
        assert_eq!(
            requests[0].messages[0].content,
            "来自 Memory Packet 的上下文"
        );
        assert_eq!(requests[0].messages[1].role, ModelRole::User);
        assert_eq!(requests[0].messages[1].content, "请总结");
    }

    let recall_payload: String =
        sqlx::query_scalar("SELECT payload_json FROM transitions WHERE run_id = ?1 AND step = 1")
            .bind(run_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    let recall_payload: serde_json::Value = serde_json::from_str(&recall_payload).unwrap();
    assert_eq!(recall_payload["phase"], "recall");
    assert_eq!(
        recall_payload["memory_packet"]["query_id"],
        "00000000-0000-7000-8000-000000000901"
    );
}
