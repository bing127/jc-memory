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
    /// 可选的 gRPC TLS 证书和私钥文件。
    pub tls: Option<TlsFiles>,
    /// Embedded Memory 数据目录。
    pub data_dir: PathBuf,
    /// 服务端 API Key。
    pub api_key: String,
    /// API Key 绑定的 Namespace。
    pub namespace_id: NamespaceId,
    /// 最大并发请求数。
    pub max_in_flight: usize,
    /// 每个 API Key 每秒允许的最大请求数。
    pub max_requests_per_second: u64,
}

impl ServerConfig {
    /// 从环境变量读取配置。
    ///
    /// 使用 `JC_MEMORY_BIND`、`JC_MEMORY_GRPC_BIND`、`JC_MEMORY_TLS_CERT_FILE`、
    /// `JC_MEMORY_TLS_KEY_FILE`、`JC_MEMORY_DATA_DIR`、`JC_MEMORY_API_KEY`、
    /// `JC_MEMORY_NAMESPACE_ID`、`JC_MEMORY_MAX_IN_FLIGHT` 和
    /// `JC_MEMORY_MAX_REQUESTS_PER_SECOND`。
    ///
    /// # Errors
    ///
    /// 当必填环境变量缺失，或地址、Namespace UUID、并发上限无法解析时返回错误。
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(env_value)
    }

    fn from_lookup<F>(lookup: F) -> Result<Self, ConfigError>
    where
        F: Fn(&str) -> Option<String>,
    {
        let bind: SocketAddr = lookup("JC_MEMORY_BIND")
            .unwrap_or_else(|| "127.0.0.1:8787".into())
            .parse::<SocketAddr>()
            .map_err(|error| ConfigError::Invalid("JC_MEMORY_BIND", error.to_string()))?;
        let grpc_bind: SocketAddr = lookup("JC_MEMORY_GRPC_BIND")
            .unwrap_or_else(|| "127.0.0.1:8788".into())
            .parse::<SocketAddr>()
            .map_err(|error| ConfigError::Invalid("JC_MEMORY_GRPC_BIND", error.to_string()))?;
        let tls = match (
            lookup("JC_MEMORY_TLS_CERT_FILE"),
            lookup("JC_MEMORY_TLS_KEY_FILE"),
        ) {
            (None, None) => None,
            (Some(cert_file), Some(key_file)) => Some(TlsFiles {
                cert_file: cert_file.into(),
                key_file: key_file.into(),
            }),
            (Some(_), None) => {
                return Err(ConfigError::Missing("JC_MEMORY_TLS_KEY_FILE"));
            }
            (None, Some(_)) => {
                return Err(ConfigError::Missing("JC_MEMORY_TLS_CERT_FILE"));
            }
        };
        let data_dir = PathBuf::from(
            lookup("JC_MEMORY_DATA_DIR").unwrap_or_else(|| "./data/jc-memory".into()),
        );
        let api_key =
            lookup("JC_MEMORY_API_KEY").ok_or(ConfigError::Missing("JC_MEMORY_API_KEY"))?;
        let namespace_id = lookup("JC_MEMORY_NAMESPACE_ID")
            .ok_or(ConfigError::Missing("JC_MEMORY_NAMESPACE_ID"))?
            .parse::<NamespaceId>()
            .map_err(|error| ConfigError::Invalid("JC_MEMORY_NAMESPACE_ID", error.to_string()))?;
        let max_in_flight = lookup("JC_MEMORY_MAX_IN_FLIGHT")
            .unwrap_or_else(|| "64".into())
            .parse::<usize>()
            .map_err(|error| ConfigError::Invalid("JC_MEMORY_MAX_IN_FLIGHT", error.to_string()))?;
        if max_in_flight == 0 {
            return Err(ConfigError::Invalid(
                "JC_MEMORY_MAX_IN_FLIGHT",
                "value must be greater than zero".into(),
            ));
        }
        let max_requests_per_second = lookup("JC_MEMORY_MAX_REQUESTS_PER_SECOND")
            .unwrap_or_else(|| "128".into())
            .parse::<u64>()
            .map_err(|error| {
                ConfigError::Invalid("JC_MEMORY_MAX_REQUESTS_PER_SECOND", error.to_string())
            })?;
        if max_requests_per_second == 0 {
            return Err(ConfigError::Invalid(
                "JC_MEMORY_MAX_REQUESTS_PER_SECOND",
                "value must be greater than zero".into(),
            ));
        }
        Ok(Self {
            bind,
            grpc_bind,
            tls,
            data_dir,
            api_key,
            namespace_id,
            max_in_flight,
            max_requests_per_second,
        })
    }
}

/// 从文件加载的 gRPC TLS 证书和私钥路径。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TlsFiles {
    /// PEM 编码证书链文件。
    pub cert_file: PathBuf,
    /// PEM 编码私钥文件。
    pub key_file: PathBuf,
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn values(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
        entries
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect()
    }

    #[test]
    fn tls_paths_are_loaded_as_a_pair_without_reading_private_material() {
        let settings = values(&[
            ("JC_MEMORY_API_KEY", "test-key"),
            (
                "JC_MEMORY_NAMESPACE_ID",
                "018f4c8f-5f7b-7d31-9a6c-b7a0f4f37001",
            ),
            ("JC_MEMORY_TLS_CERT_FILE", "/etc/jc-memory/server.crt"),
            ("JC_MEMORY_TLS_KEY_FILE", "/etc/jc-memory/server.key"),
        ]);

        let config = ServerConfig::from_lookup(|name| settings.get(name).cloned()).unwrap();

        assert_eq!(
            config.tls,
            Some(TlsFiles {
                cert_file: "/etc/jc-memory/server.crt".into(),
                key_file: "/etc/jc-memory/server.key".into(),
            })
        );
    }

    #[test]
    fn tls_configuration_requires_both_files() {
        let settings = values(&[
            ("JC_MEMORY_API_KEY", "test-key"),
            (
                "JC_MEMORY_NAMESPACE_ID",
                "018f4c8f-5f7b-7d31-9a6c-b7a0f4f37001",
            ),
            ("JC_MEMORY_TLS_CERT_FILE", "/etc/jc-memory/server.crt"),
        ]);

        let error = ServerConfig::from_lookup(|name| settings.get(name).cloned()).unwrap_err();

        assert_eq!(error, ConfigError::Missing("JC_MEMORY_TLS_KEY_FILE"));
    }

    #[test]
    fn missing_tls_files_keep_local_grpc_insecure_mode() {
        let settings = values(&[
            ("JC_MEMORY_API_KEY", "test-key"),
            (
                "JC_MEMORY_NAMESPACE_ID",
                "018f4c8f-5f7b-7d31-9a6c-b7a0f4f37001",
            ),
        ]);

        let config = ServerConfig::from_lookup(|name| settings.get(name).cloned()).unwrap();

        assert_eq!(config.tls, None);
    }

    #[test]
    fn request_rate_limit_is_positive_and_configurable() {
        let settings = values(&[
            ("JC_MEMORY_API_KEY", "test-key"),
            (
                "JC_MEMORY_NAMESPACE_ID",
                "018f4c8f-5f7b-7d31-9a6c-b7a0f4f37001",
            ),
            ("JC_MEMORY_MAX_REQUESTS_PER_SECOND", "7"),
        ]);

        let config = ServerConfig::from_lookup(|name| settings.get(name).cloned()).unwrap();

        assert_eq!(config.max_requests_per_second, 7);
    }

    #[test]
    fn zero_request_rate_limit_is_rejected() {
        let settings = values(&[
            ("JC_MEMORY_API_KEY", "test-key"),
            (
                "JC_MEMORY_NAMESPACE_ID",
                "018f4c8f-5f7b-7d31-9a6c-b7a0f4f37001",
            ),
            ("JC_MEMORY_MAX_REQUESTS_PER_SECOND", "0"),
        ]);

        let error = ServerConfig::from_lookup(|name| settings.get(name).cloned()).unwrap_err();

        assert_eq!(
            error,
            ConfigError::Invalid(
                "JC_MEMORY_MAX_REQUESTS_PER_SECOND",
                "value must be greater than zero".into(),
            )
        );
    }
}
