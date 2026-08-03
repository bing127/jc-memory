//! 完全离线且可重复的规则与词法哈希实现。

use async_trait::async_trait;
use jc_memory_activation::{IndexedPoint, RecallQuery};
use jc_memory_graph::{CognitivePoint, EdgeCandidate};
use jc_memory_protocol::{
    CognitiveFragment, CognitiveStage, FragmentContent, FragmentKind, RecallMode,
};
use jc_memory_surface::CognitiveSurface;
use sha2::{Digest, Sha256};

use crate::routing::FallbackRoutingPolicy;
use crate::{
    Classification, ContradictionDetector, ContradictionScore, DevicePolicy, EdgePredictor,
    Embedding, EventClassifier, FragmentEncoder, ImportancePredictor, ImportanceScore,
    ModelManifest, ModelManifestSpec, NeuralError, QueryRouter, RankedCandidate, Reranker,
    RoutingDecision, SurfaceAssigner, SurfaceScore,
};

const EMBEDDING_DIMENSION: usize = 64;
const MAX_BATCH_SIZE: usize = 256;
const DEFAULT_TIMEOUT_MS: u64 = 250;
const PREFIX: &str = "jc-memory-neural/offline-";

/// 实现全部八个 Trait 的确定性离线 Provider。
#[derive(Clone, Debug)]
pub struct DeterministicFallback {
    encoder: ModelManifest,
    classifier: ModelManifest,
    importance: ModelManifest,
    edge: ModelManifest,
    router: ModelManifest,
    reranker: ModelManifest,
    surface: ModelManifest,
    contradiction: ModelManifest,
    routing_policy: FallbackRoutingPolicy,
}

impl Default for DeterministicFallback {
    fn default() -> Self {
        Self {
            encoder: manifest("encoder", "fragment-batch", "embedding-batch"),
            classifier: manifest("classifier", "fragment-batch", "classification-batch"),
            importance: manifest("importance", "fragment-batch", "importance-batch"),
            edge: manifest("edge", "edge-candidate-batch", "edge-candidate-batch"),
            router: manifest("router", "recall-query", "routing-decision"),
            reranker: manifest("reranker", "recall-candidates", "ranked-candidates"),
            surface: manifest("surface", "point-surface-batch", "surface-score-batch"),
            contradiction: manifest(
                "contradiction",
                "fragment-batch",
                "contradiction-score-batch",
            ),
            routing_policy: FallbackRoutingPolicy::builtin().expect("内置离线路由策略必须始终有效"),
        }
    }
}

impl DeterministicFallback {
    /// 创建使用显式离线路由策略的 Fallback。
    #[must_use]
    pub fn with_routing_policy(policy: FallbackRoutingPolicy) -> Self {
        Self {
            routing_policy: policy,
            ..Self::default()
        }
    }
}

#[async_trait]
impl FragmentEncoder for DeterministicFallback {
    async fn encode(&self, batch: &[CognitiveFragment]) -> Result<Vec<Embedding>, NeuralError> {
        validate_batch(batch.len(), self.encoder.max_batch_size)?;
        Ok(batch
            .iter()
            .map(|fragment| Embedding {
                fragment_id: fragment.id,
                values: lexical_embedding(&fragment_text(fragment)),
                model_version: self.encoder.model_version.clone(),
            })
            .collect())
    }

    fn manifest(&self) -> &ModelManifest {
        &self.encoder
    }
}

#[async_trait]
impl EventClassifier for DeterministicFallback {
    async fn classify(
        &self,
        batch: &[CognitiveFragment],
    ) -> Result<Vec<Classification>, NeuralError> {
        validate_batch(batch.len(), self.classifier.max_batch_size)?;
        Ok(batch
            .iter()
            .map(|fragment| Classification {
                fragment_id: fragment.id,
                stage: stage_for(fragment),
                model_version: self.classifier.model_version.clone(),
            })
            .collect())
    }

    fn manifest(&self) -> &ModelManifest {
        &self.classifier
    }
}

#[async_trait]
impl ImportancePredictor for DeterministicFallback {
    async fn predict_importance(
        &self,
        batch: &[CognitiveFragment],
    ) -> Result<Vec<ImportanceScore>, NeuralError> {
        validate_batch(batch.len(), self.importance.max_batch_size)?;
        Ok(batch
            .iter()
            .map(|fragment| ImportanceScore {
                fragment_id: fragment.id,
                score: (fragment.importance * 0.7 + fragment.confidence * 0.3).clamp(0.0, 1.0),
                model_version: self.importance.model_version.clone(),
            })
            .collect())
    }

    fn manifest(&self) -> &ModelManifest {
        &self.importance
    }
}

#[async_trait]
impl EdgePredictor for DeterministicFallback {
    async fn predict_edges(
        &self,
        batch: &[EdgeCandidate],
    ) -> Result<Vec<EdgeCandidate>, NeuralError> {
        validate_batch(batch.len(), self.edge.max_batch_size)?;
        Ok(batch
            .iter()
            .cloned()
            .map(|mut edge| {
                edge.model_version_id = Some(self.edge.model_version.clone());
                edge
            })
            .collect())
    }

    fn manifest(&self) -> &ModelManifest {
        &self.edge
    }
}

#[async_trait]
impl QueryRouter for DeterministicFallback {
    async fn route(&self, query: &RecallQuery) -> Result<RoutingDecision, NeuralError> {
        let normalized = normalize(&query.text);
        let mode = if query.at_time.is_some() {
            RecallMode::HistoricalState
        } else if self.routing_policy.matches_causal(&normalized) {
            RecallMode::Causal
        } else if self.routing_policy.matches_current_state(&normalized) {
            RecallMode::CurrentState
        } else if self.routing_policy.matches_procedural(&normalized) {
            RecallMode::Procedural
        } else if !query.entities.is_empty() {
            RecallMode::Entity
        } else {
            query.mode
        };
        Ok(RoutingDecision {
            mode,
            model_version: self.router.model_version.clone(),
        })
    }

    fn manifest(&self) -> &ModelManifest {
        &self.router
    }
}

#[async_trait]
impl Reranker for DeterministicFallback {
    async fn rerank(
        &self,
        query: &RecallQuery,
        candidates: &[IndexedPoint],
    ) -> Result<Vec<RankedCandidate>, NeuralError> {
        validate_batch(candidates.len(), self.reranker.max_batch_size)?;
        let query_terms = terms(&query.text);
        let mut ranked: Vec<_> = candidates
            .iter()
            .map(|candidate| RankedCandidate {
                point_id: candidate.point.id,
                score: lexical_overlap(&query_terms, &terms(&candidate.text)),
                model_version: self.reranker.model_version.clone(),
            })
            .collect();
        ranked.sort_by(|left, right| {
            right
                .score
                .total_cmp(&left.score)
                .then_with(|| left.point_id.cmp(&right.point_id))
        });
        Ok(ranked)
    }

    fn manifest(&self) -> &ModelManifest {
        &self.reranker
    }
}

#[async_trait]
impl SurfaceAssigner for DeterministicFallback {
    async fn assign(
        &self,
        points: &[CognitivePoint],
        surfaces: &[CognitiveSurface],
    ) -> Result<Vec<SurfaceScore>, NeuralError> {
        validate_batch(
            points.len().saturating_mul(surfaces.len()),
            self.surface.max_batch_size,
        )?;
        let mut scores = Vec::new();
        for point in points {
            for surface in surfaces
                .iter()
                .filter(|surface| surface.scope == point.scope)
            {
                let score = surface
                    .current()
                    .member_points
                    .get(&point.id)
                    .copied()
                    .unwrap_or(0.0);
                if score > 0.0 {
                    scores.push(SurfaceScore {
                        point_id: point.id,
                        surface_id: surface.id,
                        score: score.clamp(0.0, 1.0),
                        model_version: self.surface.model_version.clone(),
                    });
                }
            }
        }
        Ok(scores)
    }

    fn manifest(&self) -> &ModelManifest {
        &self.surface
    }
}

#[async_trait]
impl ContradictionDetector for DeterministicFallback {
    async fn detect(
        &self,
        batch: &[CognitiveFragment],
    ) -> Result<Vec<ContradictionScore>, NeuralError> {
        validate_batch(batch.len(), self.contradiction.max_batch_size)?;
        let mut scores = Vec::new();
        for (index, left) in batch.iter().enumerate() {
            for right in &batch[index + 1..] {
                if left.scope != right.scope {
                    continue;
                }
                scores.push(ContradictionScore {
                    left_fragment_id: left.id,
                    right_fragment_id: right.id,
                    score: contradiction_score(&fragment_text(left), &fragment_text(right)),
                    model_version: self.contradiction.model_version.clone(),
                });
            }
        }
        Ok(scores)
    }

    fn manifest(&self) -> &ModelManifest {
        &self.contradiction
    }
}

fn manifest(kind: &str, input: &str, output: &str) -> ModelManifest {
    ModelManifest::new(ModelManifestSpec {
        model_version: format!("{PREFIX}{kind}-v1"),
        provider: "jc-memory".into(),
        name: format!("deterministic-{kind}"),
        version: "1".into(),
        input_schema: format!("jc-memory://neural/{input}/v1"),
        output_schema: format!("jc-memory://neural/{output}/v1"),
        device_policy: DevicePolicy::CpuOnly,
        max_batch_size: MAX_BATCH_SIZE,
        timeout_ms: DEFAULT_TIMEOUT_MS,
    })
    .expect("内置确定性模型清单必须始终有效")
}

fn validate_batch(actual: usize, maximum: usize) -> Result<(), NeuralError> {
    if actual > maximum {
        Err(NeuralError::BatchTooLarge { actual, maximum })
    } else {
        Ok(())
    }
}

fn lexical_embedding(text: &str) -> Vec<f32> {
    let mut values = vec![0.0_f32; EMBEDDING_DIMENSION];
    for term in terms(text) {
        let digest = Sha256::digest(term.as_bytes());
        let bucket = usize::from(digest[0]) % EMBEDDING_DIMENSION;
        let sign = if digest[1] & 1 == 0 { 1.0 } else { -1.0 };
        values[bucket] += sign;
    }
    let norm = values.iter().map(|value| value * value).sum::<f32>().sqrt();
    if norm > 0.0 {
        for value in &mut values {
            *value /= norm;
        }
    }
    values
}

fn stage_for(fragment: &CognitiveFragment) -> CognitiveStage {
    match fragment.kind {
        FragmentKind::Prediction | FragmentKind::Plan | FragmentKind::Inference => {
            CognitiveStage::FutureHypothesis
        }
        FragmentKind::Decision | FragmentKind::Approval | FragmentKind::Rejection => {
            CognitiveStage::RuntimeDecision
        }
        FragmentKind::ToolCall | FragmentKind::Action => CognitiveStage::ToolAction,
        FragmentKind::ToolResult
        | FragmentKind::Outcome
        | FragmentKind::Failure
        | FragmentKind::Success => CognitiveStage::ActualResult,
        FragmentKind::StateTransition => CognitiveStage::ResultVerification,
        FragmentKind::Fact
        | FragmentKind::Correction
        | FragmentKind::Constraint
        | FragmentKind::Preference
            if fragment.validity == jc_memory_protocol::ValidityStatus::Verified =>
        {
            CognitiveStage::PastEvidence
        }
        FragmentKind::Observation
        | FragmentKind::Claim
        | FragmentKind::Fact
        | FragmentKind::Correction
        | FragmentKind::Constraint
        | FragmentKind::Preference => CognitiveStage::CurrentObservation,
    }
}

fn fragment_text(fragment: &CognitiveFragment) -> String {
    match &fragment.content {
        FragmentContent::Text { text } => text.clone(),
        FragmentContent::Json { value } => serde_json::to_string(value).unwrap_or_default(),
        FragmentContent::Blob {
            blob_id,
            media_type,
            byte_len,
        } => {
            format!("blob:{blob_id}:{media_type}:{byte_len}")
        }
    }
}

fn normalize(value: &str) -> String {
    value
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn terms(value: &str) -> Vec<String> {
    normalize(value)
        .split(|character: char| !character.is_alphanumeric())
        .filter(|term| !term.is_empty())
        .map(str::to_owned)
        .collect()
}

fn lexical_overlap(left: &[String], right: &[String]) -> f32 {
    if left.is_empty() {
        return 0.0;
    }
    let (matches, total) = left
        .iter()
        .fold((0.0_f32, 0.0_f32), |(matches, total), term| {
            let increment = if right.contains(term) { 1.0 } else { 0.0 };
            (matches + increment, total + 1.0)
        });
    (matches / total).clamp(0.0, 1.0)
}

fn contradiction_score(left: &str, right: &str) -> f32 {
    let left = normalize(left);
    let right = normalize(right);
    let left_negated = strip_negation(&left);
    let right_negated = strip_negation(&right);
    if left_negated.is_some_and(|value| value == right)
        || right_negated.is_some_and(|value| value == left)
    {
        1.0
    } else {
        0.0
    }
}

fn strip_negation(value: &str) -> Option<&str> {
    value
        .strip_prefix("not ")
        .or_else(|| value.strip_prefix("不 "))
        .or_else(|| value.strip_prefix('不'))
}
