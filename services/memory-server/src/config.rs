//! Server 启动配置。

use std::{
    fmt::Display,
    net::SocketAddr,
    path::{Path, PathBuf},
    str::FromStr,
};

use jc_memory_protocol::NamespaceId;
use serde::Deserialize;

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
    /// 单个请求允许执行的最长时间（毫秒）。
    pub request_timeout_ms: u64,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct PartialConfig {
    /// YAML 中的 HTTP 监听地址。
    bind: Option<String>,
    /// YAML 中的 gRPC 监听地址。
    grpc_bind: Option<String>,
    /// YAML 中的 Embedded 数据目录。
    data_dir: Option<String>,
    /// YAML 中的 Namespace UUID。
    namespace_id: Option<String>,
    /// YAML 中的最大并发请求数。
    max_in_flight: Option<usize>,
    /// YAML 中的每秒请求数上限。
    max_requests_per_second: Option<u64>,
    /// YAML 中的请求超时毫秒数。
    request_timeout_ms: Option<u64>,
    /// YAML 中的 TLS 文件路径。
    tls: Option<PartialTls>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct PartialTls {
    /// PEM 编码证书链文件路径。
    cert_file: Option<PathBuf>,
    /// PEM 编码私钥文件路径。
    key_file: Option<PathBuf>,
}

impl ServerConfig {
    /// 从环境变量读取配置。
    ///
    /// 使用 `JC_MEMORY_BIND`、`JC_MEMORY_GRPC_BIND`、`JC_MEMORY_TLS_CERT_FILE`、
    /// `JC_MEMORY_TLS_KEY_FILE`、`JC_MEMORY_DATA_DIR`、`JC_MEMORY_API_KEY`、
    /// `JC_MEMORY_NAMESPACE_ID`、`JC_MEMORY_MAX_IN_FLIGHT`、
    /// `JC_MEMORY_MAX_REQUESTS_PER_SECOND` 和 `JC_MEMORY_REQUEST_TIMEOUT_MS`。
    ///
    /// # Errors
    ///
    /// 当必填环境变量缺失，或地址、Namespace UUID、并发上限无法解析时返回错误。
    pub fn from_env() -> Result<Self, ConfigError> {
        let path = env_value("JC_MEMORY_CONFIG_FILE").map(PathBuf::from);
        Self::from_sources(path.as_deref(), env_value)
    }

    /// 从可选 YAML 文件和环境变量查找器合并配置。
    ///
    /// 环境变量优先于 YAML；空白环境变量视为未设置。
    ///
    /// # Errors
    ///
    /// 文件无法读取、YAML 无法解析、必填项缺失或字段无法通过校验时返回错误。
    pub fn from_sources<F>(path: Option<&Path>, lookup: F) -> Result<Self, ConfigError>
    where
        F: Fn(&str) -> Option<String>,
    {
        let partial = match path {
            Some(path) => {
                let input = std::fs::read_to_string(path).map_err(|error| ConfigError::File {
                    path: path.to_path_buf(),
                    message: error.to_string(),
                })?;
                Self::from_yaml_str(&input)?
            }
            None => PartialConfig::default(),
        };
        Self::from_partial_lookup(partial, lookup)
    }

    fn from_yaml_str(input: &str) -> Result<PartialConfig, ConfigError> {
        yaml_serde::from_str(input)
            .map_err(|error| ConfigError::Invalid("JC_MEMORY_CONFIG_FILE", error.to_string()))
    }

    #[cfg(test)]
    fn from_lookup<F>(lookup: F) -> Result<Self, ConfigError>
    where
        F: Fn(&str) -> Option<String>,
    {
        Self::from_partial_lookup(PartialConfig::default(), lookup)
    }

    fn from_partial_lookup<F>(partial: PartialConfig, lookup: F) -> Result<Self, ConfigError>
    where
        F: Fn(&str) -> Option<String>,
    {
        let bind: SocketAddr = string_value(&lookup, "JC_MEMORY_BIND")
            .or(partial.bind)
            .unwrap_or_else(|| "127.0.0.1:8787".into())
            .parse::<SocketAddr>()
            .map_err(|error| ConfigError::Invalid("JC_MEMORY_BIND", error.to_string()))?;
        let grpc_bind: SocketAddr = string_value(&lookup, "JC_MEMORY_GRPC_BIND")
            .or(partial.grpc_bind)
            .unwrap_or_else(|| "127.0.0.1:8788".into())
            .parse::<SocketAddr>()
            .map_err(|error| ConfigError::Invalid("JC_MEMORY_GRPC_BIND", error.to_string()))?;
        let partial_tls = partial.tls.unwrap_or_default();
        let tls = match (
            string_value(&lookup, "JC_MEMORY_TLS_CERT_FILE")
                .map(PathBuf::from)
                .or(partial_tls.cert_file),
            string_value(&lookup, "JC_MEMORY_TLS_KEY_FILE")
                .map(PathBuf::from)
                .or(partial_tls.key_file),
        ) {
            (None, None) => None,
            (Some(cert_file), Some(key_file)) => Some(TlsFiles {
                cert_file,
                key_file,
            }),
            (Some(_), None) => {
                return Err(ConfigError::Missing("JC_MEMORY_TLS_KEY_FILE"));
            }
            (None, Some(_)) => {
                return Err(ConfigError::Missing("JC_MEMORY_TLS_CERT_FILE"));
            }
        };
        let data_dir = PathBuf::from(
            string_value(&lookup, "JC_MEMORY_DATA_DIR")
                .or(partial.data_dir)
                .unwrap_or_else(|| "./data/jc-memory".into()),
        );
        let api_key = string_value(&lookup, "JC_MEMORY_API_KEY")
            .ok_or(ConfigError::Missing("JC_MEMORY_API_KEY"))?;
        let namespace_id = string_value(&lookup, "JC_MEMORY_NAMESPACE_ID")
            .or(partial.namespace_id)
            .ok_or(ConfigError::Missing("JC_MEMORY_NAMESPACE_ID"))?
            .parse::<NamespaceId>()
            .map_err(|error| ConfigError::Invalid("JC_MEMORY_NAMESPACE_ID", error.to_string()))?;
        let max_in_flight = parsed_value(&lookup, "JC_MEMORY_MAX_IN_FLIGHT")?
            .or(partial.max_in_flight)
            .unwrap_or(64);
        if max_in_flight == 0 {
            return Err(ConfigError::Invalid(
                "JC_MEMORY_MAX_IN_FLIGHT",
                "value must be greater than zero".into(),
            ));
        }
        let max_requests_per_second = parsed_value(&lookup, "JC_MEMORY_MAX_REQUESTS_PER_SECOND")?
            .or(partial.max_requests_per_second)
            .unwrap_or(128);
        if max_requests_per_second == 0 {
            return Err(ConfigError::Invalid(
                "JC_MEMORY_MAX_REQUESTS_PER_SECOND",
                "value must be greater than zero".into(),
            ));
        }
        let request_timeout_ms = parsed_value(&lookup, "JC_MEMORY_REQUEST_TIMEOUT_MS")?
            .or(partial.request_timeout_ms)
            .unwrap_or(30_000);
        if request_timeout_ms == 0 {
            return Err(ConfigError::Invalid(
                "JC_MEMORY_REQUEST_TIMEOUT_MS",
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
            request_timeout_ms,
        })
    }
}

fn string_value<F>(lookup: &F, name: &str) -> Option<String>
where
    F: Fn(&str) -> Option<String>,
{
    lookup(name).filter(|value| !value.trim().is_empty())
}

fn parsed_value<T, F>(lookup: &F, name: &'static str) -> Result<Option<T>, ConfigError>
where
    T: FromStr,
    T::Err: Display,
    F: Fn(&str) -> Option<String>,
{
    string_value(lookup, name)
        .map(|value| {
            value
                .parse::<T>()
                .map_err(|error| ConfigError::Invalid(name, error.to_string()))
        })
        .transpose()
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
    /// 配置文件无法读取。
    #[error("failed to read config file {path}: {message}")]
    File {
        /// 配置文件路径。
        path: PathBuf,
        /// 文件系统错误信息。
        message: String,
    },
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

    #[test]
    fn zero_request_timeout_is_rejected() {
        let settings = values(&[
            ("JC_MEMORY_API_KEY", "test-key"),
            (
                "JC_MEMORY_NAMESPACE_ID",
                "018f4c8f-5f7b-7d31-9a6c-b7a0f4f37001",
            ),
            ("JC_MEMORY_REQUEST_TIMEOUT_MS", "0"),
        ]);

        let error = ServerConfig::from_lookup(|name| settings.get(name).cloned()).unwrap_err();

        assert_eq!(
            error,
            ConfigError::Invalid(
                "JC_MEMORY_REQUEST_TIMEOUT_MS",
                "value must be greater than zero".into(),
            )
        );
    }

    #[test]
    fn yaml_config_parses_supported_fields() {
        let partial = ServerConfig::from_yaml_str(
            "bind: 0.0.0.0:8787\ngrpc_bind: 0.0.0.0:8788\ndata_dir: /var/lib/jc-memory\nmax_in_flight: 32\nmax_requests_per_second: 256\nrequest_timeout_ms: 1500\ntls:\n  cert_file: /etc/jc-memory/server.crt\n  key_file: /etc/jc-memory/server.key\n",
        )
        .unwrap();

        assert_eq!(partial.bind.as_deref(), Some("0.0.0.0:8787"));
        assert_eq!(partial.grpc_bind.as_deref(), Some("0.0.0.0:8788"));
        assert_eq!(partial.data_dir.as_deref(), Some("/var/lib/jc-memory"));
        assert_eq!(partial.max_in_flight, Some(32));
        assert_eq!(partial.max_requests_per_second, Some(256));
        assert_eq!(partial.request_timeout_ms, Some(1500));
        assert_eq!(
            partial.tls,
            Some(PartialTls {
                cert_file: Some("/etc/jc-memory/server.crt".into()),
                key_file: Some("/etc/jc-memory/server.key".into()),
            })
        );
    }

    #[test]
    fn yaml_config_rejects_unknown_fields() {
        let error = ServerConfig::from_yaml_str("unknown_field: true").unwrap_err();

        assert!(error.to_string().contains("unknown field"));
    }

    #[test]
    fn yaml_config_reports_invalid_field_types() {
        let error = ServerConfig::from_yaml_str("max_in_flight: many").unwrap_err();

        assert!(error.to_string().contains("max_in_flight"));
    }

    #[test]
    fn merge_environment_overrides_yaml_and_blank_values_do_not() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("server.yaml");
        std::fs::write(
            &path,
            "bind: 0.0.0.0:9000\nnamespace_id: 018f4c8f-5f7b-7d31-9a6c-b7a0f4f37001\nmax_in_flight: 12\nmax_requests_per_second: 99\n",
        )
        .unwrap();
        let settings = values(&[
            ("JC_MEMORY_API_KEY", "test-key"),
            ("JC_MEMORY_BIND", "   "),
            ("JC_MEMORY_MAX_IN_FLIGHT", "48"),
            ("JC_MEMORY_REQUEST_TIMEOUT_MS", "17"),
        ]);

        let config =
            ServerConfig::from_sources(Some(&path), |name| settings.get(name).cloned()).unwrap();

        assert_eq!(config.bind, "0.0.0.0:9000".parse().unwrap());
        assert_eq!(config.grpc_bind, "127.0.0.1:8788".parse().unwrap());
        assert_eq!(config.max_in_flight, 48);
        assert_eq!(config.max_requests_per_second, 99);
        assert_eq!(config.request_timeout_ms, 17);
        assert_eq!(config.api_key, "test-key");
    }

    #[test]
    fn merge_reports_missing_config_file_without_exposing_secrets() {
        let path = PathBuf::from("/missing/jc-memory/server.yaml");
        let settings = values(&[("JC_MEMORY_API_KEY", "do-not-print-this-key")]);

        let error = ServerConfig::from_sources(Some(&path), |name| settings.get(name).cloned())
            .unwrap_err();

        assert!(matches!(error, ConfigError::File { .. }));
        assert!(error.to_string().contains("/missing/jc-memory/server.yaml"));
        assert!(!error.to_string().contains("do-not-print-this-key"));
    }

    #[test]
    fn docker_yaml_example_stays_compatible_with_the_server_schema() {
        let partial = ServerConfig::from_yaml_str(include_str!(
            "../../../deploy/docker/config/server.yaml.example"
        ))
        .unwrap();

        assert_eq!(partial.bind.as_deref(), Some("0.0.0.0:8787"));
        assert_eq!(partial.grpc_bind.as_deref(), Some("0.0.0.0:8788"));
        assert_eq!(partial.data_dir.as_deref(), Some("/var/lib/jc-memory"));
    }
}
