//! Quarantine 检测上下文、治理状态和授权审查接线。

use chrono::Utc;
use jc_memory_protocol::{
    AccessContext, CognitiveFragment, EvidenceId, EvidenceRef, FragmentContent, FragmentId,
    FragmentKind, FragmentSource, Metadata, Operation, Perspective, SourceType, ValidityStatus,
    VerificationMethod,
};
use jc_memory_security::{Admission, RiskAssessment, RiskContext};

use crate::{MemoryEngine, MemoryError, QuarantineReview, QuarantineReviewReceipt, ReviewDecision};

impl MemoryEngine {
    pub(crate) async fn risk_context(
        &self,
        fragment: &CognitiveFragment,
    ) -> Result<RiskContext, MemoryError> {
        Ok(RiskContext {
            repeated_disputed_claims: self.repeated_disputed_claims(fragment).await?,
            cross_scope_reference: self.has_cross_scope_reference(fragment).await,
        })
    }

    async fn repeated_disputed_claims(
        &self,
        fragment: &CognitiveFragment,
    ) -> Result<usize, MemoryError> {
        if fragment.validity != ValidityStatus::Disputed
            || !matches!(fragment.kind, FragmentKind::Claim | FragmentKind::Fact)
        {
            return Ok(0);
        }
        let text = searchable_text(&fragment.content);
        if text.is_empty() {
            return Ok(0);
        }
        let hits = self
            .repository
            .search_fts(&fragment.scope, &text, 32)
            .await?;
        let mut count = 0;
        for hit in hits {
            let previous = self
                .repository
                .get_fragment_in_scope(&fragment.scope, hit.fragment_id)
                .await?;
            if previous.validity == ValidityStatus::Disputed
                && matches!(previous.kind, FragmentKind::Claim | FragmentKind::Fact)
                && previous.content == fragment.content
            {
                count += 1;
            }
        }
        Ok(count)
    }

    async fn has_cross_scope_reference(&self, fragment: &CognitiveFragment) -> bool {
        for evidence in &fragment.evidence {
            if let Some(fragment_id) = evidence.fragment_id
                && let Ok(referenced) = self.repository.get_fragment(fragment_id).await
                && referenced.scope != fragment.scope
            {
                return true;
            }
            if let Some(event_id) = evidence.event_id
                && let Ok(referenced) = self.repository.get_event(event_id).await
                && referenced.draft.scope != fragment.scope
            {
                return true;
            }
        }
        false
    }

    pub(crate) async fn append_automatic_quarantine(
        &self,
        target: &CognitiveFragment,
        assessment: &RiskAssessment,
        admission: Admission,
    ) -> Result<(), MemoryError> {
        let Admission::Quarantine { reason } = admission else {
            return Ok(());
        };
        let mut metadata = Metadata::new();
        metadata.insert(
            "quarantine_action".to_owned(),
            serde_json::Value::String("quarantined".to_owned()),
        );
        metadata.insert(
            "target_fragment_id".to_owned(),
            serde_json::Value::String(target.id.to_string()),
        );
        metadata.insert(
            "risk_assessment".to_owned(),
            serde_json::to_value(assessment)?,
        );
        let fragment = governance_fragment(
            target,
            FragmentKind::StateTransition,
            FragmentContent::Json {
                value: serde_json::json!({
                    "status": "quarantined",
                    "reason": reason,
                }),
            },
            assessment.evidence.clone(),
            metadata,
            None,
        );
        self.observe_inner(fragment).await?;
        Ok(())
    }

    /// 经过精确 Scope 的 Admin 授权后追加 Release 或 Reject 审查记录。
    ///
    /// # Errors
    ///
    /// 缺少 Admin Grant、目标不存在或未隔离、理由为空、追加失败时返回错误。
    pub async fn review_quarantine(
        &self,
        context: &AccessContext,
        review: QuarantineReview,
    ) -> Result<QuarantineReviewReceipt, MemoryError> {
        crate::security::authorize(context, Operation::Admin, &context.scope)?;
        if review.reason.trim().is_empty() {
            return Err(MemoryError::EmptyReviewReason);
        }
        let target = self
            .repository
            .get_fragment_in_scope(&context.scope, review.target_fragment_id)
            .await?;
        if target.validity != ValidityStatus::Quarantined {
            return Err(MemoryError::ReviewTargetNotQuarantined {
                fragment_id: target.id,
            });
        }

        let audit = audit_fragment(&target, context, &review);
        let audit_receipt = self.observe_inner(audit).await?;
        let state = review_state_fragment(&target, context, &review, audit_receipt.event_id);
        let state_receipt = self.observe_inner(state).await?;
        Ok(QuarantineReviewReceipt {
            target_fragment_id: target.id,
            audit_event_id: audit_receipt.event_id,
            state_event_id: state_receipt.event_id,
            decision: review.decision,
        })
    }
}

fn audit_fragment(
    target: &CognitiveFragment,
    context: &AccessContext,
    review: &QuarantineReview,
) -> CognitiveFragment {
    let mut metadata = Metadata::new();
    metadata.insert(
        "audit_operation".to_owned(),
        serde_json::Value::String("quarantine_review".to_owned()),
    );
    metadata.insert(
        "target_fragment_id".to_owned(),
        serde_json::Value::String(target.id.to_string()),
    );
    metadata.insert(
        "request_id".to_owned(),
        serde_json::Value::String(context.request_id.clone()),
    );
    governance_fragment(
        target,
        match review.decision {
            ReviewDecision::Release => FragmentKind::Approval,
            ReviewDecision::Reject => FragmentKind::Rejection,
        },
        FragmentContent::Json {
            value: serde_json::json!({
                "decision": review.decision,
                "principal_id": context.principal_id,
                "reason": review.reason,
                "target_fragment_id": target.id,
            }),
        },
        vec![review_evidence(target.id, &review.reason)],
        metadata,
        Some(&context.principal_id),
    )
}

fn review_state_fragment(
    target: &CognitiveFragment,
    context: &AccessContext,
    review: &QuarantineReview,
    audit_event_id: jc_memory_protocol::EventId,
) -> CognitiveFragment {
    let action = match review.decision {
        ReviewDecision::Release => "released",
        ReviewDecision::Reject => "rejected",
    };
    let mut metadata = Metadata::new();
    metadata.insert(
        "quarantine_action".to_owned(),
        serde_json::Value::String(action.to_owned()),
    );
    metadata.insert(
        "target_fragment_id".to_owned(),
        serde_json::Value::String(target.id.to_string()),
    );
    let mut evidence = vec![review_evidence(target.id, &review.reason)];
    evidence.push(EvidenceRef {
        id: EvidenceId::new(),
        fragment_id: None,
        event_id: Some(audit_event_id),
        method: VerificationMethod::Manual,
        note: Some("关联授权审查的审计 Event".to_owned()),
    });
    governance_fragment(
        target,
        FragmentKind::StateTransition,
        FragmentContent::Json {
            value: serde_json::json!({
                "status": action,
                "reason": review.reason,
                "target_fragment_id": target.id,
            }),
        },
        evidence,
        metadata,
        Some(&context.principal_id),
    )
}

fn governance_fragment(
    target: &CognitiveFragment,
    kind: FragmentKind,
    content: FragmentContent,
    evidence: Vec<EvidenceRef>,
    metadata: Metadata,
    actor_id: Option<&str>,
) -> CognitiveFragment {
    let now = Utc::now();
    CognitiveFragment {
        id: FragmentId::new(),
        scope: target.scope.clone(),
        session_id: target.session_id,
        trace_id: target.trace_id,
        kind,
        content,
        source: FragmentSource {
            source_type: SourceType::System,
            source_id: "quarantine-governance".to_owned(),
            actor_id: actor_id.map(ToOwned::to_owned),
            observer_id: Some("jc-memory-core".to_owned()),
            tool_id: None,
            uri: None,
        },
        perspective: Perspective::System,
        occurred_at: now,
        observed_at: now,
        confidence: 1.0,
        importance: target.importance,
        validity: ValidityStatus::Quarantined,
        security_label: target.security_label,
        evidence,
        metadata,
    }
}

fn review_evidence(target_fragment_id: FragmentId, reason: &str) -> EvidenceRef {
    EvidenceRef {
        id: EvidenceId::new(),
        fragment_id: Some(target_fragment_id),
        event_id: None,
        method: VerificationMethod::Manual,
        note: Some(reason.to_owned()),
    }
}

fn searchable_text(content: &FragmentContent) -> String {
    match content {
        FragmentContent::Text { text } => text.clone(),
        FragmentContent::Json { value } => value.to_string(),
        FragmentContent::Blob { .. } => String::new(),
    }
}
