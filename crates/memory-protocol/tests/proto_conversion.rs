//! Rust 领域 Envelope 与强类型 Protobuf 的无损转换契约。

use std::str::FromStr as _;

use chrono::{DateTime, Utc};
use jc_memory_protocol::{
    API_VERSION_V1, AccessScope, AgentId, BlobId, CognitiveFragment, EventId, EvidenceId,
    EvidenceRef, FragmentContent, FragmentId, FragmentKind, FragmentSource, MemoryCommand,
    MemoryRequest, MemoryResponse, MemoryScope, NamespaceId, ObserveCommand, ObserveResult,
    Perspective, ProjectId, ResponseEnvelope, SecurityLabel, SessionId, SourceType, TenantId,
    TraceId, UserId, ValidityStatus, VerificationMethod,
    generated::jiangcheng::memory::v1 as proto,
};
use prost::Message as _;

#[test]
fn request_proto_round_trip_preserves_all_fragment_content_variants() {
    let contents = [
        FragmentContent::Text {
            text: "保留原始观察文本".to_owned(),
        },
        FragmentContent::Json {
            value: serde_json::json!({
                "array": [null, true, -5, 1.5],
                "u64": 18_446_744_073_709_551_615_u64
            }),
        },
        FragmentContent::Blob {
            blob_id: BlobId::from_str(
                "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            )
            .expect("解析 BlobId"),
            media_type: "application/octet-stream".to_owned(),
            byte_len: u64::MAX,
        },
    ];

    for content in contents {
        let request = complete_request(content);
        let wire = proto::RequestEnvelope::try_from(request.clone()).expect("Rust 请求转 Proto");
        let decoded = proto::RequestEnvelope::decode(wire.encode_to_vec().as_slice())
            .expect("解码 Proto 请求");
        let round_trip = MemoryRequest::try_from(decoded).expect("Proto 请求转 Rust");

        assert_eq!(round_trip, request);
    }
}

#[test]
fn response_proto_round_trip_preserves_observe_result() {
    let response = ResponseEnvelope {
        api_version: API_VERSION_V1.to_owned(),
        request_id: "req-complete-response".to_owned(),
        response: MemoryResponse::Observe(ObserveResult {
            fragment_id: FragmentId::from_str("018f4c8f-5f7b-7d31-9a6c-b7a0f4f37006")
                .expect("解析 FragmentId"),
            event_id: EventId::from_str("018f4c8f-5f7b-7d31-9a6c-b7a0f4f37007")
                .expect("解析 EventId"),
            sequence: u64::MAX,
            content_hash: BlobId::from_str(
                "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789",
            )
            .expect("解析内容哈希"),
            created: true,
            quarantined: false,
        }),
    };
    let wire = proto::ResponseEnvelope::try_from(response.clone()).expect("Rust 响应转 Proto");
    let decoded =
        proto::ResponseEnvelope::decode(wire.encode_to_vec().as_slice()).expect("解码 Proto 响应");
    let round_trip = ResponseEnvelope::try_from(decoded).expect("Proto 响应转 Rust");

    assert_eq!(round_trip, response);
}

fn complete_request(content: FragmentContent) -> MemoryRequest {
    let scope = MemoryScope {
        namespace_id: NamespaceId::from_str("018f4c8f-5f7b-7d31-9a6c-b7a0f4f37001")
            .expect("解析 NamespaceId"),
        tenant_id: Some(
            TenantId::from_str("018f4c8f-5f7b-7d31-9a6c-b7a0f4f37002").expect("解析 TenantId"),
        ),
        user_id: Some(
            UserId::from_str("018f4c8f-5f7b-7d31-9a6c-b7a0f4f37003").expect("解析 UserId"),
        ),
        agent_id: Some(
            AgentId::from_str("018f4c8f-5f7b-7d31-9a6c-b7a0f4f37004").expect("解析 AgentId"),
        ),
        project_id: Some(
            ProjectId::from_str("018f4c8f-5f7b-7d31-9a6c-b7a0f4f37005").expect("解析 ProjectId"),
        ),
    };
    let fragment = CognitiveFragment {
        id: FragmentId::from_str("018f4c8f-5f7b-7d31-9a6c-b7a0f4f37006").expect("解析 FragmentId"),
        scope: scope.clone(),
        session_id: Some(
            SessionId::from_str("018f4c8f-5f7b-7d31-9a6c-b7a0f4f37008").expect("解析 SessionId"),
        ),
        trace_id: Some(
            TraceId::from_str("018f4c8f-5f7b-7d31-9a6c-b7a0f4f37009").expect("解析 TraceId"),
        ),
        kind: FragmentKind::Fact,
        content,
        source: FragmentSource {
            source_type: SourceType::Tool,
            source_id: "tool-complete-fixture".to_owned(),
            actor_id: Some("actor-1".to_owned()),
            observer_id: Some("observer-1".to_owned()),
            tool_id: Some("tool-1".to_owned()),
            uri: Some("urn:fixture:complete".to_owned()),
        },
        perspective: Perspective::Tool,
        occurred_at: timestamp("2026-07-30T08:00:00.123456789Z"),
        observed_at: timestamp("2026-07-30T08:00:01.987654321Z"),
        confidence: 0.875,
        importance: 0.625,
        validity: ValidityStatus::Verified,
        security_label: SecurityLabel::Restricted,
        evidence: vec![EvidenceRef {
            id: EvidenceId::from_str("018f4c8f-5f7b-7d31-9a6c-b7a0f4f3700a")
                .expect("解析 EvidenceId"),
            fragment_id: None,
            event_id: Some(
                EventId::from_str("018f4c8f-5f7b-7d31-9a6c-b7a0f4f3700b")
                    .expect("解析 Evidence EventId"),
            ),
            method: VerificationMethod::ExternalSource,
            note: Some("完整证据引用".to_owned()),
        }],
        metadata: serde_json::from_value(serde_json::json!({
            "exact_u64": 18_446_744_073_709_551_615_u64,
            "nested": {"array": [null, false, -7, 2.25]},
            "标签": "完整"
        }))
        .expect("构造 metadata"),
    };
    MemoryRequest {
        api_version: API_VERSION_V1.to_owned(),
        request_id: "req-complete-fragment".to_owned(),
        access_scope: AccessScope::from(scope),
        command: MemoryCommand::Observe(ObserveCommand { fragment }),
    }
}

fn timestamp(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value)
        .expect("解析时间 Fixture")
        .with_timezone(&Utc)
}
