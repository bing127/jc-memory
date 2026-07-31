//! `UniFFI` 的 JVM Embedded 边界。
//!
//! 该 crate 只负责把同步 JVM 调用转换为 Core `CommandBus` 调用；权威 `Fragment`、`Event`
//! 和 `EvidenceRef` 仍由 `memory-core` 负责，绑定层不复制记忆语义。

use std::sync::Mutex;

use jc_memory_core::{AccessContext, CommandBus, EmbeddedCommandBus, MemoryConfig, MemoryEngine};
use jc_memory_protocol::{
    CommandErrorCode, Grant, MemoryCommand, MemoryRequest, MemoryScope, Operation,
};

/// JVM Embedded 调用的稳定错误。
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum JcMemoryError {
    /// 请求 JSON 无法解析或不符合协议。
    #[error("invalid_argument: {message}")]
    InvalidArgument {
        /// 供调用方诊断的校验信息。
        message: String,
    },
    /// Core 命令执行失败。
    #[error("{code}: {message}")]
    Core {
        /// 稳定错误码。
        code: String,
        /// Core 返回的诊断信息。
        message: String,
    },
    /// 引擎已经关闭。
    #[error("internal: 引擎已经关闭")]
    Closed,
    /// 本地引擎无法打开或关闭。
    #[error("internal: {message}")]
    Internal {
        /// 绑定边界的诊断信息。
        message: String,
    },
}

/// 复用 Rust Core 的 JVM Embedded 引擎。
#[derive(uniffi::Object)]
pub struct JcMemoryEngine {
    state: Mutex<Option<EngineState>>,
}

struct EngineState {
    runtime: tokio::runtime::Runtime,
    bus: EmbeddedCommandBus,
    principal_id: String,
}

#[uniffi::export]
impl JcMemoryEngine {
    /// 打开本地数据库并创建一个绑定句柄。
    ///
    /// # Errors
    ///
    /// 本地数据库或 Tokio runtime 无法打开时返回 `internal` 错误。
    #[uniffi::constructor]
    pub fn new(data_dir: String, principal_id: String) -> Result<Self, JcMemoryError> {
        let runtime = tokio::runtime::Runtime::new().map_err(|error| JcMemoryError::Internal {
            message: format!("无法创建 Tokio runtime: {error}"),
        })?;
        let engine = runtime
            .block_on(MemoryEngine::open(MemoryConfig::local(data_dir)))
            .map_err(|error| JcMemoryError::Internal {
                message: error.to_string(),
            })?;
        Ok(Self {
            state: Mutex::new(Some(EngineState {
                runtime,
                bus: EmbeddedCommandBus::new(engine),
                principal_id,
            })),
        })
    }

    /// 执行一个 JSON Request Envelope 并返回 JSON Response Envelope。
    ///
    /// # Errors
    ///
    /// 请求无效、引擎已关闭或 Core 命令失败时返回稳定错误。
    #[allow(clippy::needless_pass_by_value)]
    pub fn execute(&self, request_json: String) -> Result<String, JcMemoryError> {
        let state = self.state.lock().map_err(|_| JcMemoryError::Internal {
            message: "引擎状态锁损坏".to_owned(),
        })?;
        let state = state.as_ref().ok_or(JcMemoryError::Closed)?;
        let request: MemoryRequest = serde_json::from_str(&request_json).map_err(|error| {
            JcMemoryError::InvalidArgument {
                message: format!("请求 Envelope 无效: {error}"),
            }
        })?;
        let scope: MemoryScope = request.access_scope.clone().into();
        let mut access = AccessContext::new(state.principal_id.clone(), scope.clone());
        access.request_id.clone_from(&request.request_id);
        access
            .grants
            .insert(Grant::for_scope(operation_for(&request.command), &scope));
        let response = state
            .runtime
            .block_on(state.bus.execute(&access, request))
            .map_err(|error| core_error(&error))?;
        serde_json::to_string(&response).map_err(|error| JcMemoryError::Internal {
            message: format!("响应 Envelope 序列化失败: {error}"),
        })
    }

    /// 关闭引擎；重复关闭保持幂等。
    ///
    /// # Errors
    ///
    /// Core 无法释放持久化资源时返回稳定错误。
    pub fn close(&self) -> Result<(), JcMemoryError> {
        let mut state = self.state.lock().map_err(|_| JcMemoryError::Internal {
            message: "引擎状态锁损坏".to_owned(),
        })?;
        let Some(state_value) = state.take() else {
            return Ok(());
        };
        state_value
            .runtime
            .block_on(state_value.bus.shutdown())
            .map_err(|error| core_error(&error))
    }
}

fn core_error(error: &jc_memory_core::CommandError) -> JcMemoryError {
    JcMemoryError::Core {
        code: error_code(error.code()).to_owned(),
        message: error.to_string(),
    }
}

fn error_code(code: CommandErrorCode) -> &'static str {
    match code {
        CommandErrorCode::InvalidArgument => "invalid_argument",
        CommandErrorCode::PermissionDenied => "permission_denied",
        CommandErrorCode::NotFound => "not_found",
        CommandErrorCode::Conflict => "conflict",
        CommandErrorCode::ResourceExhausted => "resource_exhausted",
        CommandErrorCode::Internal => "internal",
    }
}

fn operation_for(command: &MemoryCommand) -> Operation {
    match command {
        MemoryCommand::Observe(_) | MemoryCommand::Commit(_) => Operation::Observe,
        MemoryCommand::Recall(_) | MemoryCommand::Stats(_) => Operation::Recall,
        MemoryCommand::Explain(_) => Operation::Explain,
        MemoryCommand::Correct(_) | MemoryCommand::Forget(_) => Operation::Mutate,
    }
}

uniffi::setup_scaffolding!();

#[cfg(test)]
mod tests {
    use super::JcMemoryEngine;
    use serde_json::Value;
    use std::{fs, path::Path};

    #[test]
    fn executes_canonical_recall_fixture_and_closes_idempotently() {
        let directory = tempfile::tempdir().expect("创建临时目录");
        let fixture = fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../sdk/contracts/fixtures/recall-exact.json"),
        )
        .expect("读取 Fixture");
        let fixture: Value = serde_json::from_str(&fixture).expect("解析 Fixture");
        let request = fixture["request"].to_string();
        let engine = JcMemoryEngine::new(
            directory.path().to_string_lossy().into_owned(),
            "fixture-principal".to_owned(),
        )
        .expect("打开引擎");
        let response: Value =
            serde_json::from_str(&engine.execute(request).expect("执行请求")).expect("解析响应");
        assert_eq!(response["request_id"], "fixture-recall-exact");
        assert_eq!(response["response"]["response"], "recall");
        engine.close().expect("关闭引擎");
        engine.close().expect("重复关闭保持幂等");
        assert!(matches!(
            engine.execute("{}".to_owned()),
            Err(super::JcMemoryError::Closed)
        ));
    }
}
