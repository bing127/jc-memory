use std::path::{Path, PathBuf};

/// Sidecar 本地运行所需的文件路径配置。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SidecarConfig {
    /// 数据目录，也是默认状态文件目录。
    pub data_dir: PathBuf,
    /// 本地传输端点；Unix 使用 UDS，Windows 使用 Named Pipe 名称映射，其他平台使用地址文本。
    pub endpoint: PathBuf,
    /// 认证 Token 文件。
    pub token_file: PathBuf,
    /// 单实例排他锁文件。
    pub lock_file: PathBuf,
    /// 生命周期检查点文件。
    pub checkpoint_file: PathBuf,
}

impl SidecarConfig {
    /// 从数据目录构造默认配置。
    #[must_use]
    pub fn local(data_dir: impl Into<PathBuf>) -> Self {
        let data_dir = data_dir.into();
        Self {
            endpoint: data_dir.join("jcmemd.sock"),
            token_file: data_dir.join("jcmemd.token"),
            lock_file: data_dir.join("jcmemd.lock"),
            checkpoint_file: data_dir.join("jcmemd.checkpoint.json"),
            data_dir,
        }
    }

    /// 确保 Sidecar 所需父目录存在。
    ///
    /// # Errors
    ///
    /// 父目录无法创建时返回文件系统错误。
    pub async fn prepare(&self) -> Result<(), std::io::Error> {
        tokio::fs::create_dir_all(&self.data_dir).await
    }

    /// 校验路径都位于配置数据目录下，避免误操作宽泛路径。
    ///
    /// # Errors
    ///
    /// 端点、Token 或锁路径位于数据目录之外时返回错误。
    pub fn validate(&self) -> Result<(), String> {
        for (name, path) in [
            ("endpoint", &self.endpoint),
            ("token_file", &self.token_file),
            ("lock_file", &self.lock_file),
            ("checkpoint_file", &self.checkpoint_file),
        ] {
            if !path.starts_with(&self.data_dir) {
                return Err(format!("{name} must be inside data_dir"));
            }
        }
        Ok(())
    }

    /// 返回端点路径的字符串表示，便于 CLI 和日志使用。
    #[must_use]
    pub fn endpoint_display(&self) -> &Path {
        &self.endpoint
    }
}
