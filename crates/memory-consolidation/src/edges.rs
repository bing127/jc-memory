//! Edge 强化与弱化 Handler。

use async_trait::async_trait;
use serde_json::json;

use crate::replay::{artifact, batch_outcome, metadata_string};
use crate::{BatchContext, BatchOutcome, ConsolidationTask, JobError};

/// 使用成功 Evidence 小步增强既有 Edge。
#[derive(Clone, Copy, Debug, Default)]
pub struct EdgeReinforcement;

/// 使用失败 Evidence 小步弱化既有 Edge。
#[derive(Clone, Copy, Debug, Default)]
pub struct EdgeWeakening;

#[async_trait]
impl ConsolidationTask for EdgeReinforcement {
    async fn process_batch(
        &self,
        context: &mut BatchContext<'_>,
    ) -> Result<BatchOutcome, JobError> {
        adjust_batch(context, true, 0.05, "edge_reinforcement").await
    }
}

#[async_trait]
impl ConsolidationTask for EdgeWeakening {
    async fn process_batch(
        &self,
        context: &mut BatchContext<'_>,
    ) -> Result<BatchOutcome, JobError> {
        adjust_batch(context, false, -0.05, "edge_weakening").await
    }
}

async fn adjust_batch(
    context: &mut BatchContext<'_>,
    success: bool,
    delta: f64,
    artifact_kind: &str,
) -> Result<BatchOutcome, JobError> {
    let cursor = context.checkpoint().cursor;
    let batch_size = context.batch_size();
    let events = context.read_authority_events().await?;
    for event in &events {
        let matches_outcome = if success {
            matches!(event.fragment_kind.as_str(), "success" | "outcome")
        } else {
            event.fragment_kind == "failure"
        };
        let Some(edge_id) = metadata_string(event, "edge_id") else {
            continue;
        };
        if !matches_outcome {
            continue;
        }
        let edge_id = edge_id.to_owned();
        if !context.adjust_edge_weight(&edge_id, delta).await? {
            continue;
        }
        context
            .write_artifact(&artifact(
                artifact_kind,
                &edge_id,
                event,
                json!({"edge_id": edge_id, "delta": delta}),
            ))
            .await?;
    }
    Ok(batch_outcome(cursor, batch_size, &events))
}
