//! 权威存储与有界 Recall 流水线之间的核心门面。

use std::{
    collections::{BTreeMap, BTreeSet},
    future::Future,
    str::FromStr as _,
    time::{Duration, Instant},
};

use chrono::Utc;
use jc_memory_activation::{
    ExplainTrace, IndexedPoint, RecallEngine, RecallEngineError, RecallQuery, RecallResult,
    edge_kinds_for,
};
use jc_memory_graph::CognitivePoint;
use jc_memory_protocol::{
    AccessContext, EpisodeView, EvidenceView, FragmentId, MemoryPacket, MemoryScope,
    MissingEvidence, Operation, PointId, PointView, RecallId, RecallMode, RecallPath,
    ValidityStatus,
};
use jc_memory_storage::{ProjectedStateSet, QuarantineDisposition, StateSelector};

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

struct RecallPacketInputs<'input> {
    episodes: Vec<EpisodeView>,
    evidence: Vec<EvidenceView>,
    states: ProjectedStateSet,
    derivations: jc_memory_storage::ProjectedRecallDerivations,
    failure_fragments: &'input BTreeSet<FragmentId>,
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
        let timeout = Duration::from_millis(query.limits.timeout_ms);
        let deadline = Instant::now() + timeout;
        with_recall_deadline(
            deadline,
            self.execute_recall_inner(context, operation, query, deadline),
        )
        .await
    }

    #[allow(clippy::too_many_lines)]
    async fn execute_recall_inner(
        &self,
        context: &AccessContext,
        operation: Operation,
        query: RecallQuery,
        deadline: Instant,
    ) -> Result<CoreRecall, MemoryError> {
        self.catch_up_projections(&query.scope, deadline).await?;
        let effective_at = query.at_time.unwrap_or_else(Utc::now);
        let mut states = ProjectedStateSet::default();
        let mut exact_state_loaded = false;
        if matches!(
            query.mode,
            RecallMode::CurrentState | RecallMode::HistoricalState
        ) {
            states = self
                .projection
                .load_states(
                    &query.scope,
                    StateSelector::ExactKey(query.text.clone()),
                    effective_at,
                    query.limits.max_nodes,
                )
                .await?;
            exact_state_loaded =
                states.truncated || !states.current.is_empty() || !states.history.is_empty();
        }
        let state_nodes = states.current.len() + states.history.len();
        let graph_budget = query.limits.max_nodes.saturating_sub(state_nodes);
        let search_limit = u32::try_from(graph_budget).unwrap_or(u32::MAX);
        let search_hits = self
            .repository
            .search_fts(&query.scope, &query.text, search_limit)
            .await?;
        let mut graph_seed_fragments = search_hits
            .iter()
            .take(query.limits.max_seeds)
            .map(|hit| hit.fragment_id)
            .collect::<Vec<_>>();
        let mut hits_by_fragment: BTreeMap<_, _> = search_hits
            .into_iter()
            .map(|hit| (hit.fragment_id, hit))
            .collect();
        if query.mode != RecallMode::Exact {
            let recent_hits = self
                .repository
                .recent_fragments(&query.scope, Some(effective_at), search_limit)
                .await?;
            if graph_seed_fragments.is_empty() {
                graph_seed_fragments.extend(
                    recent_hits
                        .iter()
                        .take(query.limits.max_seeds)
                        .map(|hit| hit.fragment_id),
                );
            }
            for hit in recent_hits {
                hits_by_fragment.entry(hit.fragment_id).or_insert(hit);
            }
        }
        let mut fragment_scopes = BTreeMap::<FragmentId, MemoryScope>::new();
        let mut failure_fragments = BTreeSet::new();
        let mut episodes = Vec::new();
        let mut evidence = Vec::new();
        let hits = hits_by_fragment.into_values().collect::<Vec<_>>();
        let fragment_ids = hits.iter().map(|hit| hit.fragment_id).collect::<Vec<_>>();
        let graph = self
            .projection
            .load_graph(
                &query.scope,
                &graph_seed_fragments,
                edge_kinds_for(query.mode),
                effective_at,
                graph_budget,
            )
            .await?;
        let mut point_candidates = BTreeMap::new();
        for projected in graph.points {
            for fragment_id in &projected.point.source_fragments {
                fragment_scopes.insert(*fragment_id, projected.point.scope.clone());
            }
            point_candidates.insert(
                projected.point.id,
                IndexedPoint {
                    point: projected.point,
                    text: projected.text,
                    entities: projected.entities,
                },
            );
        }
        let fragments = self
            .repository
            .get_fragments_in_scope(&query.scope, &fragment_ids)
            .await?;
        let mut fragments_by_id = fragments
            .into_iter()
            .map(|fragment| (fragment.id, fragment))
            .collect::<BTreeMap<_, _>>();
        for hit in hits {
            let mut fragment = fragments_by_id.remove(&hit.fragment_id).ok_or_else(|| {
                MemoryError::Storage(jc_memory_storage::StorageError::NotFound {
                    entity: "fragment",
                    id: hit.fragment_id.to_string(),
                })
            })?;
            fragment_scopes.insert(fragment.id, fragment.scope.clone());
            if fragment.validity == ValidityStatus::Quarantined {
                match self
                    .repository
                    .quarantine_disposition(&query.scope, fragment.id, query.at_time)
                    .await?
                {
                    Some(QuarantineDisposition::Released) => {
                        fragment.validity = ValidityStatus::Unverified;
                    }
                    Some(QuarantineDisposition::Rejected) | None => continue,
                }
            }
            if fragment.validity == ValidityStatus::Invalidated
                || query
                    .at_time
                    .is_some_and(|effective_at| fragment.occurred_at > effective_at)
            {
                continue;
            }
            if fragment.kind == jc_memory_protocol::FragmentKind::Failure {
                failure_fragments.insert(fragment.id);
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
        let engine = RecallEngine::try_from_parts(point_candidates.into_values(), graph.edges, [])?;
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(MemoryError::Recall(RecallEngineError::TimedOut));
        }
        let mut activation_query = query.clone();
        activation_query.at_time = Some(effective_at);
        activation_query.limits.max_nodes = graph_budget;
        activation_query.limits.timeout_ms = u64::try_from(remaining.as_millis())
            .unwrap_or(u64::MAX)
            .max(1);
        let result = engine.recall(activation_query).await?;
        let explain = engine.explain(&result, query.limits);
        if !exact_state_loaded {
            let source_fragments = activated_fragment_ids(&result, &points_by_id)
                .into_iter()
                .collect::<Vec<_>>();
            let remaining_nodes = query.limits.max_nodes.saturating_sub(result.points.len());
            states = self
                .projection
                .load_states(
                    &query.scope,
                    StateSelector::SourceFragments(source_fragments),
                    effective_at,
                    remaining_nodes,
                )
                .await?;
        }
        crate::security::register_projected_state_scopes(
            &query.scope,
            &states,
            &mut fragment_scopes,
        )?;
        let activated_points = result
            .points
            .iter()
            .map(|point| point.point_id)
            .collect::<Vec<_>>();
        let activated_fragments = activated_fragment_ids(&result, &points_by_id)
            .into_iter()
            .collect::<Vec<_>>();
        let used_nodes = result
            .points
            .len()
            .saturating_add(states.current.len())
            .saturating_add(states.history.len());
        let remaining_nodes = query.limits.max_nodes.saturating_sub(used_nodes);
        let procedure_key = (query.mode == RecallMode::Procedural).then_some(query.text.as_str());
        let derivations = self
            .projection
            .load_recall_derivations(
                &query.scope,
                &activated_points,
                &activated_fragments,
                procedure_key,
                effective_at,
                remaining_nodes,
            )
            .await?;
        crate::security::register_projected_derivation_scopes(
            &query.scope,
            &derivations,
            &mut fragment_scopes,
        )?;
        let packet = build_packet(
            &query,
            &result,
            &points_by_id,
            RecallPacketInputs {
                episodes,
                evidence,
                states,
                derivations,
                failure_fragments: &failure_fragments,
            },
        )?;
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

async fn with_recall_deadline<T, F>(deadline: Instant, future: F) -> Result<T, MemoryError>
where
    F: Future<Output = Result<T, MemoryError>>,
{
    tokio::time::timeout_at(deadline.into(), future)
        .await
        .map_err(|_| MemoryError::Recall(RecallEngineError::TimedOut))?
}

fn build_packet(
    query: &RecallQuery,
    result: &RecallResult,
    points_by_id: &BTreeMap<PointId, IndexedPoint>,
    inputs: RecallPacketInputs<'_>,
) -> Result<MemoryPacket, serde_json::Error> {
    let RecallPacketInputs {
        episodes,
        mut evidence,
        states,
        derivations,
        failure_fragments,
    } = inputs;
    let mut packet = MemoryPacket::empty(RecallId::new(), query.mode);
    let activated_fragment_ids = activated_fragment_ids(result, points_by_id);
    packet.episodes = episodes
        .into_iter()
        .filter(|episode| activated_fragment_ids.contains(&episode.fragment_id))
        .collect();
    packet.past_failures = packet
        .episodes
        .iter()
        .filter(|episode| failure_fragments.contains(&episode.fragment_id))
        .cloned()
        .collect();
    evidence.retain(|item| activated_fragment_ids.contains(&item.fragment_id));
    evidence.extend(states.evidence);
    evidence.extend(derivations.evidence);
    evidence.sort_by_key(|item| (item.fragment_id, item.event_id, item.evidence_ref_id));
    evidence.dedup_by_key(|item| (item.fragment_id, item.event_id, item.evidence_ref_id));
    packet.evidence = evidence;
    packet.current_states = states.current;
    packet.historical_states = states.history;
    packet.conflicts.extend(states.conflicts);
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
    packet.activated_surfaces = derivations.surfaces;
    packet.procedures = derivations.procedures;
    packet.constraints = derivations.constraints;
    packet.preferences = derivations.preferences;
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
    packet.coverage.truncated = result.trace.truncated || states.truncated || derivations.truncated;
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

fn activated_fragment_ids(
    result: &RecallResult,
    points_by_id: &BTreeMap<PointId, IndexedPoint>,
) -> BTreeSet<FragmentId> {
    result
        .points
        .iter()
        .filter_map(|activated| points_by_id.get(&activated.point_id))
        .flat_map(|indexed| indexed.point.source_fragments.iter().copied())
        .collect()
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

#[cfg(test)]
mod tests {
    use super::with_recall_deadline;
    use std::time::{Duration, Instant};

    #[tokio::test]
    async fn recall_deadline_covers_the_full_async_pipeline() {
        let deadline = Instant::now() + Duration::from_millis(1);
        let result = with_recall_deadline(deadline, async {
            tokio::time::sleep(Duration::from_millis(20)).await;
            Ok::<_, crate::MemoryError>(())
        })
        .await;

        assert!(matches!(
            result,
            Err(crate::MemoryError::Recall(
                jc_memory_activation::RecallEngineError::TimedOut
            ))
        ));
    }
}
