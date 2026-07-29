use std::path::{Path, PathBuf};

use jc_memory_protocol::BlobId;
use tokio::io::AsyncWriteExt as _;

use super::{BlobError, BlobOptions, BlobReceipt, codec};

const ENCODED_SIZE_ALLOWANCE: u64 = 1024 * 1024;

/// Filesystem-backed, content-addressed storage for immutable blob content.
#[derive(Clone, Debug)]
pub struct BlobStore {
    root: PathBuf,
    options: BlobOptions,
}

impl BlobStore {
    /// Opens or creates a blob root with the supplied codec options.
    ///
    /// # Errors
    ///
    /// Returns [`BlobError`] when the root cannot be created or the size limit is invalid.
    pub async fn open(root: impl AsRef<Path>, options: BlobOptions) -> Result<Self, BlobError> {
        if options.max_blob_bytes == 0 {
            return Err(BlobError::InvalidOptions {
                reason: "max_blob_bytes must be greater than zero",
            });
        }
        tokio::fs::create_dir_all(root.as_ref()).await?;
        Ok(Self {
            root: root.as_ref().to_path_buf(),
            options,
        })
    }

    /// Atomically publishes plaintext under its SHA-256 content identifier.
    ///
    /// Existing files are decoded and verified before an idempotent receipt is returned.
    ///
    /// # Errors
    ///
    /// Returns [`BlobError`] for oversized input, codec failures, corrupt existing content, or
    /// filesystem failures.
    pub async fn put(&self, plaintext: &[u8]) -> Result<BlobReceipt, BlobError> {
        let original_bytes = checked_len(plaintext.len(), self.options.max_blob_bytes)?;
        let id = codec::plaintext_id(plaintext);
        let target = self.path_for(&id);

        match tokio::fs::metadata(&target).await {
            Ok(_) => return self.existing_receipt(&id).await,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }

        let encoded = codec::encode(plaintext, &self.options)?;
        let stored_bytes = u64::try_from(encoded.len()).unwrap_or(u64::MAX);
        let parent = target.parent().ok_or(BlobError::InvalidOptions {
            reason: "blob root cannot produce a shard directory",
        })?;
        tokio::fs::create_dir_all(parent).await?;

        let temporary = tempfile::NamedTempFile::new_in(parent)?;
        let mut output = tokio::fs::File::from_std(temporary.reopen()?);
        output.write_all(&encoded).await?;
        output.sync_all().await?;
        drop(output);

        match temporary.persist_noclobber(&target) {
            Ok(file) => {
                drop(file);
                sync_directory(parent).await?;
                Ok(BlobReceipt {
                    id,
                    stored_bytes,
                    original_bytes,
                    created: true,
                })
            }
            Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => {
                drop(error.file);
                self.existing_receipt(&id).await
            }
            Err(error) => Err(error.error.into()),
        }
    }

    /// Reads, decodes, and verifies the blob identified by `id`.
    ///
    /// # Errors
    ///
    /// Returns [`BlobError`] when the blob is missing, malformed, encoded with an unavailable
    /// key, corrupt, oversized, or unreadable.
    pub async fn get(&self, id: &BlobId) -> Result<Vec<u8>, BlobError> {
        let path = self.path_for(id);
        let metadata = match tokio::fs::metadata(&path).await {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(BlobError::NotFound { id: id.clone() });
            }
            Err(error) => return Err(error.into()),
        };
        let encoded_limit = self
            .options
            .max_blob_bytes
            .saturating_add(ENCODED_SIZE_ALLOWANCE);
        if metadata.len() > encoded_limit {
            return Err(BlobError::TooLarge {
                actual: metadata.len(),
                limit: encoded_limit,
            });
        }
        let encoded = tokio::fs::read(path).await?;
        codec::decode(&encoded, id, &self.options)
    }

    /// Verifies a blob and returns its current on-disk sizes.
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`Self::get`] or when file metadata cannot be read.
    pub async fn verify(&self, id: &BlobId) -> Result<BlobReceipt, BlobError> {
        let plaintext = self.get(id).await?;
        let metadata = tokio::fs::metadata(self.path_for(id)).await?;
        Ok(BlobReceipt {
            id: id.clone(),
            stored_bytes: metadata.len(),
            original_bytes: u64::try_from(plaintext.len()).unwrap_or(u64::MAX),
            created: false,
        })
    }

    async fn existing_receipt(&self, id: &BlobId) -> Result<BlobReceipt, BlobError> {
        self.verify(id).await
    }

    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    pub(crate) fn path_for(&self, id: &BlobId) -> PathBuf {
        let digest = id.as_str();
        self.root
            .join(&digest[0..2])
            .join(&digest[2..4])
            .join(format!("{digest}.jcb"))
    }
}

fn checked_len(length: usize, limit: u64) -> Result<u64, BlobError> {
    let actual = u64::try_from(length).unwrap_or(u64::MAX);
    if actual > limit {
        Err(BlobError::TooLarge { actual, limit })
    } else {
        Ok(actual)
    }
}

#[cfg(unix)]
async fn sync_directory(path: &Path) -> Result<(), std::io::Error> {
    tokio::fs::File::open(path).await?.sync_all().await
}

#[cfg(not(unix))]
async fn sync_directory(_path: &Path) -> Result<(), std::io::Error> {
    Ok(())
}
