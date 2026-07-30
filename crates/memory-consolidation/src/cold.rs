//! 冷层移动 Handler。

use async_trait::async_trait;
use serde_json::json;

use crate::replay::{batch_outcome, evidence};
use crate::{BatchContext, BatchOutcome, ConsolidationTask, JobError};

/// 仅更新可重建层级索引，不移动或改写权威 Fragment。
#[derive(Clone, Copy, Debug, Default)]
pub struct ColdTierMovement;

#[async_trait]
impl ConsolidationTask for ColdTierMovement {
    async fn process_batch(
        &self,
        context: &mut BatchContext<'_>,
    ) -> Result<BatchOutcome, JobError> {
        let cursor = context.checkpoint().cursor;
        let batch_size = context.batch_size();
        let events = context.read_authority_events().await?;
        for event in &events {
            if !matches!(event.validity.as_str(), "superseded" | "invalidated") {
                continue;
            }
            context
                .write_cold_tier(event, &json!([evidence(event)]))
                .await?;
        }
        Ok(batch_outcome(cursor, batch_size, &events))
    }
}
