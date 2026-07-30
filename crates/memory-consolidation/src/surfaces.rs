//! Surface 发现、合并与分裂 Handler。

use async_trait::async_trait;
use serde_json::json;

use crate::replay::{batch_outcome, evidence, stable_id};
use crate::{BatchContext, BatchOutcome, ConsolidationTask, JobError, SurfaceVersionWrite};

/// 从单个证据支持的 Point 发现初始动态 Surface。
#[derive(Clone, Copy, Debug, Default)]
pub struct SurfaceDiscovery;

/// 将来源追加为 merged 终态版本，并创建成员并集的新后继 Surface。
#[derive(Clone, Copy, Debug, Default)]
pub struct SurfaceMerge;

/// 将来源追加为 split 终态版本，并创建两个成员互斥的新后继 Surface。
#[derive(Clone, Copy, Debug, Default)]
pub struct SurfaceSplit;

#[async_trait]
impl ConsolidationTask for SurfaceDiscovery {
    async fn process_batch(
        &self,
        context: &mut BatchContext<'_>,
    ) -> Result<BatchOutcome, JobError> {
        let cursor = context.checkpoint().cursor;
        let batch_size = context.batch_size();
        let events = context.read_authority_events().await?;
        for event in &events {
            let Some(point_id) = context.point_for_fragment(&event.fragment_id).await? else {
                continue;
            };
            context
                .write_surface_version(&SurfaceVersionWrite {
                    id: stable_id("surface", &[&event.content_hash]),
                    kind: "dynamic".into(),
                    status: "active".into(),
                    semantic_center: json!({"content_hash": event.content_hash}),
                    activation_threshold: 0.5,
                    confidence: event.confidence,
                    version: 1,
                    valid_from_us: event.recorded_at_us,
                    evidence: json!([evidence(event)]),
                    members: vec![(point_id, 1.0)],
                })
                .await?;
        }
        Ok(batch_outcome(cursor, batch_size, &events))
    }
}

#[async_trait]
impl ConsolidationTask for SurfaceMerge {
    async fn process_batch(
        &self,
        context: &mut BatchContext<'_>,
    ) -> Result<BatchOutcome, JobError> {
        let surfaces: Vec<_> = context
            .latest_surfaces()
            .await?
            .into_iter()
            .filter(|surface| matches!(surface.status.as_str(), "active" | "frozen"))
            .collect();
        let Some(first) = surfaces.first() else {
            return Ok(done(context));
        };
        if surfaces.len() < 2 {
            return Ok(done(context));
        }
        let changed_at = surfaces
            .iter()
            .map(|surface| surface.valid_from_us)
            .max()
            .unwrap_or_default()
            .saturating_add(1);
        let mut members = Vec::<(String, f64)>::new();
        for source in &surfaces {
            for candidate in &source.members {
                if let Some(existing) = members.iter_mut().find(|member| member.0 == candidate.0) {
                    existing.1 = existing.1.max(candidate.1);
                } else {
                    members.push(candidate.clone());
                }
            }
            context
                .write_surface_version(&SurfaceVersionWrite {
                    id: source.id.clone(),
                    kind: source.kind.clone(),
                    status: "merged".into(),
                    semantic_center: source.semantic_center.clone(),
                    activation_threshold: source.activation_threshold,
                    confidence: source.confidence,
                    version: source.version.saturating_add(1),
                    valid_from_us: changed_at,
                    evidence: json!({
                        "operation": "merge_source",
                        "source": source.evidence,
                    }),
                    members: source.members.clone(),
                })
                .await?;
        }
        members.sort_by(|left, right| left.0.cmp(&right.0));
        let identity = surfaces
            .iter()
            .map(|surface| format!("{}:{}", surface.id, surface.version))
            .collect::<Vec<_>>()
            .join("|");
        context
            .write_surface_version(&SurfaceVersionWrite {
                id: stable_id("surface-merge-successor", &[&identity]),
                kind: first.kind.clone(),
                status: "active".into(),
                semantic_center: json!({
                    "merged_surface_ids": surfaces.iter().map(|surface| &surface.id).collect::<Vec<_>>(),
                }),
                activation_threshold: surfaces
                    .iter()
                    .map(|surface| surface.activation_threshold)
                    .fold(1.0, f64::min),
                confidence: surfaces
                    .iter()
                    .map(|surface| surface.confidence)
                    .fold(1.0, f64::min),
                version: 1,
                valid_from_us: changed_at,
                evidence: json!({
                    "operation": "merge_successor",
                    "sources": surfaces.iter().map(|surface| &surface.evidence).collect::<Vec<_>>(),
                }),
                members,
            })
            .await?;
        Ok(done(context))
    }
}

#[async_trait]
impl ConsolidationTask for SurfaceSplit {
    async fn process_batch(
        &self,
        context: &mut BatchContext<'_>,
    ) -> Result<BatchOutcome, JobError> {
        let surfaces = context.latest_surfaces().await?;
        let Some(surface) = surfaces.iter().find(|surface| {
            matches!(surface.status.as_str(), "active" | "frozen") && surface.members.len() >= 2
        }) else {
            return Ok(done(context));
        };
        let changed_at = surface.valid_from_us.saturating_add(1);
        context
            .write_surface_version(&SurfaceVersionWrite {
                id: surface.id.clone(),
                kind: surface.kind.clone(),
                status: "split".into(),
                semantic_center: surface.semantic_center.clone(),
                activation_threshold: surface.activation_threshold,
                confidence: surface.confidence,
                version: surface.version.saturating_add(1),
                valid_from_us: changed_at,
                evidence: json!({"operation": "split_source", "source": surface.evidence}),
                members: surface.members.clone(),
            })
            .await?;
        let mut partitions = [Vec::new(), Vec::new()];
        for (index, member) in surface.members.iter().enumerate() {
            partitions[index % 2].push(member.clone());
        }
        for (index, members) in partitions.into_iter().enumerate() {
            let partition = index.to_string();
            context
                .write_surface_version(&SurfaceVersionWrite {
                    id: stable_id(
                        "surface-split-successor",
                        &[&surface.id, &surface.version.to_string(), &partition],
                    ),
                    kind: surface.kind.clone(),
                    status: "active".into(),
                    semantic_center: json!({
                        "split_from_surface_id": surface.id,
                        "split_from_version": surface.version,
                        "partition": index,
                    }),
                    activation_threshold: surface.activation_threshold,
                    confidence: surface.confidence,
                    version: 1,
                    valid_from_us: changed_at,
                    evidence: json!({
                        "operation": "split_successor",
                        "source": surface.evidence,
                        "partition": index,
                    }),
                    members,
                })
                .await?;
        }
        Ok(done(context))
    }
}

fn done(context: &BatchContext<'_>) -> BatchOutcome {
    BatchOutcome {
        next_cursor: context.checkpoint().cursor,
        complete: true,
    }
}
