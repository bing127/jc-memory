//! 可插拔密钥读取与轮换元数据。

use std::{collections::BTreeMap, fmt};

use chrono::{DateTime, Utc};
use zeroize::Zeroizing;

/// 不暴露内部持久化方式的密钥提供者。
pub trait KeyProvider: Send + Sync {
    /// 返回离开作用域时自动清零的 256 位密钥副本。
    ///
    /// # Errors
    ///
    /// `key_id` 不存在时返回错误。
    fn get_key(&self, key_id: &str) -> Result<Zeroizing<[u8; 32]>, KeyError>;

    /// 返回不含密钥材料的轮换元数据。
    ///
    /// # Errors
    ///
    /// `key_id` 不存在时返回错误。
    fn metadata(&self, key_id: &str) -> Result<KeyRotationMetadata, KeyError>;
}

/// 不含秘密材料的密钥轮换记录。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeyRotationMetadata {
    /// 稳定密钥标识。
    pub key_id: String,
    /// 单调递增版本。
    pub version: u64,
    /// 前一密钥标识。
    pub rotated_from: Option<String>,
    /// 登记时间。
    pub created_at: DateTime<Utc>,
}

/// 密钥提供者错误。
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum KeyError {
    /// 密钥标识为空或重复。
    #[error("invalid or duplicate key id")]
    InvalidKeyId,
    /// 密钥不存在。
    #[error("key was not found: {0}")]
    NotFound(String),
    /// 轮换来源不存在。
    #[error("rotation source was not found: {0}")]
    RotationSourceNotFound(String),
}

/// 用于嵌入式部署和测试的进程内密钥提供者。
#[derive(Default)]
pub struct InMemoryKeyProvider {
    entries: BTreeMap<String, (Zeroizing<[u8; 32]>, KeyRotationMetadata)>,
    next_version: u64,
}

impl fmt::Debug for InMemoryKeyProvider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("InMemoryKeyProvider")
            .field("entry_count", &self.entries.len())
            .field("key_material", &"[REDACTED]")
            .field("next_version", &self.next_version)
            .finish()
    }
}

impl InMemoryKeyProvider {
    /// 创建空提供者。
    #[must_use]
    pub const fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
            next_version: 1,
        }
    }

    /// 登记新密钥及可选轮换来源。
    ///
    /// # Errors
    ///
    /// 标识非法、重复或轮换来源不存在时返回错误。
    pub fn insert(
        &mut self,
        key_id: impl Into<String>,
        key: [u8; 32],
        rotated_from: Option<&str>,
    ) -> Result<KeyRotationMetadata, KeyError> {
        let key_id = key_id.into();
        if key_id.trim().is_empty() || self.entries.contains_key(&key_id) {
            return Err(KeyError::InvalidKeyId);
        }
        if let Some(source) = rotated_from
            && !self.entries.contains_key(source)
        {
            return Err(KeyError::RotationSourceNotFound(source.to_owned()));
        }
        let metadata = KeyRotationMetadata {
            key_id: key_id.clone(),
            version: self.next_version,
            rotated_from: rotated_from.map(str::to_owned),
            created_at: Utc::now(),
        };
        self.next_version = self.next_version.saturating_add(1);
        self.entries
            .insert(key_id, (Zeroizing::new(key), metadata.clone()));
        Ok(metadata)
    }
}

impl KeyProvider for InMemoryKeyProvider {
    fn get_key(&self, key_id: &str) -> Result<Zeroizing<[u8; 32]>, KeyError> {
        self.entries
            .get(key_id)
            .map(|(key, _)| Zeroizing::new(**key))
            .ok_or_else(|| KeyError::NotFound(key_id.to_owned()))
    }

    fn metadata(&self, key_id: &str) -> Result<KeyRotationMetadata, KeyError> {
        self.entries
            .get(key_id)
            .map(|(_, metadata)| metadata.clone())
            .ok_or_else(|| KeyError::NotFound(key_id.to_owned()))
    }
}
