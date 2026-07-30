//! 通过追加权威 Event 表达记忆纠正和关系变更。

use std::{collections::BTreeMap, str::FromStr as _};

use chrono::Utc;
use jc_memory_activation::IndexedPoint;
use jc_memory_graph::{CognitiveEdge, CognitivePoint, EvidenceLink};
use jc_memory_protocol::{
    CognitiveFragment, EdgeId, EdgeKind, EvidenceId, EvidenceRef, FragmentContent, FragmentId,
    FragmentKind, Metadata, PointId, ValidityStatus, VerificationMethod,
};

use crate::{MemoryEngine, MemoryError, MutationCommand, MutationKind, MutationReceipt};

#[derive(Clone, Copy)]
struct MutationSpec {
    kind: MutationKind,
    fragment_kind: FragmentKind,
    validity: ValidityStatus,
    relation: Option<EdgeKind>,
}

/// 可由权威变更 Event 重建的进程内派生图。
#[derive(Debug, Default)]
pub(crate) struct DerivedGraph {
    points: BTreeMap<PointId, IndexedPoint>,
    edges: BTreeMap<EdgeId, CognitiveEdge>,
}

impl DerivedGraph {
    pub(crate) fn snapshot(&self) -> (Vec<IndexedPoint>, Vec<CognitiveEdge>) {
        (
            self.points.values().cloned().collect(),
            self.edges.values().cloned().collect(),
        )
    }

    fn insert_relation(
        &mut self,
        target: IndexedPoint,
        mutation: IndexedPoint,
        edge: CognitiveEdge,
    ) {
        self.points.insert(target.point.id, target);
        self.points.insert(mutation.point.id, mutation);
        self.edges.insert(edge.id, edge);
    }
}

impl MemoryEngine {
    /// 追加一条纠正记录，并保留原始 Fragment。
    ///
    /// # Errors
    ///
    /// 目标不存在、命令无效或事务性追加失败时返回错误。
    pub async fn correct(&self, command: MutationCommand) -> Result<MutationReceipt, MemoryError> {
        self.apply_mutation(command, spec(MutationKind::Correct))
            .await
    }

    /// 追加支持目标记忆的新证据。
    ///
    /// # Errors
    ///
    /// 目标不存在、命令无效或事务性追加失败时返回错误。
    pub async fn reinforce(
        &self,
        command: MutationCommand,
    ) -> Result<MutationReceipt, MemoryError> {
        self.apply_mutation(command, spec(MutationKind::Reinforce))
            .await
    }

    /// 追加降低目标可信度的证据。
    ///
    /// # Errors
    ///
    /// 目标不存在、命令无效或事务性追加失败时返回错误。
    pub async fn weaken(&self, command: MutationCommand) -> Result<MutationReceipt, MemoryError> {
        self.apply_mutation(command, spec(MutationKind::Weaken))
            .await
    }

    /// 追加替代目标记忆的新版本。
    ///
    /// # Errors
    ///
    /// 目标不存在、命令无效或事务性追加失败时返回错误。
    pub async fn supersede(
        &self,
        command: MutationCommand,
    ) -> Result<MutationReceipt, MemoryError> {
        self.apply_mutation(command, spec(MutationKind::Supersede))
            .await
    }

    /// 追加与目标记忆冲突的证据，不隐藏任一侧。
    ///
    /// # Errors
    ///
    /// 目标不存在、命令无效或事务性追加失败时返回错误。
    pub async fn contradict(
        &self,
        command: MutationCommand,
    ) -> Result<MutationReceipt, MemoryError> {
        self.apply_mutation(command, spec(MutationKind::Contradict))
            .await
    }

    /// 追加 `Invalidated` 纠正记录，不删除或改写原始 Fragment/Event。
    ///
    /// # Errors
    ///
    /// 目标不存在、命令无效或事务性追加失败时返回错误。
    pub async fn forget(&self, command: MutationCommand) -> Result<MutationReceipt, MemoryError> {
        self.apply_mutation(command, spec(MutationKind::Forget))
            .await
    }

    async fn apply_mutation(
        &self,
        command: MutationCommand,
        spec: MutationSpec,
    ) -> Result<MutationReceipt, MemoryError> {
        let target = self.fragment(command.target_fragment_id).await?;
        let fragment_id = FragmentId::new();
        let fragment = mutation_fragment(fragment_id, &target, &command, spec);
        let receipt = self.observe(fragment.clone()).await?;
        if let Some(relation) = spec.relation {
            self.record_relation(
                &target,
                &fragment,
                receipt.event_id,
                relation,
                command.verification_method,
            )
            .await?;
        }
        Ok(MutationReceipt {
            kind: spec.kind,
            target_fragment_id: command.target_fragment_id,
            fragment_id: receipt.fragment_id,
            event_id: receipt.event_id,
            sequence: receipt.sequence,
            relation: spec.relation,
        })
    }

    async fn record_relation(
        &self,
        target: &CognitiveFragment,
        mutation: &CognitiveFragment,
        event_id: jc_memory_protocol::EventId,
        relation: EdgeKind,
        verification_method: VerificationMethod,
    ) -> Result<(), MemoryError> {
        let evidence_ref_id = mutation.evidence.first().map(|evidence| evidence.id);
        let (Some(target), Some(mutation)) = (indexed_point(target)?, indexed_point(mutation)?)
        else {
            return Ok(());
        };
        let mut edge = CognitiveEdge::new(
            &mutation.point,
            &target.point,
            relation,
            EvidenceLink {
                fragment_id: mutation.point.source_fragments[0],
                event_id: Some(event_id),
                evidence_ref_id,
            },
        )?;
        edge.human_confirmed = verification_method == VerificationMethod::Manual;
        edge.tool_verified = verification_method == VerificationMethod::Tool;
        self.derived_graph
            .write()
            .await
            .insert_relation(target, mutation, edge);
        Ok(())
    }
}

fn indexed_point(fragment: &CognitiveFragment) -> Result<Option<IndexedPoint>, MemoryError> {
    let Ok(mut point) = CognitivePoint::try_from(fragment) else {
        return Ok(None);
    };
    point.id = PointId::from_str(&fragment.id.to_string())?;
    Ok(Some(IndexedPoint {
        point,
        text: fragment_text(&fragment.content),
        entities: Vec::new(),
    }))
}

fn fragment_text(content: &FragmentContent) -> String {
    match content {
        FragmentContent::Text { text } => text.clone(),
        FragmentContent::Json { value } => value.to_string(),
        FragmentContent::Blob { blob_id, .. } => blob_id.to_string(),
    }
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
