//! Dify Scope 和工作流节点映射。

use std::str::FromStr;

use jc_memory_protocol::{
    AgentId, MemoryPacket, MemoryScope, ProjectId, SessionId, TraceId, UserId,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::config::DifyConfig;

/// Dify 一次工作流调用的外部标识集合。
#[derive(Clone, Debug, PartialEq)]
pub struct DifyInvocation {
    /// Dify 用户标识。
    pub user_id: String,
    /// Dify 会话标识。
    pub conversation_id: String,
    /// Dify 应用标识。
    pub app_id: String,
    /// Dify 工作流标识。
    pub workflow_id: String,
    /// Dify 追踪标识。
    pub trace_id: String,
}

/// Dify Scope 中未改写的外部标识。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DifyExternalIds {
    /// 原始 Dify 用户标识。
    pub user_id: String,
    /// 原始 Dify 会话标识。
    pub conversation_id: String,
    /// 原始 Dify 应用标识。
    pub app_id: String,
    /// 原始 Dify 工作流标识。
    pub workflow_id: String,
    /// 原始 Dify 追踪标识。
    pub trace_id: String,
}

/// 映射后的 Dify Scope。
#[derive(Clone, Debug, PartialEq)]
pub struct DifyScope {
    /// Core ACL 使用的 Scope。
    pub scope: MemoryScope,
    /// 稳定会话标识。
    pub session_id: SessionId,
    /// 稳定追踪标识。
    pub trace_id: TraceId,
    /// 可重建的 Dify 外部标识。
    pub external: DifyExternalIds,
}

/// Dify 工作流节点类型。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DifyNodeType {
    /// HTTP 工具节点。
    Http,
    /// MCP 工具节点。
    Mcp,
    /// 未纳入适配契约的节点类型。
    Unsupported,
}

/// Dify 工作流节点执行状态。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DifyNodeStatus {
    /// 节点成功完成。
    Succeeded,
    /// 节点执行失败。
    Failed,
}

/// Dify 工作流节点输入。
#[derive(Clone, Debug, PartialEq)]
pub struct DifyWorkflowNode {
    /// Dify 节点标识。
    pub node_id: String,
    /// 节点类型。
    pub node_type: DifyNodeType,
    /// 节点状态。
    pub status: DifyNodeStatus,
    /// 成功节点的结构化输出。
    pub output: Option<serde_json::Value>,
    /// 失败节点的错误文本。
    pub error: Option<String>,
}

/// Dify 映射错误。
#[derive(Debug, Error)]
pub enum DifyMappingError {
    /// 必填外部标识为空。
    #[error("Dify {field} 不能为空")]
    EmptyField {
        /// 字段名。
        field: &'static str,
    },
    /// 结构化 Packet 缺失或无法解析。
    #[error("Dify 节点 {node_id} 未提供有效 MemoryPacket: {detail}")]
    InvalidPacket {
        /// 节点标识。
        node_id: String,
        /// 不包含密钥的失败原因。
        detail: String,
    },
    /// 工作流节点类型不在适配契约内。
    #[error("Dify 节点 {node_id} 类型不受支持")]
    UnsupportedNode {
        /// 节点标识。
        node_id: String,
    },
    /// 失败节点必须被拒绝。
    #[error("Dify 节点 {node_id} 执行失败: {detail}")]
    FailedNode {
        /// 节点标识。
        node_id: String,
        /// 来源错误文本。
        detail: String,
    },
}

pub(crate) fn map_scope(
    config: &DifyConfig,
    invocation: &DifyInvocation,
) -> Result<DifyScope, DifyMappingError> {
    validate_invocation(invocation)?;
    let user_id = stable_id::<UserId>(&config.source_version, "user", &invocation.user_id);
    let session_id = stable_id::<SessionId>(
        &config.source_version,
        "conversation",
        &invocation.conversation_id,
    );
    let project_id = stable_id::<ProjectId>(&config.source_version, "app", &invocation.app_id);
    let agent_id =
        stable_id::<AgentId>(&config.source_version, "workflow", &invocation.workflow_id);
    let trace_id = stable_id::<TraceId>(&config.source_version, "trace", &invocation.trace_id);
    let mut scope = MemoryScope::new(config.namespace_id);
    scope.user_id = Some(user_id);
    scope.project_id = Some(project_id);
    scope.agent_id = Some(agent_id);
    Ok(DifyScope {
        scope,
        session_id,
        trace_id,
        external: DifyExternalIds {
            user_id: invocation.user_id.clone(),
            conversation_id: invocation.conversation_id.clone(),
            app_id: invocation.app_id.clone(),
            workflow_id: invocation.workflow_id.clone(),
            trace_id: invocation.trace_id.clone(),
        },
    })
}

pub(crate) fn map_workflow_node(node: DifyWorkflowNode) -> Result<MemoryPacket, DifyMappingError> {
    if node.node_id.trim().is_empty() {
        return Err(DifyMappingError::EmptyField { field: "node_id" });
    }
    if node.node_type == DifyNodeType::Unsupported {
        return Err(DifyMappingError::UnsupportedNode {
            node_id: node.node_id,
        });
    }
    match node.status {
        DifyNodeStatus::Failed => Err(DifyMappingError::FailedNode {
            node_id: node.node_id,
            detail: node.error.unwrap_or_else(|| "未提供错误文本".to_owned()),
        }),
        DifyNodeStatus::Succeeded => {
            let output = node.output.ok_or_else(|| DifyMappingError::InvalidPacket {
                node_id: node.node_id.clone(),
                detail: "成功节点缺少 output".to_owned(),
            })?;
            serde_json::from_value(output).map_err(|error| DifyMappingError::InvalidPacket {
                node_id: node.node_id,
                detail: error.to_string(),
            })
        }
    }
}

fn validate_invocation(invocation: &DifyInvocation) -> Result<(), DifyMappingError> {
    for (field, value) in [
        ("user_id", &invocation.user_id),
        ("conversation_id", &invocation.conversation_id),
        ("app_id", &invocation.app_id),
        ("workflow_id", &invocation.workflow_id),
        ("trace_id", &invocation.trace_id),
    ] {
        if value.trim().is_empty() {
            return Err(DifyMappingError::EmptyField { field });
        }
    }
    Ok(())
}

fn stable_id<T>(source_version: &str, kind: &str, value: &str) -> T
where
    T: FromStr,
    T::Err: std::fmt::Debug,
{
    let mut digest = Sha256::new();
    digest.update(source_version.as_bytes());
    digest.update([0]);
    digest.update(kind.as_bytes());
    digest.update([0]);
    digest.update(value.as_bytes());
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest.finalize()[..16]);
    // 以 RFC 4122 版本位生成可跨语言传输的稳定 UUID。
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    uuid::Uuid::from_bytes(bytes)
        .to_string()
        .parse()
        .expect("稳定 UUID 必须可解析为目标类型")
}
