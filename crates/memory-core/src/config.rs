use std::{
    path::{Path, PathBuf},
    time::Duration,
};

/// 零外部模型混合检索配置。
#[derive(Clone, Debug, PartialEq)]
pub struct RetrievalConfig {
    /// 可选版本化业务别名词典；未提供时使用编译内置词典。
    pub aliases_file: Option<PathBuf>,
    /// unicode61 每个返回节点允许读取的候选倍数。
    pub lexical_limit_factor: usize,
    /// trigram 每个返回节点允许读取的候选倍数。
    pub trigram_limit_factor: usize,
    /// 实体通道每个返回节点允许读取的候选倍数。
    pub entity_limit_factor: usize,
    /// 近期安全网的绝对读取上限。
    pub recent_limit: usize,
    /// Reciprocal Rank Fusion 的固定 `k`。
    pub rrf_k: u32,
    /// 单次查询最多使用的词典扩展词项数。
    pub max_expanded_terms: usize,
    /// `SQLite` trigram Tokenizer 的字符宽度；当前版本固定为 3。
    pub min_trigram_chars: usize,
    /// 近期候选进入融合前必须达到的确定性匹配阈值。
    pub min_candidate_score: f64,
}

impl Default for RetrievalConfig {
    fn default() -> Self {
        Self {
            aliases_file: None,
            lexical_limit_factor: 4,
            trigram_limit_factor: 4,
            entity_limit_factor: 2,
            recent_limit: 32,
            rrf_k: 60,
            max_expanded_terms: 24,
            min_trigram_chars: 3,
            min_candidate_score: 0.2,
        }
    }
}

impl RetrievalConfig {
    /// 校验所有查询预算都处于代码级硬上限内。
    ///
    /// # Errors
    ///
    /// 零值、超上限、非有限分数或非三字符 Tokenizer 配置会被拒绝。
    pub fn validate(&self) -> Result<(), RetrievalConfigError> {
        validate_positive_limit("lexical_limit_factor", self.lexical_limit_factor, 16)?;
        validate_positive_limit("trigram_limit_factor", self.trigram_limit_factor, 16)?;
        validate_positive_limit("entity_limit_factor", self.entity_limit_factor, 8)?;
        validate_positive_limit("recent_limit", self.recent_limit, 100)?;
        validate_positive_limit("rrf_k", self.rrf_k as usize, 10_000)?;
        validate_positive_limit("max_expanded_terms", self.max_expanded_terms, 128)?;
        if self.min_trigram_chars != 3 {
            return Err(RetrievalConfigError::InvalidValue("min_trigram_chars"));
        }
        if !self.min_candidate_score.is_finite()
            || !(0.0..=1.0).contains(&self.min_candidate_score)
            || self.min_candidate_score == 0.0
        {
            return Err(RetrievalConfigError::InvalidValue("min_candidate_score"));
        }
        Ok(())
    }
}

fn validate_positive_limit(
    field: &'static str,
    value: usize,
    maximum: usize,
) -> Result<(), RetrievalConfigError> {
    if value == 0 || value > maximum {
        Err(RetrievalConfigError::InvalidValue(field))
    } else {
        Ok(())
    }
}

/// 混合检索配置错误。
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum RetrievalConfigError {
    /// 字段为零、超过硬上限或与固定 Tokenizer 契约不一致。
    #[error("invalid retrieval configuration field: {0}")]
    InvalidValue(&'static str),
}

/// Local embedded-engine configuration.
#[derive(Clone, Debug, PartialEq)]
pub struct MemoryConfig {
    data_dir: PathBuf,
    busy_timeout: Duration,
    sqlite_max_connections: u32,
    security_rules_file: Option<PathBuf>,
    retrieval: RetrievalConfig,
}

impl MemoryConfig {
    /// Creates a local-only configuration rooted at `data_dir`.
    #[must_use]
    pub fn local(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: data_dir.into(),
            busy_timeout: Duration::from_secs(5),
            sqlite_max_connections: 4,
            security_rules_file: None,
            retrieval: RetrievalConfig::default(),
        }
    }

    /// Sets how long each `SQLite` lock attempt may wait before returning Busy.
    #[must_use]
    pub const fn with_busy_timeout(mut self, busy_timeout: Duration) -> Self {
        self.busy_timeout = busy_timeout;
        self
    }

    /// 设置 SQLite 连接池上限；零值会在 Engine 启动时拒绝。
    #[must_use]
    pub const fn with_sqlite_max_connections(mut self, max_connections: u32) -> Self {
        self.sqlite_max_connections = max_connections;
        self
    }

    /// 设置外部 Quarantine 规则文件；文件不存在或内容非法时启动失败。
    #[must_use]
    pub fn with_security_rules_file(mut self, path: impl Into<PathBuf>) -> Self {
        self.security_rules_file = Some(path.into());
        self
    }

    /// 设置经过启动校验的零模型检索配置。
    #[must_use]
    pub fn with_retrieval_config(mut self, retrieval: RetrievalConfig) -> Self {
        self.retrieval = retrieval;
        self
    }

    /// Returns the root directory containing all durable engine state.
    #[must_use]
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub(crate) const fn busy_timeout(&self) -> Duration {
        self.busy_timeout
    }

    pub(crate) const fn sqlite_max_connections(&self) -> u32 {
        self.sqlite_max_connections
    }

    pub(crate) fn security_rules_file(&self) -> Option<&Path> {
        self.security_rules_file.as_deref()
    }

    pub(crate) const fn retrieval(&self) -> &RetrievalConfig {
        &self.retrieval
    }

    pub(crate) fn database_path(&self) -> PathBuf {
        self.data_dir.join("memory.sqlite3")
    }

    pub(crate) fn blob_root(&self) -> PathBuf {
        self.data_dir.join("blobs")
    }
}
