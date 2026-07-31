use std::{fs::OpenOptions, io, path::Path, process};

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
}

impl InstanceLock {
    /// 创建并尝试取得排他锁；锁文件只记录诊断信息，不作为认证材料。
    ///
    /// # Errors
    ///
    /// 其他进程已持有锁或锁文件无法打开时返回错误。
    pub fn acquire(path: &Path) -> Result<Self, LockError> {
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(path)?;
        match file.try_lock_exclusive() {
            Ok(()) => {
                file.set_len(0)?;
                let _ = std::io::Write::write_all(
                    &mut &file,
                    format!("pid={}\n", process::id()).as_bytes(),
                );
                Ok(Self { file })
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                Err(LockError::AlreadyRunning)
            }
            Err(error) => Err(LockError::Io(error)),
        }
    }
}

impl Drop for InstanceLock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}
