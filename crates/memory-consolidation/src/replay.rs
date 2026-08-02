//! 权威 Event 的增量重放 Handler。

use async_trait::async_trait;
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

use crate::{
    ArtifactWrite, AuthorityEvent, BatchContext, BatchOutcome, ConsolidationTask, JobError,
    PointWrite,
};

/// 从权威 Event 重建基础派生 Point。
#[derive(Clone, Copy, Debug, Default)]
pub struct Replay;

#[async_trait]
impl ConsolidationTask for Replay {
    async fn process_batch(
        &self,
        context: &mut BatchContext<'_>,
    ) -> Result<BatchOutcome, JobError> {
        let batch_size = context.batch_size();
        let cursor = context.checkpoint().cursor;
        let events = context.read_authority_events().await?;
        for event in &events {
            replay_event(context, event).await?;
        }
        Ok(batch_outcome(cursor, batch_size, &events))
    }
}

pub(crate) async fn replay_event(
    context: &mut BatchContext<'_>,
    event: &AuthorityEvent,
) -> Result<(), JobError> {
    let Some(kind) = point_kind(&event.fragment_kind) else {
        return Ok(());
    };
    context
        .write_point(&PointWrite {
            id: event.fragment_id.clone(),
            kind: kind.to_owned(),
            source_fragment_id: event.fragment_id.clone(),
            valid_from_us: event.occurred_at_us,
            confidence: event.confidence,
            created_at_us: event.recorded_at_us,
        })
        .await
}

pub(crate) fn batch_outcome(
    cursor: u64,
    batch_size: u32,
    events: &[AuthorityEvent],
) -> BatchOutcome {
    BatchOutcome {
        next_cursor: events
            .last()
            .map_or(cursor, |event| event.sequence.saturating_add(1)),
        complete: events.len() < usize::try_from(batch_size).unwrap_or(usize::MAX),
    }
}

pub(crate) fn point_kind(fragment_kind: &str) -> Option<&'static str> {
    match fragment_kind {
        "fact" => Some("fact"),
        "decision" => Some("decision"),
        "constraint" => Some("constraint"),
        "state_transition" => Some("state_change"),
        "action" => Some("action"),
        "outcome" | "success" => Some("result"),
        "failure" => Some("failure"),
        "preference" => Some("preference"),
        "correction" => Some("correction"),
        _ => None,
    }
}

pub(crate) fn stable_id(kind: &str, parts: &[&str]) -> String {
    hex::encode(stable_digest(kind, parts))
}

pub(crate) fn stable_protocol_id(kind: &str, parts: &[&str]) -> String {
    let digest = stable_digest(kind, parts);
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    uuid::Uuid::from_bytes(bytes).to_string()
}

fn stable_digest(kind: &str, parts: &[&str]) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(kind.as_bytes());
    for part in parts {
        digest.update([0]);
        digest.update(part.as_bytes());
    }
    digest.finalize().into()
}

pub(crate) fn evidence(event: &AuthorityEvent) -> Value {
    json!({
        "scope": event.payload.get("scope").cloned().unwrap_or(Value::Null),
        "event_id": event.event_id,
        "fragment_id": event.fragment_id,
        "source_id": event.source_id,
        "sequence": event.sequence,
    })
}

pub(crate) fn artifact(
    kind: &str,
    subject_key: &str,
    event: &AuthorityEvent,
    payload: Value,
) -> ArtifactWrite {
    ArtifactWrite {
        id: stable_id(kind, &[subject_key, &event.event_id]),
        kind: kind.to_owned(),
        subject_key: subject_key.to_owned(),
        source_cursor: event.sequence,
        payload,
        evidence: json!([evidence(event)]),
        created_at_us: event.recorded_at_us,
    }
}

pub(crate) fn metadata_string<'a>(event: &'a AuthorityEvent, key: &str) -> Option<&'a str> {
    event
        .payload
        .get("metadata")
        .and_then(|metadata| metadata.get(key))
        .and_then(Value::as_str)
}
