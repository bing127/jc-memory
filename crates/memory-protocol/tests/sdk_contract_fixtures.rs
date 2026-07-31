//! SDK canonical Fixture 兼容性门禁。

use std::fs;

use jc_memory_protocol::validate_sdk_contracts;

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
