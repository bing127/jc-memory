//! 跨语言 SDK 使用的版本化 JSON 契约 Fixture 校验器。

use std::{collections::BTreeSet, fs, path::Path};

use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest as _, Sha256};

const REQUIRED_KINDS: &[&str] = &[
    "observe",
    "recall_exact",
    "recall_semantic",
    "recall_temporal",
    "recall_causal",
    "recall_entity",
    "recall_associative",
    "recall_procedural",
    "recall_current_state",
    "recall_historical_state",
    "recall_deep_reconstruction",
    "commit",
    "correct",
    "explain",
    "forget",
    "stats",
    "error_invalid_argument",
];

/// SDK 契约 Fixture 清单。
#[derive(Debug, Deserialize)]
struct Manifest {
    schema_version: u32,
    protocol_version: String,
    canonicalization: String,
    fixtures: Vec<ManifestFixture>,
}

#[derive(Debug, Deserialize)]
struct ManifestFixture {
    id: String,
    file: String,
    kind: String,
    required_features: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct Fixture {
    id: String,
    kind: String,
    protocol_version: String,
    required_features: Vec<String>,
    request: Value,
    response: Value,
    canonical_sha256: String,
}

/// SDK 契约 Fixture 校验错误。
#[derive(Debug, thiserror::Error)]
pub enum SdkContractError {
    /// 文件无法读取。
    #[error("无法读取 SDK 契约文件 {path}: {message}")]
    Read {
        /// 无法读取的文件路径。
        path: String,
        /// 操作系统错误文本。
        message: String,
    },
    /// JSON 结构不符合契约。
    #[error("SDK 契约 JSON 无效 {path}: {message}")]
    Json {
        /// JSON 文件路径。
        path: String,
        /// 解析错误文本。
        message: String,
    },
    /// 清单或 Fixture 违反稳定约束。
    #[error("SDK 契约校验失败: {0}")]
    Invalid(String),
}

/// 校验 M5 共享 SDK 契约清单与所有 Fixture。
///
/// `manifest_path` 应指向 `sdk/contracts/manifest.json`。校验使用递归排序 JSON
/// 后的紧凑表示计算 SHA-256，因此不同语言可以复用同一规范化规则。
///
/// # Errors
///
/// 清单、Fixture、必需命令种类或 canonical SHA-256 不匹配时返回错误。
pub fn validate_sdk_contracts(manifest_path: impl AsRef<Path>) -> Result<(), SdkContractError> {
    let manifest_path = manifest_path.as_ref();
    let manifest: Manifest = read_json(manifest_path)?;
    if manifest.schema_version != 1 {
        return Err(SdkContractError::Invalid(
            "schema_version 必须为 1".to_owned(),
        ));
    }
    if manifest.protocol_version != "v1" {
        return Err(SdkContractError::Invalid(
            "protocol_version 必须为 v1".to_owned(),
        ));
    }
    if manifest.canonicalization != "sorted-json-sha256" {
        return Err(SdkContractError::Invalid(
            "canonicalization 必须为 sorted-json-sha256".to_owned(),
        ));
    }

    let manifest_dir = manifest_path
        .parent()
        .ok_or_else(|| SdkContractError::Invalid("manifest 没有父目录".to_owned()))?;
    let mut ids = BTreeSet::new();
    let mut kinds = BTreeSet::new();
    for entry in &manifest.fixtures {
        if !ids.insert(entry.id.clone()) {
            return Err(SdkContractError::Invalid(format!(
                "Fixture id 重复: {}",
                entry.id
            )));
        }
        if entry.required_features.is_empty() {
            return Err(SdkContractError::Invalid(format!(
                "Fixture 缺少 required_features: {}",
                entry.id
            )));
        }
        if !REQUIRED_KINDS.contains(&entry.kind.as_str()) {
            return Err(SdkContractError::Invalid(format!(
                "Fixture kind 未被协议允许: {}",
                entry.kind
            )));
        }
        kinds.insert(entry.kind.clone());
        let fixture_path = manifest_dir.join(&entry.file);
        let fixture: Fixture = read_json(&fixture_path)?;
        validate_fixture(&fixture, entry, &fixture_path)?;
    }
    for required_kind in REQUIRED_KINDS {
        if !kinds.contains(*required_kind) {
            return Err(SdkContractError::Invalid(format!(
                "缺少必需 Fixture kind: {required_kind}"
            )));
        }
    }
    Ok(())
}

fn validate_fixture(
    fixture: &Fixture,
    manifest_entry: &ManifestFixture,
    path: &Path,
) -> Result<(), SdkContractError> {
    if fixture.id != manifest_entry.id || fixture.kind != manifest_entry.kind {
        return Err(SdkContractError::Invalid(format!(
            "Fixture 元数据与 manifest 不一致: {}",
            path.display()
        )));
    }
    if fixture.protocol_version != "v1" {
        return Err(SdkContractError::Invalid(format!(
            "Fixture protocol_version 必须为 v1: {}",
            path.display()
        )));
    }
    if fixture.required_features != manifest_entry.required_features {
        return Err(SdkContractError::Invalid(format!(
            "Fixture required_features 与 manifest 不一致: {}",
            path.display()
        )));
    }
    validate_request(&fixture.request, &fixture.kind, path)?;
    validate_response(&fixture.response, &fixture.kind, path)?;
    let canonical = canonical_payload(&fixture.request, &fixture.response)?;
    let actual = hex::encode(Sha256::digest(canonical.as_bytes()));
    if actual != fixture.canonical_sha256 {
        return Err(SdkContractError::Invalid(format!(
            "Fixture canonical_sha256 不匹配 {}: expected {}, actual {}",
            path.display(),
            fixture.canonical_sha256,
            actual
        )));
    }
    Ok(())
}

fn validate_request(request: &Value, kind: &str, path: &Path) -> Result<(), SdkContractError> {
    let object = request.as_object().ok_or_else(|| {
        SdkContractError::Invalid(format!("request 必须是对象: {}", path.display()))
    })?;
    for field in ["api_version", "request_id", "access_scope", "command"] {
        if !object.contains_key(field) {
            return Err(SdkContractError::Invalid(format!(
                "request 缺少字段 {field}: {}",
                path.display()
            )));
        }
    }
    if object["api_version"] != "v1" || object["request_id"].as_str().is_none_or(str::is_empty) {
        return Err(SdkContractError::Invalid(format!(
            "request api_version 或 request_id 无效: {}",
            path.display()
        )));
    }
    let command = object["command"].as_object().ok_or_else(|| {
        SdkContractError::Invalid(format!("command 必须是对象: {}", path.display()))
    })?;
    if command.get("command").and_then(Value::as_str) != Some(command_name(kind)) {
        return Err(SdkContractError::Invalid(format!(
            "command 与 Fixture kind 不一致: {}",
            path.display()
        )));
    }
    if kind == "commit"
        && command
            .get("payload")
            .and_then(Value::as_object)
            .and_then(|payload| payload.get("fragments"))
            .and_then(Value::as_array)
            .is_none_or(Vec::is_empty)
    {
        return Err(SdkContractError::Invalid(format!(
            "Commit Fixture 必须包含至少一条 Fragment: {}",
            path.display()
        )));
    }
    Ok(())
}

fn validate_response(response: &Value, kind: &str, path: &Path) -> Result<(), SdkContractError> {
    let object = response.as_object().ok_or_else(|| {
        SdkContractError::Invalid(format!("response 必须是对象: {}", path.display()))
    })?;
    for field in ["api_version", "request_id"] {
        if !object.contains_key(field) {
            return Err(SdkContractError::Invalid(format!(
                "response 缺少字段 {field}: {}",
                path.display()
            )));
        }
    }
    if object["api_version"] != "v1" || object["request_id"].as_str().is_none() {
        return Err(SdkContractError::Invalid(format!(
            "response 元数据无效: {}",
            path.display()
        )));
    }
    if kind.starts_with("error_") {
        let error = object
            .get("error")
            .and_then(Value::as_object)
            .ok_or_else(|| {
                SdkContractError::Invalid(format!("错误 Fixture 缺少 error: {}", path.display()))
            })?;
        if !matches!(
            error.get("code").and_then(Value::as_str),
            Some(
                "invalid_argument"
                    | "permission_denied"
                    | "not_found"
                    | "conflict"
                    | "resource_exhausted"
                    | "internal"
            )
        ) {
            return Err(SdkContractError::Invalid(format!(
                "错误 Fixture code 不稳定: {}",
                path.display()
            )));
        }
    } else if object
        .get("response")
        .and_then(Value::as_object)
        .and_then(|value| value.get("response"))
        .and_then(Value::as_str)
        .is_none()
    {
        return Err(SdkContractError::Invalid(format!(
            "成功 Fixture 缺少 response.response: {}",
            path.display()
        )));
    }
    if matches!(kind, "recall_current_state" | "recall_historical_state") {
        validate_state_packet(object, path)?;
    }
    Ok(())
}

fn validate_state_packet(
    response: &serde_json::Map<String, Value>,
    path: &Path,
) -> Result<(), SdkContractError> {
    let payload = response
        .get("response")
        .and_then(Value::as_object)
        .and_then(|response| response.get("payload"))
        .and_then(Value::as_object)
        .ok_or_else(|| {
            SdkContractError::Invalid(format!(
                "State Fixture 缺少 Recall payload: {}",
                path.display()
            ))
        })?;
    let mut has_evidence_ref_id = false;
    for field in ["current_states", "historical_states"] {
        let states = payload
            .get(field)
            .and_then(Value::as_array)
            .ok_or_else(|| {
                SdkContractError::Invalid(format!("State Fixture 缺少 {field}: {}", path.display()))
            })?;
        if states.is_empty() {
            return Err(SdkContractError::Invalid(format!(
                "State Fixture {field} 不得为空: {}",
                path.display()
            )));
        }
        for state in states {
            validate_state_item(state, field, path, &mut has_evidence_ref_id)?;
        }
    }
    if !has_evidence_ref_id {
        return Err(SdkContractError::Invalid(format!(
            "State Fixture 缺少 EvidenceRef ID: {}",
            path.display()
        )));
    }
    Ok(())
}

fn validate_state_item(
    state: &Value,
    field: &str,
    path: &Path,
    has_evidence_ref_id: &mut bool,
) -> Result<(), SdkContractError> {
    let state = state.as_object().ok_or_else(|| {
        SdkContractError::Invalid(format!(
            "State Fixture {field} 条目必须是对象: {}",
            path.display()
        ))
    })?;
    for id_field in ["state_id", "version_id", "source_event_id"] {
        if state
            .get(id_field)
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
        {
            return Err(SdkContractError::Invalid(format!(
                "State Fixture {field} 缺少 {id_field}: {}",
                path.display()
            )));
        }
    }
    for text_field in ["key", "reason"] {
        if state
            .get(text_field)
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
        {
            return Err(SdkContractError::Invalid(format!(
                "State Fixture {field} 缺少 {text_field}: {}",
                path.display()
            )));
        }
    }
    if state.get("value").is_none_or(Value::is_null) {
        return Err(SdkContractError::Invalid(format!(
            "State Fixture {field} 缺少 value: {}",
            path.display()
        )));
    }
    let evidence = state
        .get("evidence")
        .and_then(Value::as_array)
        .filter(|evidence| !evidence.is_empty())
        .ok_or_else(|| {
            SdkContractError::Invalid(format!(
                "State Fixture {field} 缺少 Evidence ID: {}",
                path.display()
            ))
        })?;
    for item in evidence {
        let item = item.as_object().ok_or_else(|| {
            SdkContractError::Invalid(format!(
                "State Fixture {field} Evidence 必须是对象: {}",
                path.display()
            ))
        })?;
        for id_field in ["fragment_id", "event_id"] {
            if item
                .get(id_field)
                .and_then(Value::as_str)
                .is_none_or(str::is_empty)
            {
                return Err(SdkContractError::Invalid(format!(
                    "State Fixture {field} Evidence 缺少 {id_field}: {}",
                    path.display()
                )));
            }
        }
        *has_evidence_ref_id |= item
            .get("evidence_ref_id")
            .and_then(Value::as_str)
            .is_some_and(|value| !value.is_empty());
    }
    Ok(())
}

fn command_name(kind: &str) -> &'static str {
    if kind.starts_with("recall_") || kind == "explain" {
        if kind == "explain" {
            "explain"
        } else {
            "recall"
        }
    } else if kind.starts_with("error_") {
        "observe"
    } else {
        match kind {
            "observe" => "observe",
            "commit" => "commit",
            "correct" => "correct",
            "forget" => "forget",
            "stats" => "stats",
            _ => "unknown",
        }
    }
}

fn canonical_payload(request: &Value, response: &Value) -> Result<String, SdkContractError> {
    let value = canonicalize(serde_json::json!({
        "request": request,
        "response": response,
    }));
    serde_json::to_string(&value).map_err(|error| SdkContractError::Json {
        path: "canonical payload".to_owned(),
        message: error.to_string(),
    })
}

fn canonicalize(value: Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.into_iter().map(canonicalize).collect()),
        Value::Object(values) => {
            let mut entries: Vec<_> = values.into_iter().collect();
            entries.sort_by(|left, right| left.0.cmp(&right.0));
            Value::Object(
                entries
                    .into_iter()
                    .map(|(key, value)| (key, canonicalize(value)))
                    .collect(),
            )
        }
        scalar => scalar,
    }
}

fn read_json<T>(path: &Path) -> Result<T, SdkContractError>
where
    T: for<'de> Deserialize<'de>,
{
    let bytes = fs::read(path).map_err(|error| SdkContractError::Read {
        path: path.display().to_string(),
        message: error.to_string(),
    })?;
    serde_json::from_slice(&bytes).map_err(|error| SdkContractError::Json {
        path: path.display().to_string(),
        message: error.to_string(),
    })
}
