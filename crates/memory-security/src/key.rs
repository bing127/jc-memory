//! 可插拔密钥读取与轮换元数据。

use std::{
    collections::BTreeMap,
    fmt,
    fs::{self, File, OpenOptions},
    io::{Read as _, Write as _},
    path::{Path, PathBuf},
};

use chrono::{DateTime, Utc};
use fs2::FileExt as _;
use zeroize::Zeroizing;

const FILE_MAGIC: &[u8; 4] = b"JCK1";
const FILE_VERSION: u8 = 1;
const MAX_KEY_ID_BYTES: usize = 128;
const LOCK_FILE: &str = ".lock";

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
    /// 文件型 Provider 的记录损坏或格式未知。
    #[error("key record is corrupt")]
    CorruptRecord,
    /// 文件型 Provider 的目录或文件权限过宽。
    #[error("key store permissions are too broad")]
    PermissionDenied,
    /// 文件型 Provider 的文件系统操作失败。
    #[error("key store operation failed: {operation}")]
    Io {
        /// 失败的稳定操作名称。
        operation: &'static str,
    },
    /// 文件型 Provider 根路径不是目录。
    #[error("key store root is invalid")]
    InvalidStore,
    /// 操作系统无法提供安全随机数。
    #[error("secure randomness is unavailable")]
    RandomnessUnavailable,
}

/// 单机/单写者场景使用的受限权限文件型密钥提供者。
pub struct FileKeyProvider {
    root: PathBuf,
}

impl fmt::Debug for FileKeyProvider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FileKeyProvider")
            .field("root", &self.root)
            .field("key_material", &"[REDACTED]")
            .finish()
    }
}

impl FileKeyProvider {
    /// 打开或创建文件型密钥根目录。
    ///
    /// # Errors
    ///
    /// 根目录不是目录、权限过宽或文件系统操作失败时返回错误。
    pub fn open(root: impl AsRef<Path>) -> Result<Self, KeyError> {
        let root = root.as_ref().to_path_buf();
        ensure_root(&root)?;
        let lock_path = root.join(LOCK_FILE);
        let existed = match fs::symlink_metadata(&lock_path) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
                    return Err(KeyError::InvalidStore);
                }
                true
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(_) => {
                return Err(KeyError::Io {
                    operation: "inspect lock",
                });
            }
        };
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&lock_path)
            .map_err(|_| KeyError::Io {
                operation: "open lock",
            })?;
        if !existed {
            set_restricted_file_permissions(&lock)?;
        }
        ensure_restricted_file(&lock_path, &lock)?;
        Ok(Self { root })
    }

    /// 写入一把新的固定 256 位密钥，并记录轮换来源。
    ///
    /// 已存在的 ID 不会被覆盖；写入使用同目录临时文件和原子发布。
    ///
    /// # Errors
    ///
    /// ID 非法、重复、轮换来源不存在、记录损坏或文件系统操作失败时返回错误。
    pub fn insert(
        &self,
        key_id: impl Into<String>,
        key: [u8; 32],
        rotated_from: Option<&str>,
    ) -> Result<KeyRotationMetadata, KeyError> {
        let key_id = key_id.into();
        validate_key_id(&key_id)?;
        if let Some(source) = rotated_from {
            validate_key_id(source)?;
        }
        let lock = self.acquire_lock()?;
        let target = self.path_for(&key_id);
        if fs::symlink_metadata(&target).is_ok() {
            return Err(KeyError::InvalidKeyId);
        }
        if let Some(source) = rotated_from
            && !self.record_exists(source)?
        {
            return Err(KeyError::RotationSourceNotFound(source.to_owned()));
        }
        let version = self.next_version()?;
        let metadata = KeyRotationMetadata {
            key_id: key_id.clone(),
            version,
            rotated_from: rotated_from.map(str::to_owned),
            created_at: Utc::now(),
        };
        let record = KeyRecord {
            metadata: metadata.clone(),
            key: Zeroizing::new(key),
        };
        self.publish(&key_id, &record)?;
        drop(lock);
        Ok(metadata)
    }

    /// 生成并保存一把由操作系统随机源提供的密钥。
    ///
    /// # Errors
    ///
    /// 随机源不可用或保存失败时返回错误。
    pub fn generate(
        &self,
        key_id: impl Into<String>,
        rotated_from: Option<&str>,
    ) -> Result<KeyRotationMetadata, KeyError> {
        let mut key = Zeroizing::new([0; 32]);
        getrandom::fill(&mut *key).map_err(|_| KeyError::RandomnessUnavailable)?;
        self.insert(key_id, *key, rotated_from)
    }

    fn acquire_lock(&self) -> Result<File, KeyError> {
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .open(self.root.join(LOCK_FILE))
            .map_err(|_| KeyError::Io {
                operation: "open lock",
            })?;
        lock.lock_exclusive().map_err(|_| KeyError::Io {
            operation: "lock key store",
        })?;
        Ok(lock)
    }

    fn path_for(&self, key_id: &str) -> PathBuf {
        self.root.join(format!("{key_id}.jck"))
    }

    fn record_exists(&self, key_id: &str) -> Result<bool, KeyError> {
        let path = self.path_for(key_id);
        match fs::symlink_metadata(path) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
                    return Err(KeyError::CorruptRecord);
                }
                Ok(true)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(_) => Err(KeyError::Io {
                operation: "inspect key record",
            }),
        }
    }

    fn next_version(&self) -> Result<u64, KeyError> {
        let entries = fs::read_dir(&self.root).map_err(|_| KeyError::Io {
            operation: "list key records",
        })?;
        let mut maximum = 0;
        for entry in entries {
            let entry = entry.map_err(|_| KeyError::Io {
                operation: "list key records",
            })?;
            let path = entry.path();
            if path.extension().and_then(|extension| extension.to_str()) == Some("jck") {
                let record = read_record(&path)?;
                maximum = maximum.max(record.metadata.version);
            }
        }
        Ok(maximum.saturating_add(1))
    }

    fn publish(&self, key_id: &str, record: &KeyRecord) -> Result<(), KeyError> {
        let target = self.path_for(key_id);
        let mut temporary =
            tempfile::NamedTempFile::new_in(&self.root).map_err(|_| KeyError::Io {
                operation: "create temporary key record",
            })?;
        set_restricted_file_permissions(temporary.as_file())?;
        let encoded = encode_record(record)?;
        temporary
            .as_file_mut()
            .write_all(&encoded)
            .and_then(|()| temporary.as_file().sync_all())
            .map_err(|_| KeyError::Io {
                operation: "sync temporary key record",
            })?;
        match temporary.persist_noclobber(&target) {
            Ok(file) => {
                drop(file);
                sync_directory(&self.root).map_err(|_| KeyError::Io {
                    operation: "sync key directory",
                })?;
                Ok(())
            }
            Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => {
                Err(KeyError::InvalidKeyId)
            }
            Err(_) => Err(KeyError::Io {
                operation: "publish key record",
            }),
        }
    }
}

impl KeyProvider for FileKeyProvider {
    fn get_key(&self, key_id: &str) -> Result<Zeroizing<[u8; 32]>, KeyError> {
        validate_key_id(key_id)?;
        Ok(read_record(&self.path_for(key_id))?.key)
    }

    fn metadata(&self, key_id: &str) -> Result<KeyRotationMetadata, KeyError> {
        validate_key_id(key_id)?;
        Ok(read_record(&self.path_for(key_id))?.metadata)
    }
}

struct KeyRecord {
    metadata: KeyRotationMetadata,
    key: Zeroizing<[u8; 32]>,
}

fn validate_key_id(key_id: &str) -> Result<(), KeyError> {
    if key_id.is_empty()
        || key_id == "."
        || key_id == ".."
        || key_id.len() > MAX_KEY_ID_BYTES
        || !key_id.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_alphanumeric() || (index > 0 && matches!(byte, b'.' | b'_' | b'-'))
        })
    {
        return Err(KeyError::InvalidKeyId);
    }
    Ok(())
}

fn ensure_root(root: &Path) -> Result<(), KeyError> {
    match fs::symlink_metadata(root) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.file_type().is_dir() {
                return Err(KeyError::InvalidStore);
            }
            ensure_restricted_directory(root)?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir_all(root).map_err(|_| KeyError::Io {
                operation: "create key store root",
            })?;
            set_restricted_directory_permissions(root)?;
        }
        Err(_) => {
            return Err(KeyError::Io {
                operation: "inspect key store root",
            });
        }
    }
    Ok(())
}

fn read_record(path: &Path) -> Result<KeyRecord, KeyError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            KeyError::NotFound(
                path.file_stem()
                    .and_then(|stem| stem.to_str())
                    .unwrap_or_default()
                    .to_owned(),
            )
        } else {
            KeyError::Io {
                operation: "inspect key record",
            }
        }
    })?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
        return Err(KeyError::CorruptRecord);
    }
    let mut file = OpenOptions::new()
        .read(true)
        .open(path)
        .map_err(|_| KeyError::Io {
            operation: "open key record",
        })?;
    ensure_restricted_file(path, &file)?;
    let mut bytes = Zeroizing::new(Vec::new());
    file.read_to_end(&mut bytes).map_err(|_| KeyError::Io {
        operation: "read key record",
    })?;
    let record = decode_record(&bytes)?;
    let filename_id = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .ok_or(KeyError::CorruptRecord)?;
    if record.metadata.key_id != filename_id {
        return Err(KeyError::CorruptRecord);
    }
    Ok(record)
}

fn encode_record(record: &KeyRecord) -> Result<Zeroizing<Vec<u8>>, KeyError> {
    let key_id = record.metadata.key_id.as_bytes();
    let rotated_from = record
        .metadata
        .rotated_from
        .as_deref()
        .unwrap_or("")
        .as_bytes();
    let key_id_len = u16::try_from(key_id.len()).map_err(|_| KeyError::InvalidKeyId)?;
    let rotated_len = u16::try_from(rotated_from.len()).map_err(|_| KeyError::InvalidKeyId)?;
    let mut encoded = Zeroizing::new(Vec::with_capacity(
        4 + 1 + 2 + 8 + 8 + 4 + 2 + key_id.len() + 2 + rotated_from.len() + 32,
    ));
    encoded.extend_from_slice(FILE_MAGIC);
    encoded.push(FILE_VERSION);
    encoded.extend_from_slice(&key_id_len.to_be_bytes());
    encoded.extend_from_slice(&record.metadata.version.to_be_bytes());
    encoded.extend_from_slice(&record.metadata.created_at.timestamp().to_be_bytes());
    encoded.extend_from_slice(
        &record
            .metadata
            .created_at
            .timestamp_subsec_nanos()
            .to_be_bytes(),
    );
    encoded.extend_from_slice(&rotated_len.to_be_bytes());
    encoded.extend_from_slice(key_id);
    encoded.extend_from_slice(rotated_from);
    encoded.extend_from_slice(&*record.key);
    Ok(encoded)
}

fn decode_record(bytes: &[u8]) -> Result<KeyRecord, KeyError> {
    let mut cursor = Cursor::new(bytes);
    if cursor.take(4)? != FILE_MAGIC || cursor.take_u8()? != FILE_VERSION {
        return Err(KeyError::CorruptRecord);
    }
    let key_id_len = usize::from(cursor.take_u16()?);
    let version = cursor.take_u64()?;
    let seconds = cursor.take_i64()?;
    let nanos = cursor.take_u32()?;
    let rotated_len = usize::from(cursor.take_u16()?);
    let key_id = String::from_utf8(cursor.take(key_id_len)?.to_vec())
        .map_err(|_| KeyError::CorruptRecord)?;
    let rotated = String::from_utf8(cursor.take(rotated_len)?.to_vec())
        .map_err(|_| KeyError::CorruptRecord)?;
    validate_key_id(&key_id)?;
    let rotated_from = if rotated.is_empty() {
        None
    } else {
        validate_key_id(&rotated)?;
        Some(rotated)
    };
    let created_at = chrono::TimeZone::timestamp_opt(&Utc, seconds, nanos)
        .single()
        .ok_or(KeyError::CorruptRecord)?;
    let key_slice = cursor.take(32)?;
    if !cursor.is_empty() {
        return Err(KeyError::CorruptRecord);
    }
    let mut key = [0; 32];
    key.copy_from_slice(key_slice);
    Ok(KeyRecord {
        metadata: KeyRotationMetadata {
            key_id,
            version,
            rotated_from,
            created_at,
        },
        key: Zeroizing::new(key),
    })
}

struct Cursor<'a> {
    bytes: &'a [u8],
}

impl<'a> Cursor<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes }
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], KeyError> {
        if self.bytes.len() < length {
            return Err(KeyError::CorruptRecord);
        }
        let (head, tail) = self.bytes.split_at(length);
        self.bytes = tail;
        Ok(head)
    }

    fn take_u8(&mut self) -> Result<u8, KeyError> {
        Ok(self.take(1)?[0])
    }

    fn take_u16(&mut self) -> Result<u16, KeyError> {
        Ok(u16::from_be_bytes(
            self.take(2)?
                .try_into()
                .map_err(|_| KeyError::CorruptRecord)?,
        ))
    }

    fn take_u32(&mut self) -> Result<u32, KeyError> {
        Ok(u32::from_be_bytes(
            self.take(4)?
                .try_into()
                .map_err(|_| KeyError::CorruptRecord)?,
        ))
    }

    fn take_u64(&mut self) -> Result<u64, KeyError> {
        Ok(u64::from_be_bytes(
            self.take(8)?
                .try_into()
                .map_err(|_| KeyError::CorruptRecord)?,
        ))
    }

    fn take_i64(&mut self) -> Result<i64, KeyError> {
        Ok(i64::from_be_bytes(
            self.take(8)?
                .try_into()
                .map_err(|_| KeyError::CorruptRecord)?,
        ))
    }

    const fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
}

fn sync_directory(path: &Path) -> Result<(), std::io::Error> {
    #[cfg(unix)]
    {
        File::open(path)?.sync_all()
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(())
    }
}

fn ensure_restricted_directory(path: &Path) -> Result<(), KeyError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = fs::metadata(path)
            .map_err(|_| KeyError::Io {
                operation: "inspect key store permissions",
            })?
            .permissions()
            .mode();
        if mode & 0o077 != 0 {
            return Err(KeyError::PermissionDenied);
        }
    }
    let _ = path;
    Ok(())
}

fn set_restricted_directory_permissions(path: &Path) -> Result<(), KeyError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|_| KeyError::Io {
            operation: "set key store permissions",
        })?;
    }
    let _ = path;
    Ok(())
}

fn ensure_restricted_file(path: &Path, file: &File) -> Result<(), KeyError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = file
            .metadata()
            .map_err(|_| KeyError::Io {
                operation: "inspect key file permissions",
            })?
            .permissions()
            .mode();
        if mode & 0o077 != 0 {
            return Err(KeyError::PermissionDenied);
        }
    }
    let _ = path;
    Ok(())
}

fn set_restricted_file_permissions(file: &File) -> Result<(), KeyError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|_| KeyError::Io {
                operation: "set key file permissions",
            })?;
    }
    Ok(())
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
