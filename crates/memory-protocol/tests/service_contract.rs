//! M4 服务 Envelope 的 Rust 与 Protobuf 契约测试。

use std::{path::PathBuf, process::Command};

use jc_memory_protocol::{
    API_VERSION_V1, AccessScope, FragmentContent, MemoryCommand, MemoryRequest, MemoryScope,
    MutationCommand, NamespaceId, ObserveCommand, RequestValidationError, StateId, StateVersionId,
    StateView, ValidityStatus, generated::jiangcheng::memory::v1 as proto,
};
use prost::Message as _;

#[test]
fn generated_request_envelope_round_trips_protobuf_bytes() {
    let fragment = protocol_fragment();
    let request = MemoryRequest {
        api_version: API_VERSION_V1.to_owned(),
        request_id: "req-prost-round-trip".to_owned(),
        access_scope: AccessScope::from(fragment.scope.clone()),
        command: MemoryCommand::Observe(ObserveCommand { fragment }),
    };
    let request = proto::RequestEnvelope::try_from(request).expect("Rust 请求转 Proto");

    let decoded = proto::RequestEnvelope::decode(request.encode_to_vec().as_slice())
        .expect("解码 Prost 请求 Envelope");

    assert_eq!(decoded, request);
}

#[test]
fn buf_round_trips_observe_json_through_protobuf() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let proto_root = manifest_dir.join("../../proto");
    let fixture = manifest_dir.join("tests/fixtures/observe_request.json");
    let output_dir = tempfile::tempdir().expect("创建 Proto 往返目录");
    let binary = output_dir.path().join("request.binpb");
    let json = output_dir.path().join("request.json");

    let encode = Command::new("buf")
        .current_dir(&proto_root)
        .args([
            "convert",
            ".",
            "--type=jiangcheng.memory.v1.RequestEnvelope",
            &format!("--from={}#format=json", fixture.display()),
            &format!("--to={}#format=binpb", binary.display()),
        ])
        .output()
        .expect("运行 buf JSON -> Proto");
    assert!(
        encode.status.success(),
        "buf JSON -> Proto 失败: {}",
        String::from_utf8_lossy(&encode.stderr)
    );

    let decode = Command::new("buf")
        .current_dir(&proto_root)
        .args([
            "convert",
            ".",
            "--type=jiangcheng.memory.v1.RequestEnvelope",
            &format!("--from={}#format=binpb", binary.display()),
            &format!("--to={}#format=json", json.display()),
        ])
        .output()
        .expect("运行 buf Proto -> JSON");
    assert!(
        decode.status.success(),
        "buf Proto -> JSON 失败: {}",
        String::from_utf8_lossy(&decode.stderr)
    );

    let expected: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture).expect("读取版本化 JSON Fixture"))
            .expect("解析版本化 JSON Fixture");
    let actual: serde_json::Value =
        serde_json::from_slice(&std::fs::read(json).expect("读取 Buf 往返 JSON"))
            .expect("解析 Buf 往返 JSON");
    assert_eq!(actual, expected);

    let wire =
        proto::RequestEnvelope::decode(std::fs::read(binary).expect("读取 Buf Proto").as_slice())
            .expect("解码 Buf Proto");
    let request = MemoryRequest::try_from(wire).expect("完整 Fixture 转 Rust 请求");
    let MemoryCommand::Observe(command) = request.command else {
        panic!("Fixture command must be Observe");
    };
    assert_eq!(
        MemoryScope::from(request.access_scope),
        command.fragment.scope
    );
    assert!(matches!(
        command.fragment.content,
        FragmentContent::Text { .. }
    ));
    assert_eq!(
        command.fragment.metadata["exact_u64"],
        serde_json::json!(18_446_744_073_709_551_615_u64)
    );
}

#[test]
fn request_envelope_requires_version_request_id_and_contains_no_grant() {
    let fragment = protocol_fragment();
    let mut request = MemoryRequest {
        api_version: API_VERSION_V1.to_owned(),
        request_id: "req-rust-envelope".to_owned(),
        access_scope: AccessScope::from(fragment.scope.clone()),
        command: MemoryCommand::Observe(ObserveCommand { fragment }),
    };
    request.validate().expect("接受完整请求 Envelope");
    let json = serde_json::to_value(&request).expect("序列化请求 Envelope");
    assert!(json.get("api_version").is_some());
    assert!(json.get("request_id").is_some());
    assert!(json.get("access_scope").is_some());
    assert!(json.pointer("/access_scope/grants").is_none());

    request.request_id = "  ".to_owned();
    assert_eq!(
        request.validate(),
        Err(RequestValidationError::EmptyRequestId)
    );
    request.request_id = "req-unsupported-version".to_owned();
    request.api_version = "v2".to_owned();
    assert_eq!(
        request.validate(),
        Err(RequestValidationError::UnsupportedApiVersion {
            api_version: "v2".to_owned()
        })
    );
}

#[test]
fn legacy_mutation_json_defaults_missing_state_to_none() {
    let command: MutationCommand = serde_json::from_value(serde_json::json!({
        "target_fragment_id": "00000000-0000-7000-8000-000000000010",
        "content": {"type": "text", "text": "旧版更正内容"},
        "source": {
            "source_type": "user",
            "source_id": "operator",
            "actor_id": null,
            "observer_id": null,
            "tool_id": null,
            "uri": null
        },
        "perspective": "first_person",
        "occurred_at": "2026-08-02T10:00:00Z",
        "verification_method": "manual",
        "reason": "旧版客户端未携带 State"
    }))
    .expect("反序列化旧版 Mutation JSON");

    assert_eq!(command.state, None);
}

#[test]
fn state_view_omits_absent_optional_fields_from_json() {
    let state = StateView {
        state_id: StateId::new(),
        version_id: StateVersionId::new(),
        key: "project.deployment_strategy".to_owned(),
        value: serde_json::json!("blue_green"),
        valid_from: chrono::DateTime::parse_from_rfc3339("2026-08-02T09:00:00Z")
            .expect("解析状态生效时间")
            .with_timezone(&chrono::Utc),
        valid_to: None,
        validity: ValidityStatus::Unverified,
        evidence: Vec::new(),
        reason: None,
        source_event_id: None,
    };

    let json = serde_json::to_value(state).expect("序列化 StateView");
    assert!(json.get("reason").is_none());
    assert!(json.get("source_event_id").is_none());
}

fn protocol_fragment() -> jc_memory_protocol::CognitiveFragment {
    use chrono::Utc;
    use jc_memory_protocol::{
        FragmentContent, FragmentId, FragmentKind, FragmentSource, MemoryScope, Perspective,
        SecurityLabel, SourceType, ValidityStatus,
    };

    let now = Utc::now();
    jc_memory_protocol::CognitiveFragment {
        id: FragmentId::new(),
        scope: MemoryScope::new(NamespaceId::new()),
        session_id: None,
        trace_id: None,
        kind: FragmentKind::Observation,
        content: FragmentContent::Text {
            text: "协议 Envelope 证据".to_owned(),
        },
        source: FragmentSource {
            source_type: SourceType::System,
            source_id: "protocol-test".to_owned(),
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
