use chrono::Utc;
use jc_memory_protocol::{
    CognitiveFragment, FragmentContent, FragmentId, FragmentKind, FragmentSource, MemoryScope,
    NamespaceId, Perspective, SecurityLabel, SourceType, ValidityStatus,
};

pub fn observation(text: &str) -> CognitiveFragment {
    observation_in_scope(MemoryScope::new(NamespaceId::new()), text)
}

pub fn observation_in_scope(scope: MemoryScope, text: &str) -> CognitiveFragment {
    let now = Utc::now();
    CognitiveFragment {
        id: FragmentId::new(),
        scope,
        session_id: None,
        trace_id: None,
        kind: FragmentKind::Observation,
        content: FragmentContent::Text {
            text: text.to_owned(),
        },
        source: FragmentSource {
            source_type: SourceType::System,
            source_id: "embedded-test".to_owned(),
            actor_id: None,
            observer_id: None,
            tool_id: None,
            uri: None,
        },
        perspective: Perspective::System,
        occurred_at: now,
        observed_at: now,
        confidence: 1.0,
        importance: 0.5,
        validity: ValidityStatus::Unverified,
        security_label: SecurityLabel::Internal,
        evidence: Vec::new(),
        metadata: serde_json::Map::new(),
    }
}
