//! 保守去重 Handler。

use async_trait::async_trait;

use crate::replay::{batch_outcome, stable_protocol_id};
use crate::{BatchContext, BatchOutcome, ConsolidationTask, EdgeWrite, JobError};

/// 只从同内容或共同激活证据创建非因果关系。
#[derive(Clone, Copy, Debug, Default)]
pub struct Deduplication;

#[async_trait]
impl ConsolidationTask for Deduplication {
    async fn process_batch(
        &self,
        context: &mut BatchContext<'_>,
    ) -> Result<BatchOutcome, JobError> {
        let cursor = context.checkpoint().cursor;
        let batch_size = context.batch_size();
        let events = context.read_authority_events().await?;
        for event in &events {
            let Some(current) = context.point_for_fragment(&event.fragment_id).await? else {
                continue;
            };
            let Some((candidate, kind)) = context.dedup_candidate(event).await? else {
                continue;
            };
            if candidate == current {
                continue;
            }
            context
                .write_edge(&EdgeWrite {
                    id: stable_protocol_id("dedup-edge", &[&candidate, &current, &kind]),
                    source_point_id: candidate,
                    target_point_id: current,
                    kind,
                    weight: 0.5,
                    confidence: event.confidence,
                    source_fragment_id: event.fragment_id.clone(),
                    valid_from_us: event.recorded_at_us,
                    created_at_us: event.recorded_at_us,
                })
                .await?;
        }
        Ok(batch_outcome(cursor, batch_size, &events))
    }
}
