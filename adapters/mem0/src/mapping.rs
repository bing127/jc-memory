//! Mem0 记录到权威 Fragment 的映射。

use std::str::FromStr as _;

use jc_memory_protocol::{
    CognitiveFragment, FragmentContent, FragmentId, FragmentKind, FragmentSource, MemoryScope,
    Metadata, Perspective, SourceType, ValidityStatus,
};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

use crate::{Mem0Config, Mem0Error, Mem0Record};

pub(crate) struct MappedRecord {
    pub(crate) fragment: CognitiveFragment,
    pub(crate) content_sha256: String,
}

pub(crate) fn map_record(
    record: &Mem0Record,
    config: &Mem0Config,
    scope: &MemoryScope,
) -> Result<MappedRecord, Mem0Error> {
    if record.id.trim().is_empty() || record.memory.trim().is_empty() {
        return Err(Mem0Error::InvalidRecord("id 和 memory 不能为空".to_owned()));
    }
    if record
        .legacy_confidence
        .is_some_and(|confidence| !confidence.is_finite())
    {
        return Err(Mem0Error::InvalidRecord(
            "legacy_confidence 必须是有限数值".to_owned(),
        ));
    }
    let content_sha256 = hex::encode(Sha256::digest(record.memory.as_bytes()));
    let source_id = format!(
        "mem0/{}/memory/{}/{}",
        config.source_version,
        record.id,
        record.updated_at.to_rfc3339()
    );
    let fragment_id = deterministic_fragment_id(&format!(
        "{source_id}\n{}\n{}",
        record.created_at.to_rfc3339(),
        record.memory
    ))?;
    let mut metadata = Metadata::new();
    metadata.insert("mem0_id".to_owned(), Value::String(record.id.clone()));
    metadata.insert(
        "mem0_source_version".to_owned(),
        Value::String(config.source_version.clone()),
    );
    metadata.insert("created_at".to_owned(), json!(record.created_at));
    metadata.insert("updated_at".to_owned(), json!(record.updated_at));
    metadata.insert(
        "legacy_confidence".to_owned(),
        record
            .legacy_confidence
            .map_or(Value::Null, |value| json!(value)),
    );
    metadata.insert("legacy_metadata".to_owned(), record.metadata.clone());
    metadata.insert(
        "legacy_user_id".to_owned(),
        record.user_id.clone().map_or(Value::Null, Value::String),
    );
    metadata.insert(
        "legacy_agent_id".to_owned(),
        record.agent_id.clone().map_or(Value::Null, Value::String),
    );
    metadata.insert(
        "content_sha256".to_owned(),
        Value::String(content_sha256.clone()),
    );
    Ok(MappedRecord {
        content_sha256,
        fragment: CognitiveFragment {
            id: fragment_id,
            scope: scope.clone(),
            session_id: None,
            trace_id: None,
            kind: FragmentKind::Fact,
            content: FragmentContent::Text {
                text: record.memory.clone(),
            },
            source: FragmentSource {
                source_type: SourceType::Import,
                source_id,
                actor_id: record.user_id.clone(),
                observer_id: Some("jc-memory-adapter-mem0".to_owned()),
                tool_id: None,
                uri: None,
            },
            perspective: Perspective::ThirdPerson,
            occurred_at: record.updated_at,
            observed_at: record.updated_at,
            confidence: 0.0,
            importance: config.default_importance,
            validity: ValidityStatus::Unverified,
            security_label: config.security_label,
            evidence: Vec::new(),
            metadata,
        },
    })
}

fn deterministic_fragment_id(seed: &str) -> Result<FragmentId, Mem0Error> {
    let digest = Sha256::digest(seed.as_bytes());
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x80;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let value = format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0],
        bytes[1],
        bytes[2],
        bytes[3],
        bytes[4],
        bytes[5],
        bytes[6],
        bytes[7],
        bytes[8],
        bytes[9],
        bytes[10],
        bytes[11],
        bytes[12],
        bytes[13],
        bytes[14],
        bytes[15]
    );
    Ok(FragmentId::from_str(&value)?)
}
