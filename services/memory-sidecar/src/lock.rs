use std::{
    fs::OpenOptions,
    io::{self, Read as _, Seek as _, SeekFrom, Write as _},
    path::Path,
    process,
    time::{SystemTime, UNIX_EPOCH},
};

use fs2::FileExt as _;

/// Sidecar 单实例锁错误。
#[derive(Debug, thiserror::Error)]
pub enum LockError {
    /// 文件系统错误。
    #[error("sidecar lock error: {0}")]
    Io(#[from] io::Error),
    /// 已有其他进程持有实例锁。
    #[error("another sidecar instance already owns the lock")]
    AlreadyRunning,
}

/// 持有期间保证一个数据目录只运行一个 Sidecar。
pub struct InstanceLock {
    file: std::fs::File,
    stale_metadata: bool,
}

impl InstanceLock {
    /// 创建并尝试取得排他锁；锁文件记录 PID 和启动标识用于诊断陈旧元数据。
    ///
    /// # Errors
    ///
    /// 其他进程已持有锁或锁文件无法打开时返回错误。
    pub fn acquire(path: &Path) -> Result<Self, LockError> {
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(path)?;
        let mut previous = String::new();
        (&file).read_to_string(&mut previous)?;
        match file.try_lock_exclusive() {
            Ok(()) => {
                file.set_len(0)?;
                (&file).seek(SeekFrom::Start(0))?;
                let started_at = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                let mut writer = &file;
                writer.write_all(
                    format!("pid={}\nstarted_at={started_at}\n", process::id()).as_bytes(),
                )?;
                file.sync_all()?;
                Ok(Self {
                    file,
                    stale_metadata: !previous.trim().is_empty(),
                })
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                Err(LockError::AlreadyRunning)
            }
            Err(error) => Err(LockError::Io(error)),
        }
    }

    /// 返回本次取得锁时是否发现了上一个实例留下的元数据。
    #[must_use]
    pub const fn recovered_stale_metadata(&self) -> bool {
        self.stale_metadata
    }
}

impl Drop for InstanceLock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}
