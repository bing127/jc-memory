use jc_memory_protocol::RecallMode;

use crate::{AliasDictionary, character_trigrams, lexical_terms, normalize_text};

const MAX_EXPANDED_TERMS: usize = 128;
const MAX_TRIGRAMS: usize = 256;
const MAX_QUERY_CHARS: usize = 2_048;

/// 交给存储层安全编译的字面查询形式。
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LexicalQuery {
    /// 保持词序和完整内容的 Exact 短语。
    Phrase(String),
    /// 任一词项命中即可成为候选的非 Exact 查询。
    AnyTerm(Vec<String>),
}

/// 查询规划阶段的硬上限。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RetrievalLimits {
    /// 单次查询最多扩展的业务别名数。
    pub max_expanded_terms: usize,
    /// 单次查询最多生成的字符 trigram 数。
    pub max_trigrams: usize,
}

impl Default for RetrievalLimits {
    fn default() -> Self {
        Self {
            max_expanded_terms: 24,
            max_trigrams: 64,
        }
    }
}

/// 不接触存储的确定性查询计划。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RetrievalPlan {
    /// Exact 模式使用的完整规范短语。
    pub exact_phrase: Option<String>,
    /// 非 Exact 模式使用的字面词项。
    pub lexical_terms: Vec<String>,
    /// 非 Exact 模式使用的版本化业务别名。
    pub alias_terms: Vec<String>,
    /// 非 Exact 模式使用的字符 trigram。
    pub trigrams: Vec<String>,
    /// 生成计划的词典版本。
    pub dictionary_version: String,
    /// 生成计划的词典 SHA-256。
    pub dictionary_sha256: String,
}

/// 零模型查询规划器。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueryPlanner {
    dictionary: AliasDictionary,
    limits: RetrievalLimits,
}

impl QueryPlanner {
    /// 创建带硬上限的查询规划器。
    ///
    /// # Errors
    ///
    /// 任一上限为零或超过代码级最大值时返回错误。
    pub fn new(
        dictionary: AliasDictionary,
        limits: RetrievalLimits,
    ) -> Result<Self, RetrievalError> {
        if limits.max_expanded_terms == 0 || limits.max_expanded_terms > MAX_EXPANDED_TERMS {
            return Err(RetrievalError::InvalidLimit("max_expanded_terms"));
        }
        if limits.max_trigrams == 0 || limits.max_trigrams > MAX_TRIGRAMS {
            return Err(RetrievalError::InvalidLimit("max_trigrams"));
        }
        Ok(Self { dictionary, limits })
    }

    /// 为指定 Recall 模式生成稳定查询计划。
    ///
    /// # Errors
    ///
    /// 查询为空或超过代码级字符上限时返回错误。
    pub fn plan(&self, mode: RecallMode, input: &str) -> Result<RetrievalPlan, RetrievalError> {
        let normalized = normalize_text(input);
        let query_chars = normalized.chars().count();
        if query_chars == 0 {
            return Err(RetrievalError::EmptyQuery);
        }
        if query_chars > MAX_QUERY_CHARS {
            return Err(RetrievalError::QueryTooLong);
        }
        let exact = mode == RecallMode::Exact;
        Ok(RetrievalPlan {
            exact_phrase: exact.then_some(normalized.clone()),
            lexical_terms: if exact {
                Vec::new()
            } else {
                lexical_terms(&normalized)
            },
            alias_terms: if exact {
                Vec::new()
            } else {
                self.dictionary
                    .expand(&normalized, self.limits.max_expanded_terms)
            },
            trigrams: if exact {
                Vec::new()
            } else {
                character_trigrams(&normalized, self.limits.max_trigrams)
            },
            dictionary_version: self.dictionary.version().to_owned(),
            dictionary_sha256: self.dictionary.sha256().to_owned(),
        })
    }
}

/// 查询规划错误。
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum RetrievalError {
    /// 查询规范化后为空。
    #[error("retrieval query cannot be empty")]
    EmptyQuery,
    /// 查询超过代码级字符上限。
    #[error("retrieval query is too long")]
    QueryTooLong,
    /// 配置上限无效。
    #[error("retrieval limit is invalid: {0}")]
    InvalidLimit(&'static str),
}
