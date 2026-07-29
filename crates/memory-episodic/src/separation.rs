//! Deterministic pattern separation for candidate cognitive points.

use std::collections::BTreeSet;

use jc_memory_graph::CognitivePoint;
use jc_memory_protocol::{CognitiveFragment, FragmentContent, PointId};
use serde_json::Value;
use sha2::{Digest as _, Sha256};

const CONTENT_WEIGHT: f32 = 0.45;
const ENTITY_WEIGHT: f32 = 0.15;
const TEMPORAL_WEIGHT: f32 = 0.10;
const PROJECT_WEIGHT: f32 = 0.10;
const SOURCE_WEIGHT: f32 = 0.10;
const STATE_WEIGHT: f32 = 0.10;
const DEFAULT_LINK_THRESHOLD: f32 = 0.75;
const TIME_BUCKET_SECONDS: i64 = 5 * 60;

/// Transient source signals used to compare one prospective point.
///
/// These signals remain derived input and do not replace the authoritative
/// fragment or become additional fields on [`CognitivePoint`].
#[derive(Clone, Debug, PartialEq)]
pub struct SeparationInput {
    /// Original authoritative fragment.
    pub fragment: CognitiveFragment,
    /// Explicit stable entity identities found by an upstream extractor.
    pub entity_keys: Vec<String>,
    /// Optional stable signature of the state represented by the fragment.
    pub state_signature: Option<String>,
}

/// An existing point together with the transient signals that produced it.
#[derive(Clone, Debug, PartialEq)]
pub struct SeparationCandidate {
    /// Existing derived point.
    pub point: CognitivePoint,
    /// Source signals retained for deterministic comparison.
    pub input: SeparationInput,
}

/// Six independently inspectable pattern-separation dimensions.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SeparationScore {
    /// Token Jaccard similarity of canonical content.
    pub content: f32,
    /// Jaccard similarity of explicit entity identities.
    pub entity: f32,
    /// Similarity of five-minute occurrence buckets.
    pub temporal: f32,
    /// Whether both fragments belong to the same project.
    pub project: f32,
    /// Whether full source provenance matches.
    pub source: f32,
    /// Whether state signatures match.
    pub state: f32,
}

impl SeparationScore {
    /// Returns the deterministic weighted score used for similar-point links.
    #[must_use]
    pub fn total(self) -> f32 {
        self.content * CONTENT_WEIGHT
            + self.entity * ENTITY_WEIGHT
            + self.temporal * TEMPORAL_WEIGHT
            + self.project * PROJECT_WEIGHT
            + self.source * SOURCE_WEIGHT
            + self.state * STATE_WEIGHT
    }
}

/// The write-time action selected by pattern separation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SeparationDecision {
    /// Reuse an existing point representing the same occurrence.
    ExactDuplicate(PointId),
    /// Keep a distinct point and emit only a future low-confidence similarity candidate.
    LinkSimilar(PointId),
    /// Create a distinct point without a similarity candidate.
    CreateDistinct,
}

/// A deterministic separator that never infers causal relationships.
#[derive(Clone, Copy, Debug)]
pub struct PatternSeparator {
    link_threshold: f32,
}

impl Default for PatternSeparator {
    fn default() -> Self {
        Self {
            link_threshold: DEFAULT_LINK_THRESHOLD,
        }
    }
}

impl PatternSeparator {
    /// Chooses the closest same-scope candidate using stable point-ID tie breaking.
    #[must_use]
    pub fn decide(
        &self,
        input: &SeparationInput,
        candidates: &[SeparationCandidate],
    ) -> SeparationDecision {
        let eligible = candidates.iter().filter(|candidate| {
            candidate.point.scope == input.fragment.scope
                && candidate.input.fragment.scope == input.fragment.scope
        });

        if let Some(point_id) = eligible
            .clone()
            .filter(|candidate| is_exact_duplicate(input, candidate))
            .map(|candidate| candidate.point.id)
            .min()
        {
            return SeparationDecision::ExactDuplicate(point_id);
        }

        let best = eligible
            .map(|candidate| (self.score(input, candidate).total(), candidate.point.id))
            .filter(|(score, _)| *score >= self.link_threshold)
            .min_by(|left, right| {
                right
                    .0
                    .total_cmp(&left.0)
                    .then_with(|| left.1.cmp(&right.1))
            });

        best.map_or(SeparationDecision::CreateDistinct, |(_, point_id)| {
            SeparationDecision::LinkSimilar(point_id)
        })
    }

    /// Computes the six deterministic similarity dimensions for one candidate.
    #[must_use]
    pub fn score(
        &self,
        input: &SeparationInput,
        candidate: &SeparationCandidate,
    ) -> SeparationScore {
        debug_assert!((0.0..=1.0).contains(&self.link_threshold));
        let input_entities = normalized_entities(&input.entity_keys);
        let candidate_entities = normalized_entities(&candidate.input.entity_keys);
        let input_bucket = time_bucket(input.fragment.occurred_at.timestamp());
        let candidate_bucket = time_bucket(candidate.input.fragment.occurred_at.timestamp());

        SeparationScore {
            content: jaccard(
                &content_tokens(&input.fragment.content),
                &content_tokens(&candidate.input.fragment.content),
            ),
            entity: jaccard(&input_entities, &candidate_entities),
            temporal: match input_bucket.abs_diff(candidate_bucket) {
                0 => 1.0,
                1 => 0.5,
                _ => 0.0,
            },
            project: equality_score(
                &input.fragment.scope.project_id,
                &candidate.input.fragment.scope.project_id,
            ),
            source: equality_score(&input.fragment.source, &candidate.input.fragment.source),
            state: equality_score(&input.state_signature, &candidate.input.state_signature),
        }
    }
}

fn is_exact_duplicate(input: &SeparationInput, candidate: &SeparationCandidate) -> bool {
    canonical_content_hash(&input.fragment.content)
        == canonical_content_hash(&candidate.input.fragment.content)
        && input.fragment.scope == candidate.input.fragment.scope
        && normalized_entities(&input.entity_keys)
            == normalized_entities(&candidate.input.entity_keys)
        && input.fragment.source == candidate.input.fragment.source
        && time_bucket(input.fragment.occurred_at.timestamp())
            == time_bucket(candidate.input.fragment.occurred_at.timestamp())
        && input.state_signature == candidate.input.state_signature
}

fn canonical_content_hash(content: &FragmentContent) -> [u8; 32] {
    let mut hasher = Sha256::new();
    match content {
        FragmentContent::Text { text } => {
            hasher.update(b"text\0");
            hasher.update(text.as_bytes());
        }
        FragmentContent::Json { value } => {
            hasher.update(b"json\0");
            let canonical = canonicalize_json(value.clone());
            hasher.update(
                serde_json::to_vec(&canonical)
                    .expect("serializing a serde_json::Value into bytes cannot fail"),
            );
        }
        FragmentContent::Blob {
            blob_id,
            media_type,
            byte_len,
        } => {
            hasher.update(b"blob\0");
            hasher.update(blob_id.as_str().as_bytes());
            hasher.update([0]);
            hasher.update(media_type.as_bytes());
            hasher.update([0]);
            hasher.update(byte_len.to_be_bytes());
        }
    }
    hasher.finalize().into()
}

fn canonicalize_json(value: Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.into_iter().map(canonicalize_json).collect()),
        Value::Object(values) => Value::Object(
            values
                .into_iter()
                .map(|(key, value)| (key, canonicalize_json(value)))
                .collect(),
        ),
        scalar => scalar,
    }
}

fn content_tokens(content: &FragmentContent) -> BTreeSet<String> {
    match content {
        FragmentContent::Text { text } => tokenize(text),
        FragmentContent::Json { value } => tokenize(&canonicalize_json(value.clone()).to_string()),
        FragmentContent::Blob { blob_id, .. } => BTreeSet::from([blob_id.to_string()]),
    }
}

fn tokenize(value: &str) -> BTreeSet<String> {
    let mut tokens = BTreeSet::new();
    let mut ascii_word = String::new();
    for character in value.chars() {
        if character.is_ascii_alphanumeric() {
            ascii_word.push(character.to_ascii_lowercase());
        } else {
            if !ascii_word.is_empty() {
                tokens.insert(std::mem::take(&mut ascii_word));
            }
            if character.is_alphanumeric() {
                tokens.insert(character.to_lowercase().collect());
            }
        }
    }
    if !ascii_word.is_empty() {
        tokens.insert(ascii_word);
    }
    tokens
}

fn normalized_entities(entity_keys: &[String]) -> BTreeSet<String> {
    entity_keys
        .iter()
        .map(|key| key.trim().to_lowercase())
        .filter(|key| !key.is_empty())
        .collect()
}

fn jaccard(left: &BTreeSet<String>, right: &BTreeSet<String>) -> f32 {
    if left.is_empty() && right.is_empty() {
        return 1.0;
    }
    let intersection = left
        .intersection(right)
        .fold(0.0_f32, |count, _| count + 1.0);
    let union = left.union(right).fold(0.0_f32, |count, _| count + 1.0);
    intersection / union
}

fn equality_score<T: PartialEq>(left: &T, right: &T) -> f32 {
    if left == right { 1.0 } else { 0.0 }
}

fn time_bucket(timestamp_seconds: i64) -> i64 {
    timestamp_seconds.div_euclid(TIME_BUCKET_SECONDS)
}
