//! Memory Poisoning 场景的确定性 fail-closed Runner。

use std::{fs, path::Path};

use serde::Deserialize;
use thiserror::Error;

/// 污染场景的预期安全结果。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
pub struct Outcome {
    /// 是否进入隔离区。
    pub quarantined: bool,
    /// 是否拒绝该操作或跨租户访问。
    pub denied: bool,
}

/// 一个版本化污染场景。
#[derive(Clone, Debug, Deserialize)]
pub struct Case {
    /// 稳定场景 ID。
    pub id: String,
    /// 场景分类，不从自然语言推断。
    pub kind: String,
    /// 预期 fail-closed 结果。
    pub expected: Outcome,
}

/// 污染 Fixture。
#[derive(Clone, Debug, Deserialize)]
pub struct Suite {
    /// Schema 版本。
    pub schema_version: String,
    /// 是否仅为 Smoke。
    pub smoke: bool,
    /// 场景列表。
    pub cases: Vec<Case>,
}

/// Fixture 读取错误。
#[derive(Debug, Error)]
pub enum Error {
    /// 文件读取错误。
    #[error("读取 Memory Poisoning Fixture 失败: {0}")]
    Io(#[from] std::io::Error),
    /// JSON 错误。
    #[error("Memory Poisoning Fixture 无效: {0}")]
    Json(#[from] serde_json::Error),
    /// 版本或场景契约错误。
    #[error("Memory Poisoning Fixture 契约无效: {0}")]
    Invalid(String),
}

/// 读取并校验污染 Fixture。
///
/// # Errors
///
/// 当文件不存在、JSON 无效或 Fixture 不符合版本化契约时返回错误。
pub fn load_fixture<P: AsRef<Path>>(path: P) -> Result<Suite, Error> {
    let suite: Suite = serde_json::from_slice(&fs::read(path)?)?;
    if suite.schema_version != "jcmem.poisoning.v1" || !suite.smoke || suite.cases.is_empty() {
        return Err(Error::Invalid("必须是非空 Smoke Fixture".to_owned()));
    }
    if suite.cases.iter().any(|case| case.id.trim().is_empty()) {
        return Err(Error::Invalid("场景 ID 不能为空".to_owned()));
    }
    Ok(suite)
}

/// 按场景类型执行 fail-closed 规则。
#[must_use]
pub fn evaluate(case: &Case) -> Outcome {
    match case.kind.as_str() {
        "injection" | "unverified_inference" | "repeated_error" | "malicious_tool_result" => {
            Outcome {
                quarantined: true,
                denied: false,
            }
        }
        _ => Outcome {
            quarantined: true,
            denied: true,
        },
    }
}
