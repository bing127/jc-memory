//! `RuntimeService` 到共享 Core `CommandBus` 的权威映射。

use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use jc_memory_core::CommandBus;
use jc_memory_protocol::{
    API_VERSION_V1, AccessContext, CognitiveFragment, CommitCommand, FragmentContent, FragmentId,
    FragmentKind, FragmentSource, MemoryCommand, MemoryRequest, MemoryResponse, MemoryScope,
    ObserveCommand, Perspective, RecallCommand, RecallLimits, RecallMode, SecurityLabel,
    SourceType, TraceId, ValidityStatus,
};
use jc_memory_runtime::{RuntimeBackend, RuntimeCommit, RuntimeRecall};
use serde_json::{Map, json};

/// 只通过稳定 `MemoryCommand` 访问 Core 的 Runtime 后端。
#[derive(Clone)]
pub struct CommandBusRuntimeBackend<B> {
    bus: Arc<B>,
}

impl<B> CommandBusRuntimeBackend<B> {
    /// 创建共享 `CommandBus` 的 Runtime 后端。
    #[must_use]
    pub fn new(bus: B) -> Self {
        Self { bus: Arc::new(bus) }
    }
}

#[async_trait]
impl<B> RuntimeBackend for CommandBusRuntimeBackend<B>
where
    B: CommandBus + Send + Sync + 'static,
{
    async fn observe(
        &self,
        access: &AccessContext,
        scope: &MemoryScope,
        input: &str,
    ) -> Result<Vec<jc_memory_protocol::EvidenceRef>, String> {
        let fragment = runtime_fragment(
            scope,
            FragmentKind::Observation,
            FragmentContent::Text {
                text: input.to_owned(),
            },
            SourceType::User,
            "runtime.user_prompt",
            Perspective::FirstPerson,
        );
        let response = self
            .execute(
                access,
                scope,
                MemoryCommand::Observe(ObserveCommand { fragment }),
                "observe",
            )
            .await?;
        if !matches!(response, MemoryResponse::Observe(_)) {
            return Err("command bus returned an unexpected runtime observe response".to_owned());
        }
        Ok(Vec::new())
    }

    async fn recall(
        &self,
        access: &AccessContext,
        scope: &MemoryScope,
        input: &str,
    ) -> Result<RuntimeRecall, String> {
        let response = self
            .execute(
                access,
                scope,
                MemoryCommand::Recall(RecallCommand {
                    mode: RecallMode::Associative,
                    text: input.to_owned(),
                    entities: Vec::new(),
                    at_time: None,
                    limits: RecallLimits {
                        max_seeds: 8,
                        max_neighbors: 8,
                        max_hops: 3,
                        max_nodes: 64,
                        timeout_ms: 2_000,
                    },
                }),
                "recall",
            )
            .await?;
        let MemoryResponse::Recall(packet) = response else {
            return Err("command bus returned an unexpected runtime recall response".to_owned());
        };
        let transition_payload = json!({
            "query_id": packet.query_id,
            "intent": packet.intent,
            "coverage": packet.coverage,
            "confidence": packet.confidence,
            "evidence": packet.evidence,
            "missing_evidence": packet.missing_evidence,
        });
        Ok(RuntimeRecall {
            recommended_context: packet.recommended_context,
            transition_payload,
            evidence_refs: Vec::new(),
        })
    }

    async fn commit(
        &self,
        access: &AccessContext,
        scope: &MemoryScope,
        commit: RuntimeCommit,
    ) -> Result<Vec<jc_memory_protocol::EvidenceRef>, String> {
        let RuntimeCommit::Final(response) = commit else {
            return Err("runtime tool commit is not available in the server adapter".to_owned());
        };
        let fragment = runtime_fragment(
            scope,
            FragmentKind::Inference,
            FragmentContent::Text {
                text: response.text,
            },
            SourceType::Model,
            "runtime.final_response",
            Perspective::System,
        );
        let response = self
            .execute(
                access,
                scope,
                MemoryCommand::Commit(CommitCommand {
                    fragments: vec![fragment],
                }),
                "commit",
            )
            .await?;
        if !matches!(response, MemoryResponse::Commit(_)) {
            return Err("command bus returned an unexpected runtime commit response".to_owned());
        }
        Ok(Vec::new())
    }
}

impl<B> CommandBusRuntimeBackend<B>
where
    B: CommandBus + Send + Sync + 'static,
{
    async fn execute(
        &self,
        access: &AccessContext,
        scope: &MemoryScope,
        command: MemoryCommand,
        phase: &str,
    ) -> Result<MemoryResponse, String> {
        let request_id = format!("{}:runtime-{phase}:{}", access.request_id, TraceId::new());
        let mut phase_access = access.clone();
        phase_access.request_id.clone_from(&request_id);
        self.bus
            .execute(
                &phase_access,
                MemoryRequest {
                    api_version: API_VERSION_V1.to_owned(),
                    request_id,
                    access_scope: scope.clone().into(),
                    command,
                },
            )
            .await
            .map(|response| response.response)
            .map_err(|_| format!("command bus rejected runtime {phase}"))
    }
}

fn runtime_fragment(
    scope: &MemoryScope,
    kind: FragmentKind,
    content: FragmentContent,
    source_type: SourceType,
    source_id: &str,
    perspective: Perspective,
) -> CognitiveFragment {
    let now = Utc::now();
    let mut metadata = Map::new();
    metadata.insert("runtime_generated".to_owned(), json!(true));
    CognitiveFragment {
        id: FragmentId::new(),
        scope: scope.clone(),
        session_id: None,
        trace_id: None,
        kind,
        content,
        source: FragmentSource {
            source_type,
            source_id: source_id.to_owned(),
            actor_id: None,
            observer_id: None,
            tool_id: None,
            uri: None,
        },
        perspective,
        occurred_at: now,
        observed_at: now,
        confidence: 0.0,
        importance: 0.0,
        validity: ValidityStatus::Unverified,
        security_label: SecurityLabel::Internal,
        evidence: Vec::new(),
        metadata,
    }
}
