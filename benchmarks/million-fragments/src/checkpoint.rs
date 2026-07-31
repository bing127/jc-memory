//! 原子 Checkpoint、Digest 校验和 Resume 边界。

use std::{fs, io::Write, path::Path};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

/// 最后一个已提交批次的状态。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Checkpoint {
    /// 运行配置的 SHA-256。
    pub config_digest: String,
    /// 最后一个已提交的 sequence。
    pub last_sequence: u64,
    /// 已提交 Fragment 数量。
    pub committed_fragments: u64,
}

impl Checkpoint {
    /// 创建 Checkpoint。
    #[must_use]
    pub fn new(config_digest: String, last_sequence: u64, committed_fragments: u64) -> Self {
        Self {
            config_digest,
            last_sequence,
            committed_fragments,
        }
    }
}

/// Checkpoint 或 Resume 失败。
#[derive(Debug, Error)]
pub enum CheckpointError {
    /// 文件系统错误。
    #[error("Checkpoint 文件操作失败: {0}")]
    Io(#[from] std::io::Error),
    /// JSON 编解码失败。
    #[error("Checkpoint JSON 无效: {0}")]
    Json(#[from] serde_json::Error),
    /// 配置 Digest 不匹配。
    #[error("Checkpoint config digest 不匹配: expected {expected}, actual {actual}")]
    ConfigMismatch {
        /// 运行时提供的配置 Digest。
        expected: String,
        /// Checkpoint 中保存的配置 Digest。
        actual: String,
    },
    /// Checkpoint 字段不满足不变量。
    #[error("Checkpoint 无效: {0}")]
    Invalid(String),
}

/// 计算配置 canonical UTF-8 的 SHA-256。
#[must_use]
pub fn config_digest(config: &str) -> String {
    hex::encode(Sha256::digest(config.as_bytes()))
}

/// 原子保存 Checkpoint。
///
/// # Errors
///
/// 当字段不满足不变量或临时文件写入、同步、重命名失败时返回错误。
pub fn save_checkpoint<P: AsRef<Path>>(
    path: P,
    checkpoint: &Checkpoint,
) -> Result<(), CheckpointError> {
    if checkpoint.config_digest.is_empty() || checkpoint.committed_fragments == 0 {
        return Err(CheckpointError::Invalid(
            "config_digest 和 committed_fragments 不能为空".to_owned(),
        ));
    }
    let path = path.as_ref();
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    let bytes = serde_json::to_vec_pretty(checkpoint)?;
    {
        let mut file = fs::File::create(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
    }
    fs::rename(&temporary, path)?;
    Ok(())
}

/// 加载并验证 Checkpoint。
///
/// # Errors
///
/// 当文件读取、JSON 解码或 sequence 不变量校验失败时返回错误。
pub fn load_checkpoint<P: AsRef<Path>>(path: P) -> Result<Checkpoint, CheckpointError> {
    let checkpoint: Checkpoint = serde_json::from_slice(&fs::read(path)?)?;
    if checkpoint.config_digest.is_empty() || checkpoint.committed_fragments == 0 {
        return Err(CheckpointError::Invalid(
            "Checkpoint 字段不能为空".to_owned(),
        ));
    }
    if checkpoint.last_sequence + 1 != checkpoint.committed_fragments {
        return Err(CheckpointError::Invalid(
            "last_sequence 必须等于 committed_fragments - 1".to_owned(),
        ));
    }
    Ok(checkpoint)
}

/// 校验配置后返回下一次应该生成的 sequence。
///
/// # Errors
///
/// 当 config digest 不匹配或 sequence 溢出时返回错误。
pub fn resume_sequence(
    checkpoint: &Checkpoint,
    expected_config_digest: &str,
) -> Result<u64, CheckpointError> {
    if checkpoint.config_digest != expected_config_digest {
        return Err(CheckpointError::ConfigMismatch {
            expected: expected_config_digest.to_owned(),
            actual: checkpoint.config_digest.clone(),
        });
    }
    checkpoint
        .last_sequence
        .checked_add(1)
        .ok_or_else(|| CheckpointError::Invalid("sequence 溢出".to_owned()))
}
