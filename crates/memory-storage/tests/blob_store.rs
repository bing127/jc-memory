//! Content-addressed blob storage contract tests.

use std::{path::Path, str::FromStr, sync::Arc};

use jc_memory_protocol::BlobId;
use jc_memory_storage::{BlobError, BlobOptions, BlobStore, Compression, Encryption};
use sha2::{Digest as _, Sha256};
use tempfile::TempDir;
use tokio::sync::Barrier;
use zeroize::Zeroizing;

const KEY: [u8; 32] = [0x31; 32];
const OTHER_KEY: [u8; 32] = [0x73; 32];

#[tokio::test]
async fn blob_id_is_plaintext_sha256_and_round_trips_all_codecs() {
    let bytes = b"immutable evidence";

    for options in codec_options() {
        let fixture = Fixture::new(options).await;
        let receipt = fixture.store.put(bytes).await.expect("store blob");

        assert_eq!(receipt.id.as_str(), hex::encode(Sha256::digest(bytes)));
        assert_eq!(receipt.original_bytes, bytes.len() as u64);
        assert!(receipt.created);
        assert_eq!(
            fixture.store.get(&receipt.id).await.expect("read blob"),
            bytes
        );
    }
}

#[tokio::test]
async fn encoded_file_uses_the_stable_jcb1_header() {
    let fixture = Fixture::new(BlobOptions {
        compression: Compression::Zstd { level: 3 },
        encryption: Encryption::Aes256Gcm {
            key: Zeroizing::new(KEY),
        },
        max_blob_bytes: 1024,
    })
    .await;
    let plaintext = b"header contract";
    let receipt = fixture.store.put(plaintext).await.expect("store blob");
    let encoded = tokio::fs::read(fixture.path(&receipt.id))
        .await
        .expect("read encoded blob");

    assert_eq!(&encoded[0..4], b"JCB1");
    assert_eq!(encoded[4], 0b0000_0011);
    assert_eq!(encoded[5], 12);
    assert_eq!(
        u64::from_be_bytes(encoded[6..14].try_into().expect("read length")),
        plaintext.len() as u64
    );
}

#[tokio::test]
async fn repeated_writes_are_idempotent_and_verify_the_existing_blob() {
    let fixture = Fixture::new(BlobOptions::plain()).await;
    let first = fixture
        .store
        .put(b"same evidence")
        .await
        .expect("first put");
    let second = fixture
        .store
        .put(b"same evidence")
        .await
        .expect("second put");
    let verified = fixture.store.verify(&first.id).await.expect("verify blob");

    assert!(first.created);
    assert!(!second.created);
    assert!(!verified.created);
    assert_eq!(first.id, second.id);
    assert_eq!(first.stored_bytes, second.stored_bytes);
    assert_eq!(first.original_bytes, verified.original_bytes);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_writes_publish_exactly_one_valid_blob() {
    let fixture = Fixture::new(BlobOptions::plain()).await;
    let store = Arc::new(fixture.store);
    let barrier = Arc::new(Barrier::new(8));
    let mut tasks = Vec::new();

    for _ in 0..8 {
        let store = Arc::clone(&store);
        let barrier = Arc::clone(&barrier);
        tasks.push(tokio::spawn(async move {
            barrier.wait().await;
            store.put(b"concurrent immutable blob").await
        }));
    }

    let mut receipts = Vec::new();
    for task in tasks {
        receipts.push(task.await.expect("join put task").expect("put blob"));
    }

    assert_eq!(receipts.iter().filter(|receipt| receipt.created).count(), 1);
    assert!(receipts.windows(2).all(|pair| pair[0].id == pair[1].id));
    assert_eq!(
        store.get(&receipts[0].id).await.expect("read blob"),
        b"concurrent immutable blob"
    );
}

#[tokio::test]
async fn tampered_plaintext_blob_is_rejected() {
    let fixture = Fixture::new(BlobOptions::plain()).await;
    let receipt = fixture.store.put(b"evidence").await.expect("store blob");
    let path = fixture.path(&receipt.id);
    let mut encoded = tokio::fs::read(&path).await.expect("read blob file");
    encoded[14] ^= 0x01;
    tokio::fs::write(path, encoded)
        .await
        .expect("tamper blob file");

    assert!(matches!(
        fixture.store.get(&receipt.id).await,
        Err(BlobError::ChecksumMismatch { .. })
    ));
}

#[tokio::test]
async fn encrypted_blob_rejects_the_wrong_key_without_exposing_plaintext() {
    let directory = tempfile::tempdir().expect("create blob temp directory");
    let writer = BlobStore::open(directory.path(), encrypted_options(KEY))
        .await
        .expect("open writer");
    let receipt = writer
        .put(b"private evidence")
        .await
        .expect("store encrypted blob");
    let wrong_reader = BlobStore::open(directory.path(), encrypted_options(OTHER_KEY))
        .await
        .expect("open reader");

    let error = wrong_reader
        .get(&receipt.id)
        .await
        .expect_err("wrong key must fail");
    assert!(matches!(error, BlobError::DecryptionFailed));
    assert!(!error.to_string().contains("private evidence"));
}

#[tokio::test]
async fn temporary_and_missing_files_are_never_treated_as_blobs() {
    let fixture = Fixture::new(BlobOptions::plain()).await;
    let id = blob_id(b"not published");
    let target = fixture.path(&id);
    let parent = target.parent().expect("blob parent");
    tokio::fs::create_dir_all(parent)
        .await
        .expect("create shard directory");
    tokio::fs::write(parent.join(format!("{}.tmp", id.as_str())), b"partial")
        .await
        .expect("write temporary blob");

    assert!(matches!(
        fixture.store.get(&id).await,
        Err(BlobError::NotFound { .. })
    ));
}

#[tokio::test]
async fn zero_byte_blobs_round_trip() {
    let fixture = Fixture::new(BlobOptions::plain()).await;
    let receipt = fixture.store.put(&[]).await.expect("store empty blob");

    assert_eq!(receipt.original_bytes, 0);
    assert!(
        fixture
            .store
            .get(&receipt.id)
            .await
            .expect("read blob")
            .is_empty()
    );
}

#[tokio::test]
async fn oversized_input_is_rejected_before_writing() {
    let fixture = Fixture::new(BlobOptions {
        max_blob_bytes: 4,
        ..BlobOptions::plain()
    })
    .await;

    assert!(matches!(
        fixture.store.put(b"12345").await,
        Err(BlobError::TooLarge {
            actual: 5,
            limit: 4
        })
    ));
}

#[tokio::test]
async fn malformed_headers_are_rejected() {
    let fixture = Fixture::new(BlobOptions::plain()).await;
    let receipt = fixture.store.put(b"header").await.expect("store blob");
    let path = fixture.path(&receipt.id);
    tokio::fs::write(&path, b"JCB1")
        .await
        .expect("truncate blob");
    assert!(matches!(
        fixture.store.get(&receipt.id).await,
        Err(BlobError::InvalidHeader { .. })
    ));

    let replacement = fixture
        .store
        .put(b"different header")
        .await
        .expect("store replacement blob");
    let replacement_path = fixture.path(&replacement.id);
    let mut encoded = tokio::fs::read(&replacement_path)
        .await
        .expect("read replacement blob");
    encoded[4] = 0x80;
    tokio::fs::write(replacement_path, encoded)
        .await
        .expect("write unsupported flags");
    assert!(matches!(
        fixture.store.get(&replacement.id).await,
        Err(BlobError::UnsupportedFlags { flags: 0x80 })
    ));
}

struct Fixture {
    _directory: TempDir,
    root: std::path::PathBuf,
    store: BlobStore,
}

impl Fixture {
    async fn new(options: BlobOptions) -> Self {
        let directory = tempfile::tempdir().expect("create blob temp directory");
        let root = directory.path().to_path_buf();
        let store = BlobStore::open(&root, options)
            .await
            .expect("open blob store");
        Self {
            _directory: directory,
            root,
            store,
        }
    }

    fn path(&self, id: &BlobId) -> std::path::PathBuf {
        blob_path(&self.root, id)
    }
}

fn codec_options() -> Vec<BlobOptions> {
    vec![
        BlobOptions::plain(),
        BlobOptions {
            compression: Compression::Zstd { level: 3 },
            ..BlobOptions::plain()
        },
        encrypted_options(KEY),
        BlobOptions {
            compression: Compression::Zstd { level: 3 },
            encryption: Encryption::Aes256Gcm {
                key: Zeroizing::new(KEY),
            },
            max_blob_bytes: 1024,
        },
    ]
}

fn encrypted_options(key: [u8; 32]) -> BlobOptions {
    BlobOptions {
        compression: Compression::None,
        encryption: Encryption::Aes256Gcm {
            key: Zeroizing::new(key),
        },
        max_blob_bytes: 1024,
    }
}

fn blob_id(bytes: &[u8]) -> BlobId {
    BlobId::from_str(&hex::encode(Sha256::digest(bytes))).expect("valid blob digest")
}

fn blob_path(root: &Path, id: &BlobId) -> std::path::PathBuf {
    let digest = id.as_str();
    root.join(&digest[0..2])
        .join(&digest[2..4])
        .join(format!("{digest}.jcb"))
}
