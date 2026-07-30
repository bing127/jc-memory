//! 陈旧状态检测 Handler。

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::replay::{artifact, batch_outcome};
use crate::{BatchContext, BatchOutcome, ConsolidationTask, JobError};

/// 从显式状态替代 Evidence 记录陈旧状态，不覆盖状态历史。
#[derive(Clone, Copy, Debug, Default)]
pub struct StaleStateDetection;

#[async_trait]
impl ConsolidationTask for StaleStateDetection {
    async fn process_batch(
        &self,
        context: &mut BatchContext<'_>,
    ) -> Result<BatchOutcome, JobError> {
        let cursor = context.checkpoint().cursor;
        let batch_size = context.batch_size();
        let events = context.read_authority_events().await?;
        for event in &events {
            let Some(state_key) = event
                .payload
                .get("metadata")
                .and_then(|metadata| metadata.get("supersedes_state_key"))
                .and_then(Value::as_str)
            else {
                continue;
            };
            let state_key = state_key.to_owned();
            context
                .write_artifact(&artifact(
                    "stale_state",
                    &state_key,
                    event,
                    json!({"state_key": state_key, "detected": true}),
                ))
                .await?;
        }
        Ok(batch_outcome(cursor, batch_size, &events))
    }
}
