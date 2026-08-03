//! Server 启动配置。

use std::{
    fmt,
    fmt::Display,
    net::SocketAddr,
    path::{Path, PathBuf},
    str::FromStr,
};

use jc_memory_core::RetrievalConfig;
use jc_memory_protocol::{NamespaceId, RecallLimits};
use serde::Deserialize;

const API_KEY_PREFIX: &str = "sk-jcmemory-";
const API_KEY_SECRET_LENGTH: usize = 34;

/// 可从环境变量构造的本地 Server 配置。
#[derive(Clone, PartialEq)]
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
    /// 零外部模型混合检索配置。
    pub retrieval: RetrievalConfig,
    /// SQLite 连接池与忙等待策略。
    pub sqlite: SqliteRuntimeConfig,
    /// 可选的外部 Quarantine 规则文件。
    pub security_rules_file: Option<PathBuf>,
    /// Runtime Provider 重试策略。
    pub runtime_provider_tuning: RuntimeProviderTuning,
    /// Runtime Recall 有界预算。
    pub runtime_recall: RuntimeRecallConfig,
    /// 可选的 Runtime `OpenAI` Compatible Provider。
    pub runtime_provider: Option<RuntimeProviderConfig>,
}

/// SQLite 运行时连接池配置。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SqliteRuntimeConfig {
    /// 最大连接数。
    pub max_connections: u32,
    /// SQLite 忙等待毫秒数。
    pub busy_timeout_ms: u64,
}

impl Default for SqliteRuntimeConfig {
    fn default() -> Self {
        Self {
            max_connections: 4,
            busy_timeout_ms: 5_000,
        }
    }
}

/// Runtime Provider 重试与退避配置。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeProviderTuning {
    /// 除首次请求外允许的重试次数。
    pub max_retries: u32,
    /// 两次尝试之间的固定退避毫秒数。
    pub retry_backoff_ms: u64,
}

impl Default for RuntimeProviderTuning {
    fn default() -> Self {
        Self {
            max_retries: 1,
            retry_backoff_ms: 100,
        }
    }
}

/// Runtime Recall 有界预算配置。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeRecallConfig {
    /// 初始 Seed 数量上限。
    pub max_seeds: usize,
    /// 每个节点的邻居扩展上限。
    pub max_neighbors: usize,
    /// 单个 Seed 的最大遍历深度。
    pub max_hops: usize,
    /// 单次 Recall 的节点总上限。
    pub max_nodes: usize,
    /// 单次 Recall 的超时毫秒数。
    pub timeout_ms: u64,
}

impl Default for RuntimeRecallConfig {
    fn default() -> Self {
        Self {
            max_seeds: 8,
            max_neighbors: 8,
            max_hops: 3,
            max_nodes: 64,
            timeout_ms: 2_000,
        }
    }
}

impl RuntimeRecallConfig {
    /// 转换为 Core 使用的有界 Recall 限制。
    #[must_use]
    pub const fn limits(&self) -> RecallLimits {
        RecallLimits {
            max_seeds: self.max_seeds,
            max_neighbors: self.max_neighbors,
            max_hops: self.max_hops,
            max_nodes: self.max_nodes,
            timeout_ms: self.timeout_ms,
        }
    }
}

/// Runtime Provider 启动配置；认证值不会出现在 Debug 输出。
#[derive(Clone, Eq, PartialEq)]
pub struct RuntimeProviderConfig {
    /// Chat Completions 兼容地址。
    pub endpoint: String,
    /// 请求使用的模型标识。
    pub model: String,
    /// 可选 Bearer Token，只从环境变量读取。
    pub api_key: Option<String>,
}

impl fmt::Debug for RuntimeProviderConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RuntimeProviderConfig")
            .field("endpoint", &self.endpoint)
            .field("model", &self.model)
            .field("api_key", &self.api_key.as_ref().map(|_| "[REDACTED]"))
            .finish()
    }
}

impl fmt::Debug for ServerConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ServerConfig")
            .field("bind", &self.bind)
            .field("grpc_bind", &self.grpc_bind)
            .field("tls", &self.tls)
            .field("data_dir", &self.data_dir)
            .field("api_key", &"[REDACTED]")
            .field("namespace_id", &self.namespace_id)
            .field("max_in_flight", &self.max_in_flight)
            .field("max_requests_per_second", &self.max_requests_per_second)
            .field("request_timeout_ms", &self.request_timeout_ms)
            .field("retrieval", &RetrievalDebug(&self.retrieval))
            .field("sqlite", &self.sqlite)
            .field(
                "security_rules_file",
                &self.security_rules_file.as_ref().map(|_| "[CONFIGURED]"),
            )
            .field("runtime_provider_tuning", &self.runtime_provider_tuning)
            .field("runtime_recall", &self.runtime_recall)
            .field("runtime_provider", &self.runtime_provider)
            .finish()
    }
}

struct RetrievalDebug<'config>(&'config RetrievalConfig);

impl fmt::Debug for RetrievalDebug<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RetrievalConfig")
            .field(
                "aliases_file",
                &self.0.aliases_file.as_ref().map(|_| "[CONFIGURED]"),
            )
            .field("lexical_limit_factor", &self.0.lexical_limit_factor)
            .field("trigram_limit_factor", &self.0.trigram_limit_factor)
            .field("entity_limit_factor", &self.0.entity_limit_factor)
            .field("recent_limit", &self.0.recent_limit)
            .field("rrf_k", &self.0.rrf_k)
            .field("max_expanded_terms", &self.0.max_expanded_terms)
            .field("min_trigram_chars", &self.0.min_trigram_chars)
            .field("min_candidate_score", &self.0.min_candidate_score)
            .finish()
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
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
    /// YAML 中的零模型检索配置。
    retrieval: Option<PartialRetrieval>,
    /// YAML 中的 SQLite 运行配置。
    sqlite: Option<PartialSqlite>,
    /// YAML 中的安全规则配置。
    security: Option<PartialSecurity>,
    /// YAML 中的 Runtime 运行配置。
    runtime: Option<PartialRuntime>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct PartialSqlite {
    max_connections: Option<u32>,
    busy_timeout_ms: Option<u64>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct PartialSecurity {
    rules_file: Option<PathBuf>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct PartialRuntime {
    provider: Option<PartialRuntimeProvider>,
    recall: Option<PartialRuntimeRecall>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct PartialRuntimeProvider {
    max_retries: Option<u32>,
    retry_backoff_ms: Option<u64>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct PartialRuntimeRecall {
    max_seeds: Option<usize>,
    max_neighbors: Option<usize>,
    max_hops: Option<usize>,
    max_nodes: Option<usize>,
    timeout_ms: Option<u64>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct PartialTls {
    /// PEM 编码证书链文件路径。
    cert_file: Option<PathBuf>,
    /// PEM 编码私钥文件路径。
    key_file: Option<PathBuf>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct PartialRetrieval {
    aliases_file: Option<PathBuf>,
    lexical_limit_factor: Option<usize>,
    trigram_limit_factor: Option<usize>,
    entity_limit_factor: Option<usize>,
    recent_limit: Option<usize>,
    rrf_k: Option<u32>,
    max_expanded_terms: Option<usize>,
    min_trigram_chars: Option<usize>,
    min_candidate_score: Option<f64>,
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
        let tls = tls_files(partial.tls.unwrap_or_default(), &lookup)?;
        let data_dir = PathBuf::from(
            string_value(&lookup, "JC_MEMORY_DATA_DIR")
                .or(partial.data_dir)
                .unwrap_or_else(|| "./data/jc-memory".into()),
        );
        let api_key = api_key_value(&lookup)?;
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
        let retrieval = retrieval_config(partial.retrieval.unwrap_or_default(), &lookup)?;
        let sqlite = sqlite_config(partial.sqlite.unwrap_or_default(), &lookup)?;
        let security_rules_file = string_value(&lookup, "JC_MEMORY_SECURITY_RULES_FILE")
            .map(PathBuf::from)
            .or_else(|| partial.security.and_then(|security| security.rules_file));
        let (runtime_provider_tuning, runtime_recall) =
            runtime_operational_config(partial.runtime.unwrap_or_default(), &lookup)?;
        let runtime_provider = runtime_provider_config(&lookup)?;
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
            retrieval,
            sqlite,
            security_rules_file,
            runtime_provider_tuning,
            runtime_recall,
            runtime_provider,
        })
    }
}

fn sqlite_config<F>(partial: PartialSqlite, lookup: &F) -> Result<SqliteRuntimeConfig, ConfigError>
where
    F: Fn(&str) -> Option<String>,
{
    let defaults = SqliteRuntimeConfig::default();
    let config = SqliteRuntimeConfig {
        max_connections: parsed_value(&lookup, "JC_MEMORY_SQLITE_MAX_CONNECTIONS")?
            .or(partial.max_connections)
            .unwrap_or(defaults.max_connections),
        busy_timeout_ms: parsed_value(&lookup, "JC_MEMORY_SQLITE_BUSY_TIMEOUT_MS")?
            .or(partial.busy_timeout_ms)
            .unwrap_or(defaults.busy_timeout_ms),
    };
    if config.max_connections == 0 || config.max_connections > 64 {
        return Err(ConfigError::Invalid(
            "JC_MEMORY_SQLITE_MAX_CONNECTIONS",
            "value must be in 1..=64".into(),
        ));
    }
    if config.busy_timeout_ms == 0 || config.busy_timeout_ms > 60_000 {
        return Err(ConfigError::Invalid(
            "JC_MEMORY_SQLITE_BUSY_TIMEOUT_MS",
            "value must be in 1..=60000".into(),
        ));
    }
    Ok(config)
}

fn runtime_operational_config<F>(
    partial: PartialRuntime,
    lookup: &F,
) -> Result<(RuntimeProviderTuning, RuntimeRecallConfig), ConfigError>
where
    F: Fn(&str) -> Option<String>,
{
    let provider_defaults = RuntimeProviderTuning::default();
    let provider_partial = partial.provider.unwrap_or_default();
    let provider = RuntimeProviderTuning {
        max_retries: parsed_value(&lookup, "JC_MEMORY_RUNTIME_PROVIDER_MAX_RETRIES")?
            .or(provider_partial.max_retries)
            .unwrap_or(provider_defaults.max_retries),
        retry_backoff_ms: parsed_value(&lookup, "JC_MEMORY_RUNTIME_PROVIDER_RETRY_BACKOFF_MS")?
            .or(provider_partial.retry_backoff_ms)
            .unwrap_or(provider_defaults.retry_backoff_ms),
    };
    if provider.max_retries > 10 {
        return Err(ConfigError::Invalid(
            "JC_MEMORY_RUNTIME_PROVIDER_MAX_RETRIES",
            "value must be in 0..=10".into(),
        ));
    }
    if provider.retry_backoff_ms > 60_000 {
        return Err(ConfigError::Invalid(
            "JC_MEMORY_RUNTIME_PROVIDER_RETRY_BACKOFF_MS",
            "value must be in 0..=60000".into(),
        ));
    }

    let recall_defaults = RuntimeRecallConfig::default();
    let recall_partial = partial.recall.unwrap_or_default();
    let recall = RuntimeRecallConfig {
        max_seeds: parsed_value(&lookup, "JC_MEMORY_RUNTIME_RECALL_MAX_SEEDS")?
            .or(recall_partial.max_seeds)
            .unwrap_or(recall_defaults.max_seeds),
        max_neighbors: parsed_value(&lookup, "JC_MEMORY_RUNTIME_RECALL_MAX_NEIGHBORS")?
            .or(recall_partial.max_neighbors)
            .unwrap_or(recall_defaults.max_neighbors),
        max_hops: parsed_value(&lookup, "JC_MEMORY_RUNTIME_RECALL_MAX_HOPS")?
            .or(recall_partial.max_hops)
            .unwrap_or(recall_defaults.max_hops),
        max_nodes: parsed_value(&lookup, "JC_MEMORY_RUNTIME_RECALL_MAX_NODES")?
            .or(recall_partial.max_nodes)
            .unwrap_or(recall_defaults.max_nodes),
        timeout_ms: parsed_value(&lookup, "JC_MEMORY_RUNTIME_RECALL_TIMEOUT_MS")?
            .or(recall_partial.timeout_ms)
            .unwrap_or(recall_defaults.timeout_ms),
    };
    validate_recall_limit("JC_MEMORY_RUNTIME_RECALL_MAX_SEEDS", recall.max_seeds, 64)?;
    validate_recall_limit(
        "JC_MEMORY_RUNTIME_RECALL_MAX_NEIGHBORS",
        recall.max_neighbors,
        128,
    )?;
    validate_recall_limit("JC_MEMORY_RUNTIME_RECALL_MAX_HOPS", recall.max_hops, 16)?;
    validate_recall_limit("JC_MEMORY_RUNTIME_RECALL_MAX_NODES", recall.max_nodes, 1024)?;
    if recall.timeout_ms == 0 || recall.timeout_ms > 60_000 {
        return Err(ConfigError::Invalid(
            "JC_MEMORY_RUNTIME_RECALL_TIMEOUT_MS",
            "value must be in 1..=60000".into(),
        ));
    }
    Ok((provider, recall))
}

fn validate_recall_limit(
    field: &'static str,
    value: usize,
    maximum: usize,
) -> Result<(), ConfigError> {
    if value == 0 || value > maximum {
        Err(ConfigError::Invalid(
            field,
            format!("value must be in 1..={maximum}"),
        ))
    } else {
        Ok(())
    }
}

fn tls_files<F>(partial: PartialTls, lookup: &F) -> Result<Option<TlsFiles>, ConfigError>
where
    F: Fn(&str) -> Option<String>,
{
    match (
        string_value(lookup, "JC_MEMORY_TLS_CERT_FILE")
            .map(PathBuf::from)
            .or(partial.cert_file),
        string_value(lookup, "JC_MEMORY_TLS_KEY_FILE")
            .map(PathBuf::from)
            .or(partial.key_file),
    ) {
        (None, None) => Ok(None),
        (Some(cert_file), Some(key_file)) => Ok(Some(TlsFiles {
            cert_file,
            key_file,
        })),
        (Some(_), None) => Err(ConfigError::Missing("JC_MEMORY_TLS_KEY_FILE")),
        (None, Some(_)) => Err(ConfigError::Missing("JC_MEMORY_TLS_CERT_FILE")),
    }
}

fn retrieval_config<F>(
    partial: PartialRetrieval,
    lookup: &F,
) -> Result<RetrievalConfig, ConfigError>
where
    F: Fn(&str) -> Option<String>,
{
    let defaults = RetrievalConfig::default();
    let retrieval = RetrievalConfig {
        aliases_file: string_value(lookup, "JC_MEMORY_RETRIEVAL_ALIASES_FILE")
            .map(PathBuf::from)
            .or(partial.aliases_file),
        lexical_limit_factor: partial
            .lexical_limit_factor
            .unwrap_or(defaults.lexical_limit_factor),
        trigram_limit_factor: partial
            .trigram_limit_factor
            .unwrap_or(defaults.trigram_limit_factor),
        entity_limit_factor: partial
            .entity_limit_factor
            .unwrap_or(defaults.entity_limit_factor),
        recent_limit: partial.recent_limit.unwrap_or(defaults.recent_limit),
        rrf_k: partial.rrf_k.unwrap_or(defaults.rrf_k),
        max_expanded_terms: partial
            .max_expanded_terms
            .unwrap_or(defaults.max_expanded_terms),
        min_trigram_chars: partial
            .min_trigram_chars
            .unwrap_or(defaults.min_trigram_chars),
        min_candidate_score: partial
            .min_candidate_score
            .unwrap_or(defaults.min_candidate_score),
    };
    retrieval
        .validate()
        .map_err(|error| ConfigError::Invalid("retrieval", error.to_string()))?;
    Ok(retrieval)
}

fn runtime_provider_config<F>(lookup: &F) -> Result<Option<RuntimeProviderConfig>, ConfigError>
where
    F: Fn(&str) -> Option<String>,
{
    match (
        string_value(lookup, "JC_MEMORY_RUNTIME_PROVIDER_ENDPOINT"),
        string_value(lookup, "JC_MEMORY_RUNTIME_PROVIDER_MODEL"),
    ) {
        (None, None) => Ok(None),
        (Some(endpoint), Some(model)) => Ok(Some(RuntimeProviderConfig {
            endpoint,
            model,
            api_key: string_value(lookup, "JC_MEMORY_RUNTIME_PROVIDER_API_KEY"),
        })),
        (Some(_), None) => Err(ConfigError::Missing("JC_MEMORY_RUNTIME_PROVIDER_MODEL")),
        (None, Some(_)) => Err(ConfigError::Missing("JC_MEMORY_RUNTIME_PROVIDER_ENDPOINT")),
    }
}

fn valid_api_key(api_key: &str) -> bool {
    api_key.strip_prefix(API_KEY_PREFIX).is_some_and(|secret| {
        secret.len() == API_KEY_SECRET_LENGTH
            && secret
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
    })
}

fn api_key_value<F>(lookup: &F) -> Result<String, ConfigError>
where
    F: Fn(&str) -> Option<String>,
{
    let api_key = string_value(lookup, "JC_MEMORY_API_KEY")
        .ok_or(ConfigError::Missing("JC_MEMORY_API_KEY"))?;
    if valid_api_key(&api_key) {
        Ok(api_key)
    } else {
        Err(ConfigError::Invalid(
            "JC_MEMORY_API_KEY",
            "must match sk-jcmemory- followed by 34 lowercase alphanumeric characters".into(),
        ))
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

    const API_KEY: &str = "sk-jcmemory-87s92kdp5vqz4mnh3cr0xfbj6gwy1aue7t";

    fn values(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
        entries
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect()
    }

    #[test]
    fn api_key_requires_the_branded_secret_format() {
        let invalid = values(&[
            ("JC_MEMORY_API_KEY", "test-key"),
            (
                "JC_MEMORY_NAMESPACE_ID",
                "018f4c8f-5f7b-7d31-9a6c-b7a0f4f37001",
            ),
        ]);

        let error = ServerConfig::from_lookup(|name| invalid.get(name).cloned()).unwrap_err();

        assert_eq!(
            error,
            ConfigError::Invalid(
                "JC_MEMORY_API_KEY",
                "must match sk-jcmemory- followed by 34 lowercase alphanumeric characters".into(),
            )
        );

        let valid = values(&[
            ("JC_MEMORY_API_KEY", API_KEY),
            (
                "JC_MEMORY_NAMESPACE_ID",
                "018f4c8f-5f7b-7d31-9a6c-b7a0f4f37001",
            ),
        ]);

        let config = ServerConfig::from_lookup(|name| valid.get(name).cloned()).unwrap();
        assert_eq!(config.api_key, API_KEY);
    }

    #[test]
    fn tls_paths_are_loaded_as_a_pair_without_reading_private_material() {
        let settings = values(&[
            ("JC_MEMORY_API_KEY", API_KEY),
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
            ("JC_MEMORY_API_KEY", API_KEY),
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
            ("JC_MEMORY_API_KEY", API_KEY),
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
            ("JC_MEMORY_API_KEY", API_KEY),
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
            ("JC_MEMORY_API_KEY", API_KEY),
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
            ("JC_MEMORY_API_KEY", API_KEY),
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
            ("JC_MEMORY_API_KEY", API_KEY),
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
        assert_eq!(config.api_key, API_KEY);
    }

    #[test]
    fn retrieval_yaml_and_alias_environment_override_are_merged() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("server.yaml");
        std::fs::write(
            &path,
            "namespace_id: 018f4c8f-5f7b-7d31-9a6c-b7a0f4f37001\nretrieval:\n  aliases_file: ./yaml-aliases.yaml\n  lexical_limit_factor: 5\n  trigram_limit_factor: 6\n  entity_limit_factor: 3\n  recent_limit: 24\n  rrf_k: 70\n  max_expanded_terms: 18\n  min_trigram_chars: 3\n  min_candidate_score: 0.25\n",
        )
        .unwrap();
        let settings = values(&[
            ("JC_MEMORY_API_KEY", API_KEY),
            ("JC_MEMORY_RETRIEVAL_ALIASES_FILE", "./env-aliases.yaml"),
        ]);

        let config =
            ServerConfig::from_sources(Some(&path), |name| settings.get(name).cloned()).unwrap();

        assert_eq!(
            config.retrieval.aliases_file.as_deref(),
            Some(Path::new("./env-aliases.yaml"))
        );
        assert_eq!(config.retrieval.lexical_limit_factor, 5);
        assert_eq!(config.retrieval.trigram_limit_factor, 6);
        assert_eq!(config.retrieval.entity_limit_factor, 3);
        assert_eq!(config.retrieval.recent_limit, 24);
        assert_eq!(config.retrieval.rrf_k, 70);
        assert_eq!(config.retrieval.max_expanded_terms, 18);
        assert_eq!(config.retrieval.min_trigram_chars, 3);
        assert!((config.retrieval.min_candidate_score - 0.25).abs() < f64::EPSILON);
    }

    #[test]
    fn retrieval_limits_reject_zero_and_code_level_overflow() {
        for yaml in [
            "retrieval:\n  lexical_limit_factor: 0\n",
            "retrieval:\n  trigram_limit_factor: 999\n",
            "retrieval:\n  min_trigram_chars: 4\n",
            "retrieval:\n  min_candidate_score: 1.5\n",
        ] {
            let partial = ServerConfig::from_yaml_str(yaml).expect("解析检索 YAML");
            let settings = values(&[
                ("JC_MEMORY_API_KEY", API_KEY),
                (
                    "JC_MEMORY_NAMESPACE_ID",
                    "018f4c8f-5f7b-7d31-9a6c-b7a0f4f37001",
                ),
            ]);
            assert!(
                ServerConfig::from_partial_lookup(partial, |name| settings.get(name).cloned())
                    .is_err(),
                "应拒绝非法检索配置: {yaml}"
            );
        }
    }

    #[test]
    fn merge_reports_missing_config_file_without_exposing_secrets() {
        let path = PathBuf::from("/missing/jc-memory/server.yaml");
        let settings = values(&[("JC_MEMORY_API_KEY", API_KEY)]);

        let error = ServerConfig::from_sources(Some(&path), |name| settings.get(name).cloned())
            .unwrap_err();

        assert!(matches!(error, ConfigError::File { .. }));
        assert!(error.to_string().contains("/missing/jc-memory/server.yaml"));
        assert!(!error.to_string().contains(API_KEY));
    }

    #[test]
    fn runtime_provider_requires_endpoint_and_model_and_redacts_secrets() {
        let base = [
            ("JC_MEMORY_API_KEY", API_KEY),
            (
                "JC_MEMORY_NAMESPACE_ID",
                "018f4c8f-5f7b-7d31-9a6c-b7a0f4f37001",
            ),
        ];
        let missing_model = values(&[
            base[0],
            base[1],
            (
                "JC_MEMORY_RUNTIME_PROVIDER_ENDPOINT",
                "https://model.example/v1/chat/completions",
            ),
        ]);
        assert!(ServerConfig::from_lookup(|name| missing_model.get(name).cloned()).is_err());

        let settings = values(&[
            base[0],
            base[1],
            (
                "JC_MEMORY_RUNTIME_PROVIDER_ENDPOINT",
                "https://model.example/v1/chat/completions",
            ),
            ("JC_MEMORY_RUNTIME_PROVIDER_MODEL", "fixture-model"),
            ("JC_MEMORY_RUNTIME_PROVIDER_API_KEY", "provider-secret"),
        ]);
        let config = ServerConfig::from_lookup(|name| settings.get(name).cloned()).unwrap();
        let provider = config.runtime_provider.as_ref().unwrap();
        assert_eq!(provider.model, "fixture-model");
        let debug = format!("{config:?}");
        assert!(!debug.contains(API_KEY));
        assert!(!debug.contains("provider-secret"));
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

    #[test]
    fn operational_sections_merge_and_validate() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("server.yaml");
        std::fs::write(
            &path,
            "namespace_id: 018f4c8f-5f7b-7d31-9a6c-b7a0f4f37001\nsqlite:\n  max_connections: 2\n  busy_timeout_ms: 250\nsecurity:\n  rules_file: ./security.yaml\nruntime:\n  provider:\n    max_retries: 2\n    retry_backoff_ms: 50\n  recall:\n    max_seeds: 4\n    max_neighbors: 6\n    max_hops: 2\n    max_nodes: 24\n    timeout_ms: 900\n",
        )
        .unwrap();
        let settings = values(&[
            ("JC_MEMORY_API_KEY", API_KEY),
            ("JC_MEMORY_SQLITE_MAX_CONNECTIONS", "3"),
            ("JC_MEMORY_RUNTIME_RECALL_MAX_NODES", "32"),
        ]);

        let config =
            ServerConfig::from_sources(Some(&path), |name| settings.get(name).cloned()).unwrap();

        assert_eq!(config.sqlite.max_connections, 3);
        assert_eq!(config.sqlite.busy_timeout_ms, 250);
        assert_eq!(
            config.security_rules_file.as_deref(),
            Some(Path::new("./security.yaml"))
        );
        assert_eq!(config.runtime_provider_tuning.max_retries, 2);
        assert_eq!(config.runtime_provider_tuning.retry_backoff_ms, 50);
        assert_eq!(config.runtime_recall.max_nodes, 32);
        assert_eq!(config.runtime_recall.timeout_ms, 900);
    }

    #[test]
    fn operational_sections_reject_zero_and_overflow() {
        for yaml in [
            "sqlite:\n  max_connections: 0\n",
            "sqlite:\n  busy_timeout_ms: 0\n",
            "runtime:\n  recall:\n    max_nodes: 0\n",
            "runtime:\n  recall:\n    timeout_ms: 999999\n",
        ] {
            let partial = ServerConfig::from_yaml_str(yaml).expect("解析运行配置 YAML");
            let settings = values(&[
                ("JC_MEMORY_API_KEY", API_KEY),
                (
                    "JC_MEMORY_NAMESPACE_ID",
                    "018f4c8f-5f7b-7d31-9a6c-b7a0f4f37001",
                ),
            ]);
            assert!(
                ServerConfig::from_partial_lookup(partial, |name| settings.get(name).cloned())
                    .is_err(),
                "应拒绝非法运行配置: {yaml}"
            );
        }
    }
}
