//! Node.js 的 napi-rs 嵌入式边界。
//!
//! 该 crate 只负责 JSON/JavaScript 与 Rust Core 的类型转换。权威记录、ACL、事务和
//! Recall 预算仍由 `jc-memory-core` 执行；边界本身不复制任何业务逻辑。
//!
//! `napi` 宏会生成 N-API 所需的少量 `unsafe` glue。该例外仅存在于本 crate 的
//! 绑定边界；本文件没有手写 `unsafe`，核心 crate 仍由工作区 `unsafe_code = forbid`
//! 保护，并通过 `cargo test -p jc-memory-node` 验证句柄和命令映射。

use jc_memory_core::{CommandBus, EmbeddedCommandBus, MemoryConfig, MemoryEngine};
use jc_memory_protocol::{
    AccessContext, Grant, MemoryCommand, MemoryRequest, MemoryScope, Operation, ResponseEnvelope,
};
use napi::bindgen_prelude::*;
use napi_derive::napi;

/// 复用 Core `CommandBus` 的 Node.js 嵌入式引擎句柄。
#[napi]
pub struct NodeMemoryEngine {
    bus: EmbeddedCommandBus,
    principal_id: String,
}

/// 打开一个本地 `SQLite` 嵌入式引擎。
///
/// # Errors
///
/// 数据目录、数据库迁移或底层存储初始化失败时返回错误。
#[napi]
pub async fn open_memory_engine(
    data_dir: String,
    principal_id: String,
) -> Result<NodeMemoryEngine> {
    let engine = MemoryEngine::open(MemoryConfig::local(data_dir))
        .await
        .map_err(to_napi_error)?;
    Ok(NodeMemoryEngine {
        bus: EmbeddedCommandBus::new(engine),
        principal_id,
    })
}

#[napi]
impl NodeMemoryEngine {
    /// 执行一个与远程 REST 完全相同的 JSON Request Envelope。
    ///
    /// # Errors
    ///
    /// JSON Envelope 无效、Core ACL 拒绝或持久化执行失败时返回错误。
    #[napi]
    pub async fn execute(&self, request_json: String) -> Result<String> {
        let request: MemoryRequest = serde_json::from_str(&request_json)
            .map_err(|error| Error::from_reason(format!("请求 Envelope 无效: {error}")))?;
        let response = self.execute_request(request).await.map_err(to_napi_error)?;
        serde_json::to_string(&response)
            .map_err(|error| Error::from_reason(format!("响应 Envelope 无法序列化: {error}")))
    }

    /// 关闭 Core 共享资源；重复关闭保持幂等。
    ///
    /// # Errors
    ///
    /// Core 资源无法安全关闭时返回错误。
    #[napi]
    pub async fn close(&self) -> Result<()> {
        self.bus.shutdown().await.map_err(to_napi_error)
    }

    /// 返回当前稳定协议版本，供 Node 包装器进行运行时能力检查。
    #[napi]
    #[must_use]
    pub fn protocol_version(&self) -> String {
        jc_memory_protocol::API_VERSION_V1.to_owned()
    }
}

impl NodeMemoryEngine {
    async fn execute_request(
        &self,
        request: MemoryRequest,
    ) -> std::result::Result<ResponseEnvelope, jc_memory_core::CommandError> {
        let scope: MemoryScope = request.access_scope.clone().into();
        let mut access = AccessContext::new(self.principal_id.clone(), scope.clone());
        access.request_id = request.request_id.clone();
        access
            .grants
            .insert(Grant::for_scope(operation_for(&request.command), &scope));
        self.bus.execute(&access, request).await
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

fn to_napi_error(error: impl std::fmt::Display) -> Error {
    Error::from_reason(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::operation_for;
    use jc_memory_protocol::{MemoryCommand, Operation, StatsCommand};

    #[test]
    fn stats_uses_read_grant() {
        assert_eq!(
            operation_for(&MemoryCommand::Stats(StatsCommand)),
            Operation::Recall
        );
    }
}
