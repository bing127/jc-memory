//! 确定性离线神经接口、模型清单、降级与熔断行为测试。

use std::{future, time::Duration};

use chrono::{TimeZone as _, Utc};
use jc_memory_activation::{IndexedPoint, RecallLimits, RecallQuery};
use jc_memory_graph::{CognitiveEdge, CognitivePoint, EdgeCandidate, EvidenceLink};
use jc_memory_neural::{
    CircuitBreaker, CircuitState, ContradictionDetector, DeterministicFallback, DevicePolicy,
    EdgePredictor, EventClassifier, FallbackReason, FragmentEncoder, ImportancePredictor,
    ModelManifest, ModelManifestSpec, QueryRouter, Reranker, SurfaceAssigner,
    with_timeout_fallback,
};
use jc_memory_protocol::{
    CognitiveFragment, FragmentContent, FragmentId, FragmentKind, FragmentSource, MemoryScope,
    NamespaceId, Perspective, PointId, RecallMode, SecurityLabel, SourceType, SurfaceId,
    SurfaceKind, SurfaceStatus, ValidityStatus,
};
use jc_memory_surface::{CognitiveSurface, SurfaceBoundary, SurfaceVersion};

const MODEL_PREFIX: &str = "jc-memory-neural/offline-";

#[tokio::test]
async fn all_eight_traits_are_public_and_implemented_offline() {
    fn assert_encoder<T: FragmentEncoder>() {}
    fn assert_classifier<T: EventClassifier>() {}
    fn assert_importance<T: ImportancePredictor>() {}
    fn assert_edges<T: EdgePredictor>() {}
    fn assert_router<T: QueryRouter>() {}
    fn assert_reranker<T: Reranker>() {}
    fn assert_surface<T: SurfaceAssigner>() {}
    fn assert_contradiction<T: ContradictionDetector>() {}

    assert_encoder::<DeterministicFallback>();
    assert_classifier::<DeterministicFallback>();
    assert_importance::<DeterministicFallback>();
    assert_edges::<DeterministicFallback>();
    assert_router::<DeterministicFallback>();
    assert_reranker::<DeterministicFallback>();
    assert_surface::<DeterministicFallback>();
    assert_contradiction::<DeterministicFallback>();

    let fallback = DeterministicFallback::default();
    let manifests = [
        FragmentEncoder::manifest(&fallback),
        EventClassifier::manifest(&fallback),
        ImportancePredictor::manifest(&fallback),
        EdgePredictor::manifest(&fallback),
        QueryRouter::manifest(&fallback),
        Reranker::manifest(&fallback),
        SurfaceAssigner::manifest(&fallback),
        ContradictionDetector::manifest(&fallback),
    ];
    assert!(manifests.iter().all(|manifest| manifest.verify_checksum()));
    let versions: std::collections::BTreeSet<_> = manifests
        .iter()
        .map(|manifest| manifest.model_version.as_str())
        .collect();
    assert_eq!(versions.len(), 8);
}

#[tokio::test]
async fn same_input_and_batch_order_produce_stable_embeddings() {
    let fallback = DeterministicFallback::default();
    let first = fragment("alpha beta", FragmentKind::Observation);
    let second = fragment("gamma delta", FragmentKind::Claim);

    let one = fallback
        .encode(&[first.clone(), second.clone()])
        .await
        .unwrap();
    let two = fallback
        .encode(&[first.clone(), second.clone()])
        .await
        .unwrap();

    assert_eq!(one, two);
    assert_eq!(one[0].fragment_id, first.id);
    assert_eq!(one[1].fragment_id, second.id);
    assert!(
        one.iter()
            .all(|value| value.model_version.starts_with(MODEL_PREFIX))
    );
}

#[tokio::test]
async fn rule_classification_keeps_prediction_unverified() {
    let fallback = DeterministicFallback::default();
    let prediction = fragment("可能发生", FragmentKind::Prediction);
    let classified = fallback.classify(&[prediction]).await.unwrap();

    assert_eq!(
        classified[0].stage,
        jc_memory_protocol::CognitiveStage::FutureHypothesis
    );
    assert_eq!(
        classified[0].model_version,
        "jc-memory-neural/offline-classifier-v1"
    );
}

#[tokio::test]
async fn rule_classification_does_not_upgrade_unverified_fact() {
    let fallback = DeterministicFallback::default();
    let fact = fragment("尚未核验的陈述", FragmentKind::Fact);
    let classified = fallback.classify(&[fact]).await.unwrap();

    assert_eq!(
        classified[0].stage,
        jc_memory_protocol::CognitiveStage::CurrentObservation
    );
}

#[tokio::test]
async fn every_derived_output_carries_model_version() {
    let fallback = DeterministicFallback::default();
    let left = fragment("service available", FragmentKind::Observation);
    let mut right = fragment("not service available", FragmentKind::Claim);
    right.scope = left.scope.clone();
    let points = point_pair(&left, &right);
    let edge = edge_candidate(&points[0], &points[1], left.id);
    let query = query("service");
    let indexed = indexed_points(&points);
    let surface = surface(&points[0]);

    let importance = fallback
        .predict_importance(std::slice::from_ref(&left))
        .await
        .unwrap();
    let edges = fallback.predict_edges(&[edge]).await.unwrap();
    let route = fallback.route(&query).await.unwrap();
    let reranked = fallback.rerank(&query, &indexed).await.unwrap();
    let surfaces = fallback.assign(&points, &[surface]).await.unwrap();
    let contradictions = fallback.detect(&[left, right]).await.unwrap();

    assert!(importance[0].model_version.starts_with(MODEL_PREFIX));
    assert_eq!(edges.len(), 1);
    assert!(
        edges[0]
            .model_version_id
            .as_deref()
            .is_some_and(|v| v.starts_with(MODEL_PREFIX))
    );
    assert!(route.model_version.starts_with(MODEL_PREFIX));
    assert!(
        reranked
            .iter()
            .all(|value| value.model_version.starts_with(MODEL_PREFIX))
    );
    assert_eq!(surfaces.len(), 1);
    assert!(
        surfaces
            .iter()
            .all(|value| value.model_version.starts_with(MODEL_PREFIX))
    );
    assert_eq!(contradictions.len(), 1);
    assert!(
        contradictions
            .iter()
            .all(|value| value.model_version.starts_with(MODEL_PREFIX))
    );
}

#[tokio::test]
async fn stable_reranking_uses_point_id_as_tie_breaker() {
    let fallback = DeterministicFallback::default();
    let left = fragment("same", FragmentKind::Fact);
    let right = fragment("same", FragmentKind::Fact);
    let points = point_pair(&left, &right);
    let mut indexed = indexed_points(&points);
    indexed.reverse();

    let ranked = fallback.rerank(&query("same"), &indexed).await.unwrap();
    let expected_first = points[0].id.min(points[1].id);

    assert_eq!(ranked[0].point_id, expected_first);
    assert!((ranked[0].score - ranked[1].score).abs() < f32::EPSILON);
}

#[test]
fn manifest_checksum_covers_required_contract() {
    let mut manifest = ModelManifest::new(ModelManifestSpec {
        model_version: "fixture-v1".into(),
        provider: "fixture".into(),
        name: "fixture-model".into(),
        version: "1".into(),
        input_schema: "fixture://input/v1".into(),
        output_schema: "fixture://output/v1".into(),
        device_policy: DevicePolicy::CpuOnly,
        max_batch_size: 16,
        timeout_ms: 25,
    })
    .unwrap();

    assert!(manifest.verify_checksum());
    assert_eq!(manifest.checksum_sha256.len(), 64);
    manifest.timeout_ms += 1;
    assert!(!manifest.verify_checksum());
}

#[tokio::test]
async fn timeout_falls_back_and_opens_circuit() {
    let now = Duration::from_secs(10);
    let mut breaker = CircuitBreaker::new(Duration::from_secs(5));

    let result = with_timeout_fallback(
        &mut breaker,
        now,
        Duration::ZERO,
        future::pending::<Result<&str, jc_memory_neural::NeuralError>>(),
        || async { Ok("offline") },
    )
    .await
    .unwrap();

    assert_eq!(result.value, "offline");
    assert_eq!(result.reason, Some(FallbackReason::Timeout));
    assert_eq!(breaker.state(), CircuitState::Open);
}

#[tokio::test]
async fn failure_falls_back_and_opens_circuit() {
    let mut breaker = CircuitBreaker::new(Duration::from_secs(5));
    let result = with_timeout_fallback(
        &mut breaker,
        Duration::ZERO,
        Duration::from_secs(1),
        async { Err(jc_memory_neural::NeuralError::Provider("offline".into())) },
        || async { Ok(42_u8) },
    )
    .await
    .unwrap();

    assert_eq!(result.value, 42);
    assert_eq!(result.reason, Some(FallbackReason::PrimaryFailure));
    assert_eq!(breaker.state(), CircuitState::Open);
}

#[test]
fn circuit_breaker_uses_explicit_time_for_three_states() {
    let cooldown = Duration::from_secs(5);
    let opened_at = Duration::from_secs(10);
    let mut breaker = CircuitBreaker::new(cooldown);

    assert!(breaker.allow_request(opened_at));
    breaker.record_failure(opened_at);
    assert_eq!(breaker.state(), CircuitState::Open);
    assert!(!breaker.allow_request(opened_at + Duration::from_secs(4)));
    assert!(breaker.allow_request(opened_at + cooldown));
    assert_eq!(breaker.state(), CircuitState::HalfOpen);
    breaker.record_failure(opened_at + cooldown);
    assert_eq!(breaker.state(), CircuitState::Open);
    assert!(breaker.allow_request(opened_at + cooldown + cooldown));
    breaker.record_success();
    assert_eq!(breaker.state(), CircuitState::Closed);
}

fn fragment(text: &str, kind: FragmentKind) -> CognitiveFragment {
    let now = Utc.timestamp_opt(1_700_000_000, 0).single().unwrap();
    CognitiveFragment {
        id: FragmentId::new(),
        scope: MemoryScope::new(NamespaceId::new()),
        session_id: None,
        trace_id: None,
        kind,
        content: FragmentContent::Text { text: text.into() },
        source: FragmentSource {
            source_type: SourceType::System,
            source_id: "fixture".into(),
            actor_id: None,
            observer_id: None,
            tool_id: None,
            uri: None,
        },
        perspective: Perspective::System,
        occurred_at: now,
        observed_at: now,
        confidence: 0.8,
        importance: 0.5,
        validity: ValidityStatus::Unverified,
        security_label: SecurityLabel::Internal,
        evidence: Vec::new(),
        metadata: serde_json::Map::new(),
    }
}

fn point_pair(left: &CognitiveFragment, right: &CognitiveFragment) -> [CognitivePoint; 2] {
    let mut right = right.clone();
    right.scope = left.scope.clone();
    [
        CognitivePoint::try_from(left).unwrap_or_else(|_| point(left)),
        CognitivePoint::try_from(&right).unwrap_or_else(|_| point(&right)),
    ]
}

fn point(fragment: &CognitiveFragment) -> CognitivePoint {
    CognitivePoint {
        id: PointId::new(),
        scope: fragment.scope.clone(),
        kind: jc_memory_protocol::PointKind::Fact,
        source_fragments: vec![fragment.id],
        occurred_at: fragment.occurred_at,
        confidence: fragment.confidence,
        validity: fragment.validity,
    }
}

fn edge_candidate(
    source: &CognitivePoint,
    target: &CognitivePoint,
    fragment_id: FragmentId,
) -> EdgeCandidate {
    CognitiveEdge::co_activated(
        source,
        target,
        EvidenceLink {
            fragment_id,
            event_id: None,
            evidence_ref_id: None,
        },
    )
    .unwrap()
}

fn query(text: &str) -> RecallQuery {
    RecallQuery {
        scope: MemoryScope::new(NamespaceId::new()),
        mode: RecallMode::Semantic,
        text: text.into(),
        entities: Vec::new(),
        at_time: None,
        limits: RecallLimits {
            max_seeds: 8,
            max_neighbors: 8,
            max_hops: 2,
            max_nodes: 32,
            timeout_ms: 100,
        },
    }
}

fn indexed_points(points: &[CognitivePoint; 2]) -> Vec<IndexedPoint> {
    points
        .iter()
        .cloned()
        .map(|point| IndexedPoint {
            point,
            text: "same".into(),
            entities: Vec::new(),
        })
        .collect()
}

fn surface(point: &CognitivePoint) -> CognitiveSurface {
    let now = Utc.timestamp_opt(1_700_000_000, 0).single().unwrap();
    CognitiveSurface {
        id: SurfaceId::new(),
        scope: point.scope.clone(),
        kind: SurfaceKind::Dynamic,
        versions: vec![SurfaceVersion {
            version: 1,
            semantic_center: Vec::new(),
            member_points: [(point.id, 0.75)].into_iter().collect(),
            member_edges: Vec::new(),
            activation_threshold: 0.5,
            valid_from: now,
            valid_to: None,
            status: SurfaceStatus::Active,
            confidence: 0.8,
            boundary: SurfaceBoundary {
                min_member_weight: 0.1,
                max_members: 8,
            },
            evidence: Vec::new(),
            model_version_id: None,
        }],
    }
}
