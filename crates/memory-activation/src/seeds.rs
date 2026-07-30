//! Deterministic searchable Point index records and basic seed selection.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use jc_memory_graph::CognitivePoint;
use jc_memory_protocol::{MemoryScope, PointId, RecallMode, ValidityStatus};

use crate::RecallQuery;

/// A derived Point paired with read-only search material from its source Fragment.
#[derive(Clone, Debug, PartialEq)]
pub struct IndexedPoint {
    /// Evidence-backed graph Point.
    pub point: CognitivePoint,
    /// Canonical searchable text; it never replaces source Fragment content.
    pub text: String,
    /// Canonical entity names associated with the Point.
    pub entities: Vec<String>,
}

/// One deterministically scored initial activation.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SeedCandidate {
    pub(crate) point_id: PointId,
    pub(crate) score: f32,
    pub(crate) occurred_at: DateTime<Utc>,
}

pub(crate) struct SeedSelection {
    pub(crate) candidates: Vec<SeedCandidate>,
    pub(crate) truncated: bool,
}

pub(crate) fn select<'a>(
    points: impl Iterator<Item = &'a IndexedPoint>,
    query: &RecallQuery,
    effective_at: DateTime<Utc>,
) -> SeedSelection {
    let query_text = normalize(&query.text);
    let query_entities: Vec<_> = query
        .entities
        .iter()
        .map(|entity| normalize(entity))
        .filter(|entity| !entity.is_empty())
        .collect();
    let mut merged = BTreeMap::<PointId, (&IndexedPoint, f32)>::new();
    for indexed in points.filter(|indexed| eligible(indexed, &query.scope, effective_at)) {
        let text = normalize(&indexed.text);
        let mut scores = seed_scores(query.mode, &query_text, &text);
        if indexed
            .entities
            .iter()
            .map(|entity| normalize(entity))
            .any(|entity| query_entities.contains(&entity))
        {
            scores.push(1.0);
        }
        for score in scores {
            let entry = merged.entry(indexed.point.id).or_insert((indexed, score));
            entry.1 = entry.1.max(score);
        }
    }
    let mut candidates: Vec<_> = merged.into_values().collect();
    candidates.sort_by(|(left, left_score), (right, right_score)| {
        right_score
            .total_cmp(left_score)
            .then_with(|| right.point.occurred_at.cmp(&left.point.occurred_at))
            .then_with(|| left.point.id.cmp(&right.point.id))
    });
    let limit = query.limits.max_seeds.min(query.limits.max_nodes);
    let truncated = candidates.len() > limit;
    let candidates = candidates
        .into_iter()
        .take(limit)
        .map(|(indexed, score)| SeedCandidate {
            point_id: indexed.point.id,
            score,
            occurred_at: indexed.point.occurred_at,
        })
        .collect();
    SeedSelection {
        candidates,
        truncated,
    }
}

fn seed_scores(mode: RecallMode, query: &str, document: &str) -> Vec<f32> {
    if query.is_empty() {
        return Vec::new();
    }
    let mut scores = Vec::new();
    if document == query {
        scores.push(1.0);
    } else if document.contains(query) {
        scores.push(0.95);
    }
    if mode != RecallMode::Exact {
        let query_terms = terms(query);
        let document_terms = terms(document);
        if !query_terms.is_empty() {
            let (matches, total) =
                query_terms
                    .iter()
                    .fold((0.0_f32, 0.0_f32), |(matches, total), term| {
                        let matched = if document_terms.contains(term) {
                            1.0
                        } else {
                            0.0
                        };
                        (matches + matched, total + 1.0)
                    });
            if matches > 0.0 {
                let coverage = matches / total;
                scores.push(0.7 + 0.2 * coverage);
            }
        }
        let sparse_score = sparse_similarity(query, document);
        if sparse_score >= 0.2 {
            scores.push(0.6 + 0.2 * sparse_score);
        }
    }
    scores
}

fn terms(value: &str) -> Vec<String> {
    value
        .split(|character: char| !character.is_alphanumeric())
        .filter(|term| !term.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn sparse_similarity(left: &str, right: &str) -> f32 {
    let left = sparse_features(left);
    let right = sparse_features(right);
    let dot: f32 = left
        .iter()
        .filter_map(|(feature, left_count)| {
            right
                .get(feature)
                .map(|right_count| left_count * right_count)
        })
        .sum();
    let left_norm = left.values().map(|value| value * value).sum::<f32>().sqrt();
    let right_norm = right
        .values()
        .map(|value| value * value)
        .sum::<f32>()
        .sqrt();
    if left_norm > 0.0 && right_norm > 0.0 {
        dot / (left_norm * right_norm)
    } else {
        0.0
    }
}

fn sparse_features(value: &str) -> BTreeMap<String, f32> {
    let compact: Vec<_> = value
        .chars()
        .flat_map(char::to_lowercase)
        .filter(|character| character.is_alphanumeric())
        .collect();
    let width = compact.len().min(3);
    if width == 0 {
        return BTreeMap::new();
    }
    let mut features = BTreeMap::new();
    for window in compact.windows(width) {
        let feature: String = window.iter().collect();
        *features.entry(feature).or_insert(0.0) += 1.0;
    }
    features
}

pub(crate) fn eligible(
    indexed: &IndexedPoint,
    scope: &MemoryScope,
    effective_at: DateTime<Utc>,
) -> bool {
    indexed.point.scope == *scope
        && !matches!(
            indexed.point.validity,
            ValidityStatus::Invalidated | ValidityStatus::Quarantined
        )
        && indexed.point.occurred_at <= effective_at
}

pub(crate) fn normalize(value: &str) -> String {
    value
        .split_whitespace()
        .map(str::to_lowercase)
        .collect::<Vec<_>>()
        .join(" ")
}
