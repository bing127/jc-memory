mod codec;
mod format;
mod store;

use std::fmt;

use jc_memory_protocol::BlobId;
use zeroize::Zeroizing;

pub use store::BlobStore;

/// Compression applied before optional blob encryption.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Compression {
    /// Store the plaintext without compression.
    None,
    /// Compress with Zstandard at the supplied level.
    Zstd {
        /// Zstandard compression level.
        level: i32,
    },
}

/// Encryption applied after optional blob compression.
#[derive(Clone)]
pub enum Encryption {
    /// Store the payload without encryption.
    None,
    /// Encrypt with AES-256-GCM and a fresh nonce per write.
    Aes256Gcm {
        /// Secret 256-bit encryption key, zeroized when dropped.
        key: Zeroizing<[u8; 32]>,
    },
}

impl fmt::Debug for Encryption {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => formatter.write_str("None"),
            Self::Aes256Gcm { .. } => formatter
                .debug_struct("Aes256Gcm")
                .field("key", &"[REDACTED]")
                .finish(),
        }
    }
}

/// Encoding and resource limits for a [`BlobStore`].
#[derive(Clone, Debug)]
pub struct BlobOptions {
    /// Compression applied before persistence.
    pub compression: Compression,
    /// Encryption applied after compression.
    pub encryption: Encryption,
    /// Maximum accepted plaintext size in bytes.
    pub max_blob_bytes: u64,
}

impl BlobOptions {
    /// Returns uncompressed, unencrypted options with a 64 MiB limit.
    #[must_use]
    pub const fn plain() -> Self {
        Self {
            compression: Compression::None,
            encryption: Encryption::None,
            max_blob_bytes: 64 * 1024 * 1024,
        }
    }
}

/// Result of publishing or verifying a content-addressed blob.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlobReceipt {
    /// SHA-256 of the original plaintext.
    pub id: BlobId,
    /// Encoded file size on disk.
    pub stored_bytes: u64,
    /// Original plaintext size.
    pub original_bytes: u64,
    /// Whether this operation published a new file.
    pub created: bool,
}

/// Failures produced by content-addressed blob storage.
#[derive(Debug, thiserror::Error)]
pub enum BlobError {
    /// The configured resource limits or codec options are invalid.
    #[error("invalid blob configuration: {reason}")]
    InvalidOptions {
        /// Human-readable reason the options were rejected.
        reason: &'static str,
    },
    /// The plaintext or encoded file exceeds its configured limit.
    #[error("blob size {actual} exceeds limit {limit}")]
    TooLarge {
        /// Observed byte count.
        actual: u64,
        /// Maximum permitted byte count.
        limit: u64,
    },
    /// A requested content identifier has no published file.
    #[error("blob {id} was not found")]
    NotFound {
        /// Missing plaintext content identifier.
        id: BlobId,
    },
    /// The stable `JCB1` envelope is malformed.
    #[error("invalid JCB1 header: {reason}")]
    InvalidHeader {
        /// Static description of the rejected header condition.
        reason: &'static str,
    },
    /// The envelope uses flag bits unknown to this implementation.
    #[error("unsupported JCB1 flags: {flags:#04x}")]
    UnsupportedFlags {
        /// Raw flags byte from the envelope.
        flags: u8,
    },
    /// Authenticated encryption failed while writing.
    #[error("blob encryption failed")]
    EncryptionFailed,
    /// The operating system could not provide a fresh encryption nonce.
    #[error("secure randomness is unavailable")]
    RandomnessUnavailable,
    /// Authenticated decryption failed without exposing key or content data.
    #[error("blob decryption failed")]
    DecryptionFailed,
    /// Decoded plaintext length differs from the authenticated envelope.
    #[error("blob length mismatch: expected {expected}, decoded {actual}")]
    LengthMismatch {
        /// Original length recorded by the envelope.
        expected: u64,
        /// Actual decoded plaintext length.
        actual: u64,
    },
    /// Decoded plaintext does not match its content identifier.
    #[error("blob checksum mismatch: expected {expected}, decoded {actual}")]
    ChecksumMismatch {
        /// Identifier required by the path or envelope.
        expected: BlobId,
        /// Identifier computed from decoded plaintext or the envelope footer.
        actual: BlobId,
    },
    /// Compression, decompression, or filesystem access failed.
    #[error("blob I/O failed: {0}")]
    Io(#[from] std::io::Error),
}
