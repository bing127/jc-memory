//! SDK canonical Fixture 兼容性门禁。

use std::fs;

use jc_memory_protocol::{MemoryCommand, MemoryRequest, validate_sdk_contracts};

#[test]
fn sdk_contract_fixtures_are_complete_and_canonical() {
    let manifest = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../sdk/contracts/manifest.json"
    );

    validate_sdk_contracts(manifest).expect("SDK 契约 Fixture 必须完整且 canonical SHA-256 稳定");
}

#[test]
fn sdk_contract_fixtures_reject_tampered_hash() {
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../sdk/contracts");
    let temporary = tempfile::tempdir().expect("创建临时契约目录");
    let fixtures = temporary.path().join("fixtures");
    fs::create_dir_all(&fixtures).expect("创建临时 Fixture 目录");
    fs::copy(
        source.join("manifest.json"),
        temporary.path().join("manifest.json"),
    )
    .expect("复制契约清单");
    for entry in fs::read_dir(source.join("fixtures")).expect("读取契约 Fixture") {
        let entry = entry.expect("读取 Fixture 目录项");
        fs::copy(entry.path(), fixtures.join(entry.file_name())).expect("复制 Fixture");
    }
    let tampered = fixtures.join("observe-basic.json");
    let mut content = fs::read_to_string(&tampered).expect("读取 Observe Fixture");
    content = content.replace(
        "a9c74713f0741d392e1b1e6e60b08a0318968f910b46efabd904877bcc02603a",
        "0000000000000000000000000000000000000000000000000000000000000000",
    );
    fs::write(&tampered, content).expect("篡改测试 Fixture");

    let error = validate_sdk_contracts(temporary.path().join("manifest.json"))
        .expect_err("篡改 canonical SHA-256 必须失败");
    assert!(error.to_string().contains("canonical_sha256"));
}

#[test]
fn sdk_contract_fixtures_reject_empty_state_packets_before_hash_validation() {
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../sdk/contracts");
    let temporary = tempfile::tempdir().expect("创建临时契约目录");
    copy_contracts(&source, temporary.path());
    let state_fixture = temporary.path().join("fixtures/recall-current-state.json");
    let mut fixture: serde_json::Value =
        serde_json::from_slice(&fs::read(&state_fixture).expect("读取 Current State Fixture"))
            .expect("解析 Current State Fixture");
    fixture["response"]["response"]["payload"]["current_states"] = serde_json::json!([]);
    fixture["response"]["response"]["payload"]["historical_states"] = serde_json::json!([]);
    fs::write(&state_fixture, serde_json::to_vec(&fixture).unwrap()).expect("写入空 State Fixture");

    let error = validate_sdk_contracts(temporary.path().join("manifest.json"))
        .expect_err("空 State Fixture 必须被拒绝");
    assert!(error.to_string().contains("current_states"), "{error}");
}

#[test]
fn legacy_mutation_request_without_state_remains_deserializable() {
    let request: MemoryRequest = serde_json::from_value(serde_json::json!({
        "api_version": "v1",
        "request_id": "legacy-correct-without-state",
        "access_scope": {
            "namespace_id": "00000000-0000-0000-0000-000000000001",
            "tenant_id": null,
            "user_id": null,
            "agent_id": null,
            "project_id": null
        },
        "command": {
            "command": "correct",
            "payload": {
                "target_fragment_id": "00000000-0000-0000-0000-000000000101",
                "content": {"type": "text", "text": "legacy correction"},
                "source": {"source_type": "user", "source_id": "legacy-sdk"},
                "perspective": "first_person",
                "occurred_at": "2026-01-01T00:00:00Z",
                "verification_method": "manual",
                "reason": "legacy client omitted state"
            }
        }
    }))
    .expect("旧 Mutation JSON 缺失 state 时仍应反序列化");
    let MemoryCommand::Correct(command) = request.command else {
        panic!("旧 Mutation Fixture 必须解析为 Correct")
    };
    assert!(command.state.is_none());
}

fn copy_contracts(source: &std::path::Path, target: &std::path::Path) {
    let fixtures = target.join("fixtures");
    fs::create_dir_all(&fixtures).expect("创建临时 Fixture 目录");
    fs::copy(source.join("manifest.json"), target.join("manifest.json")).expect("复制契约清单");
    for entry in fs::read_dir(source.join("fixtures")).expect("读取契约 Fixture") {
        let entry = entry.expect("读取 Fixture 目录项");
        fs::copy(entry.path(), fixtures.join(entry.file_name())).expect("复制 Fixture");
    }
}
