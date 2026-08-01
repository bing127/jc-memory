//! Rust 领域 Envelope 与强类型 Protobuf 的无损转换契约。

use std::str::FromStr as _;

use chrono::{DateTime, Utc};
use jc_memory_protocol::{
    API_VERSION_V1, AccessScope, AgentId, BlobId, CognitiveFragment, CommitCommand, CommitResult,
    EventId, EvidenceId, EvidenceRef, ExplainResult, ExplainSeed, ExplainSurface, ExplainTraversal,
    FragmentContent, FragmentId, FragmentKind, FragmentSource, MemoryCommand, MemoryPacket,
    MemoryRequest, MemoryResponse, MemoryScope, MutationAction, MutationCommand, MutationResult,
    NamespaceId, ObserveCommand, ObserveResult, Perspective, PointId, ProjectId, RecallCommand,
    RecallLimits, RecallMode, ResponseEnvelope, SecurityLabel, SessionId, SourceType,
    StateDeclaration, StateId, StateVersionId, StateView, StatsCommand, StatsResult, SurfaceId,
    TenantId, TraceId, UserId, ValidityStatus, VerificationMethod,
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

#[test]
fn commit_proto_round_trip_preserves_fragment_batch_and_receipts() {
    let first = protocol_fragment("提交前观察");
    let second = CognitiveFragment {
        id: FragmentId::new(),
        scope: first.scope.clone(),
        session_id: None,
        trace_id: None,
        kind: FragmentKind::ToolResult,
        content: FragmentContent::Json {
            value: serde_json::json!({"ok": true}),
        },
        source: first.source.clone(),
        perspective: first.perspective,
        occurred_at: first.occurred_at,
        observed_at: first.observed_at,
        confidence: 1.0,
        importance: 0.5,
        validity: ValidityStatus::Unverified,
        security_label: SecurityLabel::Internal,
        evidence: Vec::new(),
        metadata: serde_json::Map::new(),
    };
    let request = MemoryRequest {
        api_version: API_VERSION_V1.to_owned(),
        request_id: "req-commit-round-trip".to_owned(),
        access_scope: AccessScope::from(first.scope.clone()),
        command: MemoryCommand::Commit(CommitCommand {
            fragments: vec![first, second],
        }),
    };
    let response = ResponseEnvelope::for_request(
        &request,
        MemoryResponse::Commit(CommitResult {
            receipts: Vec::new(),
        }),
    );
    let request_wire =
        proto::RequestEnvelope::try_from(request.clone()).expect("Commit 请求转 Proto");
    let request_decoded = proto::RequestEnvelope::decode(request_wire.encode_to_vec().as_slice())
        .expect("解码 Commit 请求");
    assert_eq!(
        MemoryRequest::try_from(request_decoded).expect("Commit 请求转 Rust"),
        request
    );

    let response_wire =
        proto::ResponseEnvelope::try_from(response.clone()).expect("Commit 响应转 Proto");
    let response_decoded =
        proto::ResponseEnvelope::decode(response_wire.encode_to_vec().as_slice())
            .expect("解码 Commit 响应");
    assert_eq!(
        ResponseEnvelope::try_from(response_decoded).expect("Commit 响应转 Rust"),
        response
    );
}

#[test]
fn recall_proto_round_trip_preserves_request_and_packet() {
    let request = MemoryRequest {
        api_version: API_VERSION_V1.to_owned(),
        request_id: "req-recall-round-trip".to_owned(),
        access_scope: AccessScope::from(MemoryScope::new(NamespaceId::new())),
        command: MemoryCommand::Recall(RecallCommand {
            mode: RecallMode::HistoricalState,
            text: "部署策略".to_owned(),
            entities: vec!["疆诚记忆".to_owned(), "生产环境".to_owned()],
            at_time: Some(timestamp("2026-07-30T08:00:00.123456789Z")),
            limits: RecallLimits {
                max_seeds: 3,
                max_neighbors: 5,
                max_hops: 2,
                max_nodes: 8,
                timeout_ms: 900,
            },
        }),
    };
    let mut packet = MemoryPacket::empty(
        jc_memory_protocol::RecallId::new(),
        RecallMode::HistoricalState,
    );
    packet.coverage.seed_count = 2;
    packet.coverage.visited_nodes = 4;
    packet.confidence = 0.75;
    packet.recommended_context = "结构化上下文".to_owned();
    packet.current_states.push(StateView {
        state_id: StateId::new(),
        version_id: StateVersionId::new(),
        key: "project.deployment_strategy".to_owned(),
        value: serde_json::json!("canary"),
        valid_from: timestamp("2026-08-02T10:00:00Z"),
        valid_to: None,
        validity: ValidityStatus::Verified,
        evidence: Vec::new(),
        reason: Some("降低全量发布风险".to_owned()),
        source_event_id: Some(EventId::new()),
    });
    let response = ResponseEnvelope::for_request(&request, MemoryResponse::Recall(packet));

    let request_wire =
        proto::RequestEnvelope::try_from(request.clone()).expect("Recall 请求转 Proto");
    let request_decoded = proto::RequestEnvelope::decode(request_wire.encode_to_vec().as_slice())
        .expect("解码 Recall 请求");
    assert_eq!(
        MemoryRequest::try_from(request_decoded).expect("Recall 请求转 Rust"),
        request
    );

    let response_wire =
        proto::ResponseEnvelope::try_from(response.clone()).expect("Recall 响应转 Proto");
    let response_decoded =
        proto::ResponseEnvelope::decode(response_wire.encode_to_vec().as_slice())
            .expect("解码 Recall 响应");
    assert_eq!(
        ResponseEnvelope::try_from(response_decoded).expect("Recall 响应转 Rust"),
        response
    );
}

#[test]
fn stats_proto_round_trip_preserves_scope_counts() {
    let scope = AccessScope::from(MemoryScope::new(NamespaceId::new()));
    let request = MemoryRequest {
        api_version: API_VERSION_V1.to_owned(),
        request_id: "req-stats-round-trip".to_owned(),
        access_scope: scope.clone(),
        command: MemoryCommand::Stats(StatsCommand),
    };
    let response = ResponseEnvelope::for_request(
        &request,
        MemoryResponse::Stats(StatsResult {
            scope,
            fragment_count: u64::MAX,
            event_count: 42,
        }),
    );

    let request_wire =
        proto::RequestEnvelope::try_from(request.clone()).expect("Stats 请求转 Proto");
    let request_decoded = proto::RequestEnvelope::decode(request_wire.encode_to_vec().as_slice())
        .expect("解码 Stats 请求");
    assert_eq!(
        MemoryRequest::try_from(request_decoded).expect("Stats 请求转 Rust"),
        request
    );

    let response_wire =
        proto::ResponseEnvelope::try_from(response.clone()).expect("Stats 响应转 Proto");
    let response_decoded =
        proto::ResponseEnvelope::decode(response_wire.encode_to_vec().as_slice())
            .expect("解码 Stats 响应");
    assert_eq!(
        ResponseEnvelope::try_from(response_decoded).expect("Stats 响应转 Rust"),
        response
    );
}

#[test]
fn mutation_proto_round_trip_preserves_correct_and_forget() {
    let scope = AccessScope::from(MemoryScope::new(NamespaceId::new()));
    let target_fragment_id = "00000000-0000-7000-8000-000000000010".parse().unwrap();
    for (action, command) in [
        (
            MutationAction::Correct,
            MemoryCommand::Correct(MutationCommand {
                target_fragment_id,
                content: FragmentContent::Text {
                    text: "更正后的事实".into(),
                },
                source: FragmentSource {
                    source_type: SourceType::User,
                    source_id: "operator".into(),
                    actor_id: None,
                    observer_id: None,
                    tool_id: None,
                    uri: Some("urn:fixture:correction".into()),
                },
                perspective: Perspective::FirstPerson,
                occurred_at: timestamp("2026-07-31T01:02:03Z"),
                verification_method: VerificationMethod::Manual,
                reason: "人工核验".into(),
                state: Some(StateDeclaration {
                    key: "project.deployment_strategy".to_owned(),
                    value: serde_json::json!("canary"),
                    reason: "降低全量发布风险".to_owned(),
                }),
            }),
        ),
        (
            MutationAction::Forget,
            MemoryCommand::Forget(MutationCommand {
                target_fragment_id,
                content: FragmentContent::Text {
                    text: "应当失效的事实".into(),
                },
                source: FragmentSource {
                    source_type: SourceType::System,
                    source_id: "retention".into(),
                    actor_id: None,
                    observer_id: None,
                    tool_id: None,
                    uri: None,
                },
                perspective: Perspective::System,
                occurred_at: timestamp("2026-07-31T02:03:04Z"),
                verification_method: VerificationMethod::Tool,
                reason: "保留策略".into(),
                state: None,
            }),
        ),
    ] {
        let request = MemoryRequest {
            api_version: API_VERSION_V1.to_owned(),
            request_id: format!("req-{action:?}"),
            access_scope: scope.clone(),
            command,
        };
        let response = ResponseEnvelope::for_request(
            &request,
            MemoryResponse::Mutation(MutationResult {
                action,
                target_fragment_id,
                fragment_id: "00000000-0000-7000-8000-000000000011".parse().unwrap(),
                event_id: "00000000-0000-7000-8000-000000000012".parse().unwrap(),
                sequence: 7,
                relation: None,
            }),
        );

        let request_wire =
            proto::RequestEnvelope::try_from(request.clone()).expect("变更请求转 Proto");
        let request_decoded =
            proto::RequestEnvelope::decode(request_wire.encode_to_vec().as_slice())
                .expect("解码变更请求");
        assert_eq!(
            MemoryRequest::try_from(request_decoded).expect("变更请求转 Rust"),
            request
        );

        let response_wire =
            proto::ResponseEnvelope::try_from(response.clone()).expect("变更响应转 Proto");
        let response_decoded =
            proto::ResponseEnvelope::decode(response_wire.encode_to_vec().as_slice())
                .expect("解码变更响应");
        assert_eq!(
            ResponseEnvelope::try_from(response_decoded).expect("变更响应转 Rust"),
            response
        );
    }
}

#[test]
fn explain_proto_round_trip_preserves_trace_fields() {
    let request = MemoryRequest {
        api_version: API_VERSION_V1.to_owned(),
        request_id: "req-explain-round-trip".to_owned(),
        access_scope: AccessScope::from(MemoryScope::new(NamespaceId::new())),
        command: MemoryCommand::Explain(RecallCommand {
            mode: RecallMode::Causal,
            text: "故障原因".into(),
            entities: vec!["数据库".into()],
            at_time: None,
            limits: RecallLimits {
                max_seeds: 2,
                max_neighbors: 3,
                max_hops: 4,
                max_nodes: 5,
                timeout_ms: 600,
            },
        }),
    };
    let seed_point: PointId = "00000000-0000-7000-8000-000000000020".parse().unwrap();
    let target_point: PointId = "00000000-0000-7000-8000-000000000021".parse().unwrap();
    let surface_id: SurfaceId = "00000000-0000-7000-8000-000000000022".parse().unwrap();
    let edge_id = "00000000-0000-7000-8000-000000000023".parse().unwrap();
    let explain = ExplainResult {
        seeds: vec![ExplainSeed {
            point_id: seed_point,
            score: 0.9,
        }],
        traversals: vec![ExplainTraversal {
            edge_id,
            source_point_id: seed_point,
            target_point_id: target_point,
            kind: jc_memory_protocol::EdgeKind::Causes,
            score: 0.7,
            depth: 1,
        }],
        surfaces: vec![ExplainSurface {
            surface_id,
            version: 3,
            activation: 0.6,
        }],
        inferred_edges: vec![edge_id],
        verified_fragments: vec!["00000000-0000-7000-8000-000000000024".parse().unwrap()],
        limits: RecallLimits {
            max_seeds: 2,
            max_neighbors: 3,
            max_hops: 4,
            max_nodes: 5,
            timeout_ms: 600,
        },
        truncated: true,
    };
    let response = ResponseEnvelope::for_request(&request, MemoryResponse::Explain(explain));

    let request_wire =
        proto::RequestEnvelope::try_from(request.clone()).expect("Explain 请求转 Proto");
    let request_decoded = proto::RequestEnvelope::decode(request_wire.encode_to_vec().as_slice())
        .expect("解码 Explain 请求");
    assert_eq!(
        MemoryRequest::try_from(request_decoded).expect("Explain 请求转 Rust"),
        request
    );

    let response_wire =
        proto::ResponseEnvelope::try_from(response.clone()).expect("Explain 响应转 Proto");
    let response_decoded =
        proto::ResponseEnvelope::decode(response_wire.encode_to_vec().as_slice())
            .expect("解码 Explain 响应");
    assert_eq!(
        ResponseEnvelope::try_from(response_decoded).expect("Explain 响应转 Rust"),
        response
    );
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

fn protocol_fragment(text: &str) -> CognitiveFragment {
    let now = Utc::now();
    CognitiveFragment {
        id: FragmentId::new(),
        scope: MemoryScope::new(NamespaceId::new()),
        session_id: None,
        trace_id: None,
        kind: FragmentKind::Observation,
        content: FragmentContent::Text {
            text: text.to_owned(),
        },
        source: FragmentSource {
            source_type: SourceType::System,
            source_id: "commit-test".to_owned(),
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

fn timestamp(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value)
        .expect("解析时间 Fixture")
        .with_timezone(&Utc)
}
