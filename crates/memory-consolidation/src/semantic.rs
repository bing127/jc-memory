//! 保守语义提升、经验学习与冲突检测。

use std::{
    collections::{BTreeMap, BTreeSet},
    str::FromStr as _,
};

use async_trait::async_trait;
use jc_memory_protocol::{EventId, FragmentId, MemoryScope};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;

use crate::replay::{artifact, batch_outcome, evidence, stable_id};
use crate::{ArtifactWrite, BatchContext, BatchOutcome, ConsolidationTask, JobError, PointWrite};

/// 一次改动的离散规模。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeMagnitude {
    /// 局部、小范围改动。
    Small,
    /// 跨模块或高影响改动。
    Large,
}

/// 一个保留独立 Event、Fragment 和来源的经验样本。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ExperienceSample {
    /// 样本所属的精确 Scope。
    pub scope: MemoryScope,
    /// 权威 Event 标识。
    pub event_id: EventId,
    /// 权威 Fragment 标识。
    pub fragment_id: FragmentId,
    /// 独立来源标识。
    pub source_id: String,
    /// 改动规模。
    pub change_magnitude: ChangeMagnitude,
    /// 观察到的实际结果。
    pub succeeded: bool,
}

/// 二项成功率的 95% Wilson 置信区间。
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub struct ConfidenceInterval {
    /// 区间下界。
    pub lower: f64,
    /// 区间上界。
    pub upper: f64,
}

/// 带样本分母、区间和全部权威 Evidence 的保守经验。
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct LearnedExperience {
    /// 参与统计的独立样本数。
    pub sample_count: u32,
    /// 文案所描述成功率的 95% Wilson 区间。
    pub confidence_interval: ConfidenceInterval,
    /// 带样本限定、非绝对化的经验文案。
    pub statement: String,
    /// 实际计入的权威 Evidence/provenance。
    pub evidence: Vec<ExperienceSample>,
}

/// 经验学习输入违反权威性或独立性约束。
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum ExperienceLearningError {
    /// Procedure key 为空。
    #[error("procedure key cannot be empty")]
    EmptyProcedureKey,
    /// 样本属于其他 Scope。
    #[error("event {0} does not belong to the requested scope")]
    ScopeMismatch(EventId),
    /// Event 或来源独立样本不足。
    #[error("experience learning requires at least two independent events and sources")]
    InsufficientIndependentEvidence,
    /// 样本数超过可表示范围。
    #[error("experience sample count overflow")]
    SampleCountOverflow,
}

/// 达到独立 Event/来源门槛后才写入稳定语义 Point。
#[derive(Clone, Copy, Debug, Default)]
pub struct SemanticPromotion;

/// 记录相互矛盾的权威 Evidence，而不改写原始记录。
#[derive(Clone, Copy, Debug, Default)]
pub struct ConflictDetection;

impl SemanticPromotion {
    /// 从独立权威样本计算带置信区间的非绝对经验。
    ///
    /// # Errors
    ///
    /// Scope 不一致、Procedure key 为空或独立证据不足时返回错误。
    pub fn learn_procedure(
        scope: &MemoryScope,
        procedure_key: &str,
        samples: &[ExperienceSample],
    ) -> Result<LearnedExperience, ExperienceLearningError> {
        if procedure_key.trim().is_empty() {
            return Err(ExperienceLearningError::EmptyProcedureKey);
        }
        if let Some(sample) = samples.iter().find(|sample| &sample.scope != scope) {
            return Err(ExperienceLearningError::ScopeMismatch(sample.event_id));
        }
        let mut events = BTreeSet::new();
        let mut sources = BTreeSet::new();
        let evidence: Vec<_> = samples
            .iter()
            .filter(|sample| events.insert(sample.event_id))
            .cloned()
            .collect();
        for sample in &evidence {
            sources.insert(sample.source_id.as_str());
        }
        if evidence.len() < 2 || sources.len() < 2 {
            return Err(ExperienceLearningError::InsufficientIndependentEvidence);
        }
        let sample_count = u32::try_from(evidence.len())
            .map_err(|_| ExperienceLearningError::SampleCountOverflow)?;
        let small_success = evidence
            .iter()
            .filter(|sample| sample.change_magnitude == ChangeMagnitude::Small && sample.succeeded)
            .count();
        let small_count = evidence
            .iter()
            .filter(|sample| sample.change_magnitude == ChangeMagnitude::Small)
            .count();
        let large_success = evidence
            .iter()
            .filter(|sample| sample.change_magnitude == ChangeMagnitude::Large && sample.succeeded)
            .count();
        let large_count = evidence
            .iter()
            .filter(|sample| sample.change_magnitude == ChangeMagnitude::Large)
            .count();
        let small_sources: BTreeSet<_> = evidence
            .iter()
            .filter(|sample| sample.change_magnitude == ChangeMagnitude::Small)
            .map(|sample| sample.source_id.as_str())
            .collect();
        let large_sources: BTreeSet<_> = evidence
            .iter()
            .filter(|sample| sample.change_magnitude == ChangeMagnitude::Large)
            .map(|sample| sample.source_id.as_str())
            .collect();
        if small_count < 2 || large_count < 2 || small_sources.len() < 2 || large_sources.len() < 2
        {
            let success_count =
                u32::try_from(evidence.iter().filter(|sample| sample.succeeded).count())
                    .map_err(|_| ExperienceLearningError::SampleCountOverflow)?;
            return Ok(LearnedExperience {
                sample_count,
                confidence_interval: wilson_interval(success_count, sample_count),
                statement: format!(
                    "当前 {sample_count} 个样本中，小改成功 {small_success}/{small_count}，大改成功 {large_success}/{large_count}；至少一组少于 2 个独立 Event 或来源，不能形成规模方向性结论。区间对应全部样本成功率。"
                ),
                evidence,
            });
        }
        if large_success * small_count == small_success * large_count {
            let success_count =
                u32::try_from(evidence.iter().filter(|sample| sample.succeeded).count())
                    .map_err(|_| ExperienceLearningError::SampleCountOverflow)?;
            return Ok(LearnedExperience {
                sample_count,
                confidence_interval: wilson_interval(success_count, sample_count),
                statement: format!(
                    "当前 {sample_count} 个样本中，小改成功 {small_success}/{small_count}，大改成功 {large_success}/{large_count}；两组观察成功率相同，不能形成规模方向性结论。区间对应全部样本成功率。"
                ),
                evidence,
            });
        }
        let large_is_more_likely = large_success * small_count > small_success * large_count;
        let (preferred_magnitude, preferred_success, preferred_count) = if large_is_more_likely {
            ("大改", large_success, large_count)
        } else {
            ("小改", small_success, small_count)
        };
        let preferred_success = u32::try_from(preferred_success)
            .map_err(|_| ExperienceLearningError::SampleCountOverflow)?;
        let preferred_count = u32::try_from(preferred_count)
            .map_err(|_| ExperienceLearningError::SampleCountOverflow)?;
        Ok(LearnedExperience {
            sample_count,
            confidence_interval: wilson_interval(preferred_success, preferred_count),
            statement: format!(
                "当前 {sample_count} 个样本中，小改成功 {small_success}/{small_count}，大改成功 {large_success}/{large_count}；在这些 Evidence 下，{preferred_magnitude}更可能成功，但仍需结合新的证据判断。"
            ),
            evidence,
        })
    }
}

#[async_trait]
impl ConsolidationTask for SemanticPromotion {
    async fn process_batch(
        &self,
        context: &mut BatchContext<'_>,
    ) -> Result<BatchOutcome, JobError> {
        let cursor = context.checkpoint().cursor;
        let batch_size = context.batch_size();
        let events = context.read_authority_events().await?;
        persist_procedure_experience(context, &events).await?;
        for event in &events {
            let Some(point_kind) = semantic_point_kind(&event.fragment_kind) else {
                continue;
            };
            let supports = context.authority_events_by_content(event).await?;
            if !independent_verified_support(&supports) {
                continue;
            }
            let evidence_json: Vec<Value> = supports.iter().map(evidence).collect();
            let support_count = u32::try_from(supports.len())
                .map_err(|_| JobError::Task("semantic support count overflow".into()))?;
            let confidence = supports
                .iter()
                .map(|support| support.confidence)
                .sum::<f64>()
                / f64::from(support_count);
            let scope_identity = event
                .payload
                .get("scope")
                .ok_or_else(|| JobError::Corrupt("fragment scope is missing".into()))?
                .to_string();
            context
                .write_point(&PointWrite {
                    id: event.fragment_id.clone(),
                    kind: point_kind.into(),
                    source_fragment_id: event.fragment_id.clone(),
                    valid_from_us: event.occurred_at_us,
                    confidence,
                    created_at_us: event.recorded_at_us,
                })
                .await?;
            context
                .write_artifact(&crate::ArtifactWrite {
                    id: stable_id(
                        "semantic-promotion",
                        &[&scope_identity, point_kind, &event.content_id],
                    ),
                    kind: "semantic_promotion".into(),
                    subject_key: event.content_id.clone(),
                    source_cursor: event.sequence,
                    payload: json!({
                        "point_id": event.fragment_id,
                        "kind": point_kind,
                        "content": event.payload.get("content").cloned().unwrap_or(Value::Null),
                        "confidence": confidence,
                        "validity": "verified",
                        "stable": true,
                    }),
                    evidence: Value::Array(evidence_json),
                    created_at_us: event.recorded_at_us,
                })
                .await?;
        }
        Ok(batch_outcome(cursor, batch_size, &events))
    }
}

async fn persist_procedure_experience(
    context: &mut BatchContext<'_>,
    events: &[crate::AuthorityEvent],
) -> Result<(), JobError> {
    let scope = context.scope().clone();
    let mut grouped = BTreeMap::<String, Vec<ExperienceSample>>::new();
    for event in events {
        if let Some((procedure_key, sample)) = procedure_sample(&scope, event)? {
            grouped.entry(procedure_key).or_default().push(sample);
        }
    }
    let scope_identity = serde_json::to_string(&scope)?;
    for (procedure_key, batch_samples) in grouped {
        let mut samples = match context
            .artifact_evidence("procedure_learning_state", &procedure_key)
            .await?
        {
            Some(evidence) => serde_json::from_value::<Vec<ExperienceSample>>(evidence)?,
            None => Vec::new(),
        };
        for sample in batch_samples {
            if !samples
                .iter()
                .any(|existing| existing.event_id == sample.event_id)
            {
                samples.push(sample);
            }
        }
        let source_cursor = events
            .last()
            .map_or(context.checkpoint().cursor, |event| event.sequence);
        let state_evidence = serde_json::to_value(&samples)?;
        context
            .write_artifact(&ArtifactWrite {
                id: stable_id(
                    "procedure-learning-state",
                    &[&scope_identity, &procedure_key],
                ),
                kind: "procedure_learning_state".into(),
                subject_key: procedure_key.clone(),
                source_cursor,
                payload: json!({"sample_count": samples.len()}),
                evidence: state_evidence,
                created_at_us: events.last().map_or(0, |event| event.recorded_at_us),
            })
            .await?;
        let learning = match SemanticPromotion::learn_procedure(&scope, &procedure_key, &samples) {
            Ok(learning) => learning,
            Err(ExperienceLearningError::InsufficientIndependentEvidence) => continue,
            Err(error) => return Err(JobError::Task(error.to_string())),
        };
        let success_count = learning
            .evidence
            .iter()
            .filter(|sample| sample.succeeded)
            .count();
        let success_count = u32::try_from(success_count)
            .map_err(|_| JobError::Task("procedure success count overflow".into()))?;
        let failure_count = learning.sample_count.saturating_sub(success_count);
        let success_rate = f64::from(success_count) / f64::from(learning.sample_count);
        context
            .write_artifact(&ArtifactWrite {
                id: stable_id("procedure-experience", &[&scope_identity, &procedure_key]),
                kind: "procedure_experience".into(),
                subject_key: procedure_key,
                source_cursor,
                payload: json!({
                    "sample_count": learning.sample_count,
                    "success_count": success_count,
                    "failure_count": failure_count,
                    "success_rate": success_rate,
                    "confidence_interval": learning.confidence_interval,
                    "statement": learning.statement,
                }),
                evidence: serde_json::to_value(&learning.evidence)?,
                created_at_us: events.last().map_or(0, |event| event.recorded_at_us),
            })
            .await?;
    }
    Ok(())
}

fn procedure_sample(
    scope: &MemoryScope,
    event: &crate::AuthorityEvent,
) -> Result<Option<(String, ExperienceSample)>, JobError> {
    let Some(metadata) = event.payload.get("metadata") else {
        return Ok(None);
    };
    let Some(procedure_key) = metadata.get("procedure_key").and_then(Value::as_str) else {
        return Ok(None);
    };
    let change_magnitude = match metadata.get("change_magnitude").and_then(Value::as_str) {
        Some("small") => ChangeMagnitude::Small,
        Some("large") => ChangeMagnitude::Large,
        Some(_) => return Err(JobError::Task("invalid change_magnitude metadata".into())),
        None => return Ok(None),
    };
    let succeeded = match event.fragment_kind.as_str() {
        "success" => true,
        "failure" => false,
        "outcome" => {
            let Some(succeeded) = metadata.get("succeeded").and_then(Value::as_bool) else {
                return Ok(None);
            };
            succeeded
        }
        _ => return Ok(None),
    };
    let event_id = EventId::from_str(&event.event_id)
        .map_err(|error| JobError::Corrupt(format!("invalid procedure event ID: {error}")))?;
    let fragment_id = FragmentId::from_str(&event.fragment_id)
        .map_err(|error| JobError::Corrupt(format!("invalid procedure fragment ID: {error}")))?;
    Ok(Some((
        procedure_key.to_owned(),
        ExperienceSample {
            scope: scope.clone(),
            event_id,
            fragment_id,
            source_id: event.source_id.clone(),
            change_magnitude,
            succeeded,
        },
    )))
}

#[async_trait]
impl ConsolidationTask for ConflictDetection {
    async fn process_batch(
        &self,
        context: &mut BatchContext<'_>,
    ) -> Result<BatchOutcome, JobError> {
        let cursor = context.checkpoint().cursor;
        let batch_size = context.batch_size();
        let events = context.read_authority_events().await?;
        for event in &events {
            let Some(target) = event
                .payload
                .get("metadata")
                .and_then(|value| value.get("contradicts_fragment_id"))
                .and_then(Value::as_str)
            else {
                continue;
            };
            let target = target.to_owned();
            context
                .write_artifact(&artifact(
                    "conflict",
                    &target,
                    event,
                    json!({"referenced_fragment_id": target}),
                ))
                .await?;
        }
        Ok(batch_outcome(cursor, batch_size, &events))
    }
}

fn semantic_point_kind(fragment_kind: &str) -> Option<&'static str> {
    match fragment_kind {
        "fact" => Some("fact"),
        "preference" => Some("preference"),
        "constraint" => Some("constraint"),
        _ => None,
    }
}

fn independent_verified_support(events: &[crate::AuthorityEvent]) -> bool {
    if events.iter().any(|event| {
        matches!(
            event.fragment_kind.as_str(),
            "prediction" | "inference" | "plan"
        ) || event.validity != "verified"
    }) {
        return false;
    }
    let event_ids: BTreeSet<_> = events.iter().map(|event| event.event_id.as_str()).collect();
    let source_ids: BTreeSet<_> = events
        .iter()
        .map(|event| event.source_id.as_str())
        .collect();
    event_ids.len() >= 2 && source_ids.len() >= 2
}

fn wilson_interval(successes: u32, samples: u32) -> ConfidenceInterval {
    let n = f64::from(samples);
    let rate = f64::from(successes) / n;
    let z = 1.96_f64;
    let z_squared = z * z;
    let denominator = 1.0 + z_squared / n;
    let center = (rate + z_squared / (2.0 * n)) / denominator;
    let margin = z * ((rate * (1.0 - rate) / n + z_squared / (4.0 * n * n)).sqrt()) / denominator;
    ConfidenceInterval {
        lower: (center - margin).clamp(0.0, 1.0),
        upper: (center + margin).clamp(0.0, 1.0),
    }
}
