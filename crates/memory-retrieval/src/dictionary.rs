use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use sha2::{Digest as _, Sha256};

use crate::normalize_text;

const SCHEMA_VERSION: &str = "jcmem.retrieval-aliases.v1";
const MAX_GROUPS: usize = 256;
const MAX_TERMS_PER_GROUP: usize = 64;
const MAX_TERM_CHARS: usize = 128;

/// 版本化业务别名词典。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AliasDictionary {
    version: String,
    sha256: String,
    groups: BTreeMap<String, Vec<String>>,
}

impl AliasDictionary {
    /// 从严格 YAML 文本加载词典。
    ///
    /// # Errors
    ///
    /// Schema、版本、concept 或 term 不满足有界契约时返回错误。
    pub fn from_yaml_str(input: &str) -> Result<Self, DictionaryError> {
        let raw: RawDictionary = yaml_serde::from_str(input)
            .map_err(|error| DictionaryError::Yaml(error.to_string()))?;
        if raw.schema_version != SCHEMA_VERSION {
            return Err(DictionaryError::UnsupportedSchema(raw.schema_version));
        }
        if raw.dictionary_version.trim().is_empty() {
            return Err(DictionaryError::EmptyVersion);
        }
        if raw.language.trim().is_empty() {
            return Err(DictionaryError::EmptyLanguage);
        }
        if raw.groups.is_empty() || raw.groups.len() > MAX_GROUPS {
            return Err(DictionaryError::InvalidGroupCount(raw.groups.len()));
        }

        let mut groups = BTreeMap::new();
        for group in raw.groups {
            let concept = group.concept.trim();
            if concept.is_empty() {
                return Err(DictionaryError::EmptyConcept);
            }
            if group.terms.is_empty() || group.terms.len() > MAX_TERMS_PER_GROUP {
                return Err(DictionaryError::InvalidTermCount {
                    concept: concept.to_owned(),
                    count: group.terms.len(),
                });
            }
            let mut terms = BTreeSet::new();
            for term in group.terms {
                let normalized = normalize_text(&term);
                let char_count = normalized.chars().count();
                if char_count == 0 || char_count > MAX_TERM_CHARS {
                    return Err(DictionaryError::InvalidTerm {
                        concept: concept.to_owned(),
                    });
                }
                terms.insert(normalized);
            }
            if groups
                .insert(concept.to_owned(), terms.into_iter().collect())
                .is_some()
            {
                return Err(DictionaryError::DuplicateConcept(concept.to_owned()));
            }
        }

        Ok(Self {
            version: raw.dictionary_version,
            sha256: hex::encode(Sha256::digest(input.as_bytes())),
            groups,
        })
    }

    /// 加载仓库内置的离线默认词典。
    ///
    /// # Errors
    ///
    /// 编译内置词典不满足当前 Schema 时返回错误。
    pub fn builtin() -> Result<Self, DictionaryError> {
        Self::from_yaml_str(include_str!("../../../config/retrieval/aliases.zh-CN.yaml"))
    }

    /// 返回词典业务版本。
    #[must_use]
    pub fn version(&self) -> &str {
        &self.version
    }

    /// 返回原始 YAML 的 SHA-256。
    #[must_use]
    pub fn sha256(&self) -> &str {
        &self.sha256
    }

    /// 按查询中出现的任一已知词项双向扩展同一 concept。
    #[must_use]
    pub fn expand(&self, input: &str, limit: usize) -> Vec<String> {
        if limit == 0 {
            return Vec::new();
        }
        let normalized = normalize_text(input);
        let mut expanded = BTreeSet::new();
        for terms in self.groups.values() {
            if terms
                .iter()
                .any(|term| normalized == *term || normalized.contains(term))
            {
                expanded.extend(terms.iter().cloned());
            }
        }
        expanded.into_iter().take(limit).collect()
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDictionary {
    schema_version: String,
    dictionary_version: String,
    language: String,
    groups: Vec<RawAliasGroup>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawAliasGroup {
    concept: String,
    terms: Vec<String>,
}

/// 业务别名词典加载错误。
#[derive(Debug, thiserror::Error)]
pub enum DictionaryError {
    /// YAML 无法按严格 Schema 解码。
    #[error("invalid retrieval alias YAML: {0}")]
    Yaml(String),
    /// Schema 版本不受支持。
    #[error("unsupported retrieval alias schema: {0}")]
    UnsupportedSchema(String),
    /// 词典版本为空。
    #[error("retrieval alias dictionary version cannot be empty")]
    EmptyVersion,
    /// 语言标识为空。
    #[error("retrieval alias dictionary language cannot be empty")]
    EmptyLanguage,
    /// 分组数越界。
    #[error("retrieval alias group count is invalid: {0}")]
    InvalidGroupCount(usize),
    /// concept 为空。
    #[error("retrieval alias concept cannot be empty")]
    EmptyConcept,
    /// concept 重复。
    #[error("duplicate retrieval alias concept: {0}")]
    DuplicateConcept(String),
    /// 分组中的词项数量越界。
    #[error("retrieval alias term count for {concept} is invalid: {count}")]
    InvalidTermCount {
        /// 发生错误的 concept。
        concept: String,
        /// 实际词项数量。
        count: usize,
    },
    /// 词项为空或过长。
    #[error("retrieval alias term for {concept} is empty or too long")]
    InvalidTerm {
        /// 发生错误的 concept。
        concept: String,
    },
}
