//! Server 启动配置。

use std::{net::SocketAddr, path::PathBuf};

use jc_memory_protocol::NamespaceId;

/// 可从环境变量构造的本地 Server 配置。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServerConfig {
    /// HTTP 监听地址。
    pub bind: SocketAddr,
    /// gRPC 监听地址。
    pub grpc_bind: SocketAddr,
    /// Embedded Memory 数据目录。
    pub data_dir: PathBuf,
    /// 服务端 API Key。
    pub api_key: String,
    /// API Key 绑定的 Namespace。
    pub namespace_id: NamespaceId,
    /// 最大并发请求数。
    pub max_in_flight: usize,
}

impl ServerConfig {
    /// 从环境变量读取配置。
    ///
    /// 使用 `JC_MEMORY_BIND`、`JC_MEMORY_GRPC_BIND`、`JC_MEMORY_DATA_DIR`、
    /// `JC_MEMORY_API_KEY`、`JC_MEMORY_NAMESPACE_ID` 和 `JC_MEMORY_MAX_IN_FLIGHT`。
    ///
    /// # Errors
    ///
    /// 当必填环境变量缺失，或地址、Namespace UUID、并发上限无法解析时返回错误。
    pub fn from_env() -> Result<Self, ConfigError> {
        let bind: SocketAddr = env_value("JC_MEMORY_BIND")
            .unwrap_or_else(|| "127.0.0.1:8787".into())
            .parse::<SocketAddr>()
            .map_err(|error| ConfigError::Invalid("JC_MEMORY_BIND", error.to_string()))?;
        let grpc_bind: SocketAddr = env_value("JC_MEMORY_GRPC_BIND")
            .unwrap_or_else(|| "127.0.0.1:8788".into())
            .parse::<SocketAddr>()
            .map_err(|error| ConfigError::Invalid("JC_MEMORY_GRPC_BIND", error.to_string()))?;
        let data_dir = PathBuf::from(
            env_value("JC_MEMORY_DATA_DIR").unwrap_or_else(|| "./data/jc-memory".into()),
        );
        let api_key =
            env_value("JC_MEMORY_API_KEY").ok_or(ConfigError::Missing("JC_MEMORY_API_KEY"))?;
        let namespace_id = env_value("JC_MEMORY_NAMESPACE_ID")
            .ok_or(ConfigError::Missing("JC_MEMORY_NAMESPACE_ID"))?
            .parse::<NamespaceId>()
            .map_err(|error| ConfigError::Invalid("JC_MEMORY_NAMESPACE_ID", error.to_string()))?;
        let max_in_flight = env_value("JC_MEMORY_MAX_IN_FLIGHT")
            .unwrap_or_else(|| "64".into())
            .parse::<usize>()
            .map_err(|error| ConfigError::Invalid("JC_MEMORY_MAX_IN_FLIGHT", error.to_string()))?;
        if max_in_flight == 0 {
            return Err(ConfigError::Invalid(
                "JC_MEMORY_MAX_IN_FLIGHT",
                "value must be greater than zero".into(),
            ));
        }
        Ok(Self {
            bind,
            grpc_bind,
            data_dir,
            api_key,
            namespace_id,
            max_in_flight,
        })
    }
}

fn env_value(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

/// Server 配置错误。
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ConfigError {
    /// 必填配置缺失。
    #[error("missing environment variable {0}")]
    Missing(&'static str),
    /// 配置值格式不正确。
    #[error("invalid environment variable {0}: {1}")]
    Invalid(&'static str, String),
}
