//! 通过追加权威 Event 表达记忆纠正和关系变更。

use chrono::Utc;
use jc_memory_events::validate_state_metadata;
use jc_memory_protocol::{
    AccessContext, CognitiveFragment, EdgeKind, EvidenceId, EvidenceRef, FragmentId, FragmentKind,
    Metadata, Operation, StateDeclaration, ValidityStatus,
};

use crate::{MemoryEngine, MemoryError, MutationCommand, MutationKind, MutationReceipt};

#[derive(Clone, Copy)]
struct MutationSpec {
    kind: MutationKind,
    fragment_kind: FragmentKind,
    validity: ValidityStatus,
    relation: Option<EdgeKind>,
}

impl MemoryEngine {
    /// 追加一条纠正记录，并保留原始 Fragment。
    ///
    /// # Errors
    ///
    /// 目标不存在、命令无效或事务性追加失败时返回错误。
    pub async fn correct(
        &self,
        context: &AccessContext,
        command: MutationCommand,
    ) -> Result<MutationReceipt, MemoryError> {
        self.apply_mutation(context, command, spec(MutationKind::Correct))
            .await
    }

    /// 追加支持目标记忆的新证据。
    ///
    /// # Errors
    ///
    /// 目标不存在、命令无效或事务性追加失败时返回错误。
    pub async fn reinforce(
        &self,
        context: &AccessContext,
        command: MutationCommand,
    ) -> Result<MutationReceipt, MemoryError> {
        self.apply_mutation(context, command, spec(MutationKind::Reinforce))
            .await
    }

    /// 追加降低目标可信度的证据。
    ///
    /// # Errors
    ///
    /// 目标不存在、命令无效或事务性追加失败时返回错误。
    pub async fn weaken(
        &self,
        context: &AccessContext,
        command: MutationCommand,
    ) -> Result<MutationReceipt, MemoryError> {
        self.apply_mutation(context, command, spec(MutationKind::Weaken))
            .await
    }

    /// 追加替代目标记忆的新版本。
    ///
    /// # Errors
    ///
    /// 目标不存在、命令无效或事务性追加失败时返回错误。
    pub async fn supersede(
        &self,
        context: &AccessContext,
        command: MutationCommand,
    ) -> Result<MutationReceipt, MemoryError> {
        self.apply_mutation(context, command, spec(MutationKind::Supersede))
            .await
    }

    /// 追加与目标记忆冲突的证据，不隐藏任一侧。
    ///
    /// # Errors
    ///
    /// 目标不存在、命令无效或事务性追加失败时返回错误。
    pub async fn contradict(
        &self,
        context: &AccessContext,
        command: MutationCommand,
    ) -> Result<MutationReceipt, MemoryError> {
        self.apply_mutation(context, command, spec(MutationKind::Contradict))
            .await
    }

    /// 追加 `Invalidated` 纠正记录，不删除或改写原始 Fragment/Event。
    ///
    /// # Errors
    ///
    /// 目标不存在、命令无效或事务性追加失败时返回错误。
    pub async fn forget(
        &self,
        context: &AccessContext,
        command: MutationCommand,
    ) -> Result<MutationReceipt, MemoryError> {
        self.apply_mutation(context, command, spec(MutationKind::Forget))
            .await
    }

    async fn apply_mutation(
        &self,
        context: &AccessContext,
        command: MutationCommand,
        spec: MutationSpec,
    ) -> Result<MutationReceipt, MemoryError> {
        crate::security::authorize(context, Operation::Mutate, &context.scope)?;
        let target = self
            .repository
            .get_fragment_in_scope(&context.scope, command.target_fragment_id)
            .await?;
        validate_state_mutation_target(&target, command.state.as_ref())?;
        let fragment_id = FragmentId::new();
        let fragment = mutation_fragment(fragment_id, &target, &command, spec);
        let receipt = self.observe_admitted(fragment).await?;
        let admitted = self
            .repository
            .get_fragment_in_scope(&context.scope, receipt.fragment_id)
            .await?;
        let relation = if admitted.validity == ValidityStatus::Quarantined {
            None
        } else {
            spec.relation
        };
        Ok(MutationReceipt {
            kind: spec.kind,
            target_fragment_id: command.target_fragment_id,
            fragment_id: receipt.fragment_id,
            event_id: receipt.event_id,
            sequence: receipt.sequence,
            relation,
        })
    }
}

fn validate_state_mutation_target(
    target: &CognitiveFragment,
    state: Option<&StateDeclaration>,
) -> Result<(), MemoryError> {
    let Some(state) = state else {
        return Ok(());
    };
    if !matches!(
        target.kind,
        FragmentKind::Fact | FragmentKind::StateTransition | FragmentKind::Correction
    ) || matches!(
        target.validity,
        ValidityStatus::Quarantined | ValidityStatus::Invalidated
    ) {
        return Err(MemoryError::InvalidStateMutation {
            reason: "target is not projectable state",
        });
    }
    let target_state = validate_state_metadata(target)
        .map_err(|_| MemoryError::InvalidStateMutation {
            reason: "target state metadata is invalid",
        })?
        .ok_or(MemoryError::InvalidStateMutation {
            reason: "target has no state metadata",
        })?;
    if target_state.key != state.key {
        return Err(MemoryError::InvalidStateMutation {
            reason: "state key does not match target",
        });
    }
    Ok(())
}

fn mutation_fragment(
    id: FragmentId,
    target: &CognitiveFragment,
    command: &MutationCommand,
    spec: MutationSpec,
) -> CognitiveFragment {
    let mut metadata = Metadata::new();
    metadata.insert(
        "mutation".to_owned(),
        serde_json::to_value(spec.kind).expect("MutationKind 始终可以序列化"),
    );
    metadata.insert(
        "target_fragment_id".to_owned(),
        serde_json::Value::String(command.target_fragment_id.to_string()),
    );
    metadata.insert(
        "relation".to_owned(),
        spec.relation.map_or(serde_json::Value::Null, |relation| {
            serde_json::to_value(relation).expect("EdgeKind 始终可以序列化")
        }),
    );
    if let Some(state) = &command.state {
        metadata.insert(
            "state".to_owned(),
            serde_json::to_value(state).expect("StateDeclaration 始终可以序列化"),
        );
    }

    CognitiveFragment {
        id,
        scope: target.scope.clone(),
        session_id: target.session_id,
        trace_id: target.trace_id,
        kind: spec.fragment_kind,
        content: command.content.clone(),
        source: command.source.clone(),
        perspective: command.perspective,
        occurred_at: command.occurred_at,
        observed_at: Utc::now(),
        confidence: mutation_confidence(target.confidence, spec.kind),
        importance: target.importance,
        validity: spec.validity,
        security_label: target.security_label,
        evidence: vec![EvidenceRef {
            id: EvidenceId::new(),
            fragment_id: Some(target.id),
            event_id: None,
            method: command.verification_method,
            note: Some(command.reason.clone()),
        }],
        metadata,
    }
}

const fn spec(kind: MutationKind) -> MutationSpec {
    match kind {
        MutationKind::Correct | MutationKind::Supersede => MutationSpec {
            kind,
            fragment_kind: FragmentKind::Correction,
            validity: ValidityStatus::Verified,
            relation: Some(EdgeKind::Supersedes),
        },
        MutationKind::Reinforce => MutationSpec {
            kind,
            fragment_kind: FragmentKind::Fact,
            validity: ValidityStatus::Verified,
            relation: Some(EdgeKind::Supports),
        },
        MutationKind::Weaken => MutationSpec {
            kind,
            fragment_kind: FragmentKind::Correction,
            validity: ValidityStatus::Disputed,
            relation: Some(EdgeKind::DisprovedBy),
        },
        MutationKind::Contradict => MutationSpec {
            kind,
            fragment_kind: FragmentKind::Correction,
            validity: ValidityStatus::Disputed,
            relation: Some(EdgeKind::Contradicts),
        },
        MutationKind::Forget => MutationSpec {
            kind,
            fragment_kind: FragmentKind::Correction,
            validity: ValidityStatus::Invalidated,
            relation: None,
        },
    }
}

fn mutation_confidence(target_confidence: f32, kind: MutationKind) -> f32 {
    if kind == MutationKind::Weaken {
        target_confidence * 0.5
    } else {
        target_confidence
    }
}
