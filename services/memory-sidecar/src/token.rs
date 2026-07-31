use std::{io, path::Path, sync::Arc};

const TOKEN_BYTES: usize = 32;

/// Sidecar Token 文件操作错误。
#[derive(Debug, thiserror::Error)]
pub enum TokenError {
    /// 文件系统错误。
    #[error("token file error: {0}")]
    Io(#[from] io::Error),
    /// Token 文件内容不符合固定长度。
    #[error("token file must contain exactly {TOKEN_BYTES} raw bytes")]
    InvalidLength,
    /// 随机源不可用。
    #[error("secure random source unavailable")]
    Random,
}

/// 由权限受限文件承载的固定长度认证 Token。
#[derive(Clone)]
pub struct TokenStore {
    token: Arc<[u8; TOKEN_BYTES]>,
}

impl TokenStore {
    /// 创建或读取 Token 文件，并将新文件权限限制为仅所有者可读写。
    ///
    /// # Errors
    ///
    /// Token 文件无法读取、创建、写入或随机源不可用时返回错误。
    pub async fn load_or_create(path: &Path) -> Result<Self, TokenError> {
        if let Ok(bytes) = tokio::fs::read(path).await {
            return Self::from_bytes(bytes);
        }
        let mut token = [0_u8; TOKEN_BYTES];
        getrandom::fill(&mut token).map_err(|_| TokenError::Random)?;
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(path, token).await?;
        restrict_permissions(path).await?;
        Ok(Self {
            token: Arc::new(token),
        })
    }

    /// 从内存字节构造 Token，主要用于版本化 Fixture 和测试。
    ///
    /// # Errors
    ///
    /// 输入不是固定 32 字节时返回错误。
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, TokenError> {
        let token: [u8; TOKEN_BYTES] = bytes.try_into().map_err(|_| TokenError::InvalidLength)?;
        Ok(Self {
            token: Arc::new(token),
        })
    }

    /// 返回 Token 的十六进制表示，CLI 只显示路径而不显示此值。
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; TOKEN_BYTES] {
        &self.token
    }

    /// 使用无分支长度固定比较验证调用方 Token。
    #[must_use]
    pub fn constant_time_eq(&self, candidate: &[u8]) -> bool {
        if candidate.len() != TOKEN_BYTES {
            return false;
        }
        let mut difference = 0_u8;
        for (left, right) in self.token.iter().zip(candidate) {
            difference |= left ^ right;
        }
        difference == 0
    }
}

async fn restrict_permissions(path: &Path) -> Result<(), io::Error> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).await?;
    }
    Ok(())
}
