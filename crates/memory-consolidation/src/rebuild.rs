//! 只写派生结构的可恢复 Rebuild Handler。

use async_trait::async_trait;
use serde_json::Value;

use crate::replay::{batch_outcome, replay_event};
use crate::{
    BatchContext, BatchOutcome, ConsolidationTable, ConsolidationTask, DerivedMutation, JobError,
};

/// 按 Checkpoint 增量重建 Point 与 FTS，并保持权威三表不变。
#[derive(Clone, Copy, Debug, Default)]
pub struct Rebuild;

#[async_trait]
impl ConsolidationTask for Rebuild {
    async fn process_batch(
        &self,
        context: &mut BatchContext<'_>,
    ) -> Result<BatchOutcome, JobError> {
        let cursor = context.checkpoint().cursor;
        let batch_size = context.batch_size();
        if cursor == 0 {
            for table in [
                ConsolidationTable::Surfaces,
                ConsolidationTable::Edges,
                ConsolidationTable::Points,
                ConsolidationTable::Artifacts,
                ConsolidationTable::ColdTier,
                ConsolidationTable::FragmentFts,
            ] {
                context
                    .apply(DerivedMutation::delete_scope(
                        table,
                        context.scope().clone(),
                    ))
                    .await?;
            }
        }
        let events = context.read_authority_events().await?;
        for event in &events {
            replay_event(context, event).await?;
            if let Some(body) = inline_text(&event.payload) {
                context.write_fts(event, body).await?;
            }
        }
        Ok(batch_outcome(cursor, batch_size, &events))
    }
}

fn inline_text(payload: &Value) -> Option<&str> {
    payload.get("content")?.get("text")?.as_str()
}
