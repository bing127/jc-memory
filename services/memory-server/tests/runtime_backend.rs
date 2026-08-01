//! Runtime 到共享 `CommandBus` 的权威映射契约测试。

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use jc_memory_core::{CommandBus, CommandError};
use jc_memory_protocol::{
    AccessContext, CommitResult, FragmentKind, Grant, MemoryCommand, MemoryPacket, MemoryRequest,
    MemoryResponse, MemoryScope, NamespaceId, ObserveResult, Operation, RecallId, RecallMode,
    ResponseEnvelope, SourceType, ValidityStatus,
};
use jc_memory_runtime::{FinalResponse, RuntimeBackend, RuntimeCommit};
use jc_memory_server::CommandBusRuntimeBackend;

#[derive(Clone, Default)]
struct RecordingBus {
    requests: Arc<Mutex<Vec<MemoryRequest>>>,
}

#[async_trait]
impl CommandBus for RecordingBus {
    async fn execute(
        &self,
        _access: &AccessContext,
        request: MemoryRequest,
    ) -> Result<ResponseEnvelope, CommandError> {
        self.requests.lock().unwrap().push(request.clone());
        let response = match request.command {
            MemoryCommand::Observe(_) => MemoryResponse::Observe(observe_result()),
            MemoryCommand::Recall(command) => {
                let mut packet = MemoryPacket::empty(RecallId::new(), command.mode);
                "真实 Recall Context".clone_into(&mut packet.recommended_context);
                MemoryResponse::Recall(packet)
            }
            MemoryCommand::Commit(_) => MemoryResponse::Commit(CommitResult {
                receipts: vec![observe_result()],
            }),
            _ => panic!("unexpected command"),
        };
        Ok(ResponseEnvelope {
            api_version: request.api_version,
            request_id: request.request_id,
            response,
        })
    }
}

#[tokio::test]
async fn maps_runtime_phases_without_promoting_model_output_to_verified_fact() {
    let bus = RecordingBus::default();
    let requests = Arc::clone(&bus.requests);
    let backend = CommandBusRuntimeBackend::new(bus);
    let scope = MemoryScope::new(NamespaceId::new());
    let mut access = AccessContext::new("runtime-principal", scope.clone());
    for operation in [Operation::Observe, Operation::Recall, Operation::Mutate] {
        access.grants.insert(Grant::for_scope(operation, &scope));
    }

    assert!(
        backend
            .observe(&access, &scope, "用户原始输入")
            .await
            .unwrap()
            .is_empty()
    );
    let recalled = backend
        .recall(&access, &scope, "用户原始输入")
        .await
        .unwrap();
    assert_eq!(recalled.recommended_context, "真实 Recall Context");
    assert!(recalled.evidence_refs.is_empty());
    assert!(recalled.transition_payload["query_id"].is_string());
    assert!(
        backend
            .commit(
                &access,
                &scope,
                RuntimeCommit::Final(FinalResponse {
                    text: "模型最终响应".to_owned()
                }),
            )
            .await
            .unwrap()
            .is_empty()
    );

    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), 3);
    assert!(
        requests
            .iter()
            .all(|request| MemoryScope::from(request.access_scope.clone()) == scope)
    );
    let MemoryCommand::Observe(observe) = &requests[0].command else {
        panic!("observe")
    };
    assert_eq!(observe.fragment.kind, FragmentKind::Observation);
    assert_eq!(observe.fragment.source.source_type, SourceType::User);
    assert_eq!(observe.fragment.validity, ValidityStatus::Unverified);
    let MemoryCommand::Recall(recall) = &requests[1].command else {
        panic!("recall")
    };
    assert_eq!(recall.mode, RecallMode::Associative);
    assert!(recall.limits.max_nodes > 0);
    let MemoryCommand::Commit(commit) = &requests[2].command else {
        panic!("commit")
    };
    assert_eq!(commit.fragments.len(), 1);
    assert_eq!(commit.fragments[0].kind, FragmentKind::Inference);
    assert_eq!(commit.fragments[0].source.source_type, SourceType::Model);
    assert_eq!(commit.fragments[0].validity, ValidityStatus::Unverified);
    assert!(commit.fragments[0].confidence.abs() <= f32::EPSILON);
    assert!(commit.fragments[0].importance.abs() <= f32::EPSILON);
    assert_ne!(commit.fragments[0].kind, FragmentKind::Fact);
}

fn observe_result() -> ObserveResult {
    ObserveResult {
        fragment_id: jc_memory_protocol::FragmentId::new(),
        event_id: jc_memory_protocol::EventId::new(),
        sequence: 1,
        content_hash: "0".repeat(64).parse().unwrap(),
        created: true,
        quarantined: false,
    }
}
