//! 权威存储与有界 Recall 流水线之间的核心门面。

use std::{
    collections::{BTreeMap, BTreeSet},
    str::FromStr as _,
};

use jc_memory_activation::{ExplainTrace, IndexedPoint, RecallEngine, RecallQuery, RecallResult};
use jc_memory_graph::CognitivePoint;
use jc_memory_protocol::{
    AccessContext, EpisodeView, EvidenceView, FragmentId, MemoryPacket, MemoryScope,
    MissingEvidence, Operation, PointId, PointView, RecallId, RecallMode, RecallPath,
    ValidityStatus,
};

use crate::{MemoryEngine, MemoryError};

/// 一个精确 Scope 内的权威存储统计。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemoryStats {
    /// 被统计的精确所有权 Scope。
    pub scope: MemoryScope,
    /// 权威 Fragment 数量。
    pub fragment_count: u64,
    /// 权威 Event 数量。
    pub event_count: u64,
}

struct CoreRecall {
    packet: MemoryPacket,
    explain: ExplainTrace,
}

impl MemoryEngine {
    /// 在精确 Scope 和硬预算内返回模型无关 Memory Packet。
    ///
    /// # Errors
    ///
    /// 当 FTS 查询、权威记录解码、激活流水线或 Packet 渲染失败时返回错误。
    pub async fn recall(
        &self,
        context: &AccessContext,
        query: RecallQuery,
    ) -> Result<MemoryPacket, MemoryError> {
        Ok(self
            .execute_recall(context, Operation::Recall, query)
            .await?
            .packet)
    }

    /// 返回与一次有界 Recall 对应的可追溯解释。
    ///
    /// # Errors
    ///
    /// 返回与 [`Self::recall`] 相同的存储、激活或序列化错误。
    pub async fn explain(
        &self,
        context: &AccessContext,
        query: RecallQuery,
    ) -> Result<ExplainTrace, MemoryError> {
        Ok(self
            .execute_recall(context, Operation::Explain, query)
            .await?
            .explain)
    }

    /// 读取一个精确 Scope 的持久化权威记录统计。
    ///
    /// # Errors
    ///
    /// 当 `SQLite` 统计查询失败时返回错误。
    pub async fn stats(
        &self,
        context: &AccessContext,
        scope: &MemoryScope,
    ) -> Result<MemoryStats, MemoryError> {
        crate::security::authorize(context, Operation::Recall, scope)?;
        let stats = self.repository.stats(scope).await?;
        Ok(MemoryStats {
            scope: scope.clone(),
            fragment_count: stats.fragment_count,
            event_count: stats.event_count,
        })
    }

    async fn execute_recall(
        &self,
        context: &AccessContext,
        operation: Operation,
        query: RecallQuery,
    ) -> Result<CoreRecall, MemoryError> {
        crate::security::authorize(context, operation, &query.scope)?;
        let search_limit = u32::try_from(query.limits.max_nodes).unwrap_or(u32::MAX);
        let mut hits_by_fragment: BTreeMap<_, _> = self
            .repository
            .search_fts(&query.scope, &query.text, search_limit)
            .await?
            .into_iter()
            .map(|hit| (hit.fragment_id, hit))
            .collect();
        if query.mode != RecallMode::Exact {
            for hit in self
                .repository
                .recent_fragments(&query.scope, query.at_time, search_limit)
                .await?
            {
                hits_by_fragment.entry(hit.fragment_id).or_insert(hit);
            }
        }
        let (derived_points, derived_edges) = self.derived_graph.read().await.snapshot();
        let mut fragment_scopes = BTreeMap::<FragmentId, MemoryScope>::new();
        for indexed in &derived_points {
            for fragment_id in &indexed.point.source_fragments {
                fragment_scopes.insert(*fragment_id, indexed.point.scope.clone());
            }
        }
        let mut point_candidates: BTreeMap<_, _> = derived_points
            .into_iter()
            .map(|indexed| (indexed.point.id, indexed))
            .collect();
        let mut episodes = Vec::new();
        let mut evidence = Vec::new();
        for hit in hits_by_fragment.into_values() {
            let fragment = self
                .repository
                .get_fragment_in_scope(&query.scope, hit.fragment_id)
                .await?;
            fragment_scopes.insert(fragment.id, fragment.scope.clone());
            if matches!(
                fragment.validity,
                ValidityStatus::Invalidated | ValidityStatus::Quarantined
            ) || query
                .at_time
                .is_some_and(|effective_at| fragment.occurred_at > effective_at)
            {
                continue;
            }
            let evidence_view = EvidenceView {
                fragment_id: fragment.id,
                event_id: Some(hit.event_id),
                evidence_ref_id: None,
            };
            episodes.push(EpisodeView {
                event_id: hit.event_id,
                fragment_id: fragment.id,
                occurred_at: fragment.occurred_at,
                summary: hit.body.clone(),
                evidence: vec![evidence_view.clone()],
            });
            evidence.push(evidence_view);
            if let Ok(mut point) = CognitivePoint::try_from(&fragment) {
                point.id = PointId::from_str(&fragment.id.to_string())?;
                let indexed = IndexedPoint {
                    point,
                    text: hit.body,
                    entities: fragment_entities(&fragment.metadata),
                };
                point_candidates.insert(indexed.point.id, indexed);
            }
        }

        let points_by_id = point_candidates.clone();
        let engine =
            RecallEngine::try_from_parts(point_candidates.into_values(), derived_edges, [])?;
        let result = engine.recall(query.clone()).await?;
        let explain = engine.explain(&result, query.limits);
        let packet = build_packet(&query, &result, &points_by_id, episodes, evidence)?;
        crate::security::validate_packet(
            context,
            operation,
            &query.scope,
            &packet,
            &fragment_scopes,
        )?;
        Ok(CoreRecall { packet, explain })
    }
}

fn build_packet(
    query: &RecallQuery,
    result: &RecallResult,
    points_by_id: &BTreeMap<PointId, IndexedPoint>,
    episodes: Vec<EpisodeView>,
    evidence: Vec<EvidenceView>,
) -> Result<MemoryPacket, serde_json::Error> {
    let mut packet = MemoryPacket::empty(RecallId::new(), query.mode);
    let activated_fragment_ids: BTreeSet<_> = result
        .points
        .iter()
        .filter_map(|activated| points_by_id.get(&activated.point_id))
        .flat_map(|indexed| indexed.point.source_fragments.iter().copied())
        .collect();
    packet.episodes = episodes
        .into_iter()
        .filter(|episode| activated_fragment_ids.contains(&episode.fragment_id))
        .collect();
    packet.evidence = evidence
        .into_iter()
        .filter(|item| activated_fragment_ids.contains(&item.fragment_id))
        .collect();
    packet.points = result
        .points
        .iter()
        .filter_map(|activated| points_by_id.get(&activated.point_id))
        .map(|indexed| PointView {
            point_id: indexed.point.id,
            kind: indexed.point.kind,
            confidence: indexed.point.confidence,
            validity: indexed.point.validity,
            source_fragments: indexed.point.source_fragments.clone(),
        })
        .collect();
    packet.paths = result
        .points
        .iter()
        .map(|point| RecallPath {
            point_ids: vec![point.point_id],
            edge_ids: point.via_edge_id.into_iter().collect(),
            score: point.activation,
        })
        .collect();
    packet.activated_surfaces = Vec::new();
    packet.missing_evidence = packet
        .points
        .iter()
        .filter(|point| {
            matches!(
                point.validity,
                ValidityStatus::Unverified | ValidityStatus::Disputed
            )
        })
        .map(|point| MissingEvidence {
            point_id: point.point_id,
            expected_edge_kinds: Vec::new(),
            description: "该 Point 尚缺少独立验证证据".to_owned(),
        })
        .collect();
    packet.coverage.seed_count = result.trace.seed_count;
    packet.coverage.visited_nodes = result.trace.visited_nodes;
    packet.coverage.max_depth = result.trace.max_depth;
    packet.coverage.evidence_count = packet.evidence.len();
    packet.coverage.missing_evidence_count = packet.missing_evidence.len();
    packet.coverage.truncated = result.trace.truncated;
    let (confidence_sum, confidence_count) = packet
        .points
        .iter()
        .fold((0.0_f32, 0.0_f32), |(sum, count), point| {
            (sum + point.confidence, count + 1.0)
        });
    packet.confidence = if confidence_count > 0.0 {
        confidence_sum / confidence_count
    } else {
        0.0
    };
    packet.refresh_recommended_context()?;
    Ok(packet)
}

fn fragment_entities(metadata: &serde_json::Map<String, serde_json::Value>) -> Vec<String> {
    metadata
        .get("entities")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
        .collect()
}
