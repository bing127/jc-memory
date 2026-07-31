//! 流式 Fragment Generator 和批次 Checkpoint 驱动。

use std::{fs, path::Path};

use crate::checkpoint::{
    Checkpoint, CheckpointError, config_digest, load_checkpoint, resume_sequence, save_checkpoint,
};

/// 一个按序生成且不保留全量数据的 Fragment。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GeneratedFragment {
    /// 单调递增的序列号。
    pub sequence: u64,
    /// 确定性合成内容。
    pub payload: String,
}

/// 流式生成器，只保存 Seed、总量和当前状态。
#[derive(Clone, Debug)]
pub struct Generator {
    state: u64,
    remaining: u64,
    next_sequence: u64,
}

impl Generator {
    /// 创建固定 Seed 的流式生成器。
    #[must_use]
    pub const fn new(seed: u64, total: u64) -> Self {
        Self {
            state: seed ^ 0x9e37_79b9_7f4a_7c15,
            remaining: total,
            next_sequence: 0,
        }
    }
}

impl Iterator for Generator {
    type Item = GeneratedFragment;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let fragment = GeneratedFragment {
            sequence: self.next_sequence,
            payload: format!("fragment-{}", self.state % 1_000_000),
        };
        self.remaining -= 1;
        self.next_sequence += 1;
        Some(fragment)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = usize::try_from(self.remaining).unwrap_or(usize::MAX);
        (remaining, Some(remaining))
    }
}

/// 一次批量生成的结果。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GenerationOutcome {
    /// 最后一次成功写入的 Checkpoint。
    pub checkpoint: Checkpoint,
    /// 是否在显式故障注入点停止。
    pub crashed: bool,
}

/// 运行可恢复 Smoke；`fail_after_batches` 用于确定性模拟崩溃。
///
/// # Errors
///
/// 当输入批次无效或 Checkpoint 无法保存时返回错误。
pub fn generate_smoke<P: AsRef<Path>>(
    total: u64,
    batch_size: u64,
    checkpoint_path: P,
    fail_after_batches: Option<u64>,
) -> Result<GenerationOutcome, CheckpointError> {
    if total == 0 || batch_size == 0 {
        return Err(CheckpointError::Invalid(
            "total 和 batch_size 必须大于 0".to_owned(),
        ));
    }
    let checkpoint_path = checkpoint_path.as_ref();
    if let Some(parent) = checkpoint_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let digest = config_digest(&format!("fragments={total};batch_size={batch_size}"));
    let mut generator = Generator::new(7, total);
    let mut processed = 0_u64;
    let mut batches = 0_u64;
    let mut checkpoint = Checkpoint::new(digest, 0, 0);
    while processed < total {
        let remaining = total - processed;
        let current_batch = remaining.min(batch_size);
        for _ in 0..current_batch {
            let _ = generator.next();
        }
        processed += current_batch;
        batches += 1;
        checkpoint = Checkpoint::new(checkpoint.config_digest.clone(), processed - 1, processed);
        save_checkpoint(checkpoint_path, &checkpoint)?;
        if fail_after_batches.is_some_and(|limit| batches >= limit) && processed < total {
            return Ok(GenerationOutcome {
                checkpoint,
                crashed: true,
            });
        }
    }
    Ok(GenerationOutcome {
        checkpoint,
        crashed: false,
    })
}

/// 从现有 Checkpoint 继续生成剩余 Fragment。
///
/// # Errors
///
/// 当 Checkpoint 不存在、配置 Digest 不匹配或保存剩余批次失败时返回错误。
pub fn resume_smoke<P: AsRef<Path>>(
    total: u64,
    batch_size: u64,
    checkpoint_path: P,
) -> Result<GenerationOutcome, CheckpointError> {
    if total == 0 || batch_size == 0 {
        return Err(CheckpointError::Invalid(
            "total 和 batch_size 必须大于 0".to_owned(),
        ));
    }
    let checkpoint_path = checkpoint_path.as_ref();
    let digest = config_digest(&format!("fragments={total};batch_size={batch_size}"));
    let checkpoint = load_checkpoint(checkpoint_path)?;
    let next_sequence = resume_sequence(&checkpoint, &digest)?;
    if next_sequence >= total {
        return Ok(GenerationOutcome {
            checkpoint,
            crashed: false,
        });
    }
    let skip = usize::try_from(next_sequence)
        .map_err(|_| CheckpointError::Invalid("sequence 超出当前平台 usize".to_owned()))?;
    let mut generator = Generator::new(7, total).skip(skip);
    let mut processed = next_sequence;
    let mut current = checkpoint;
    while processed < total {
        let current_batch = (total - processed).min(batch_size);
        for _ in 0..current_batch {
            let _ = generator.next();
        }
        processed += current_batch;
        current = Checkpoint::new(current.config_digest.clone(), processed - 1, processed);
        save_checkpoint(checkpoint_path, &current)?;
    }
    Ok(GenerationOutcome {
        checkpoint: current,
        crashed: false,
    })
}
