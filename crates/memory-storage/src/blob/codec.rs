use std::{io::Read as _, str::FromStr as _};

use aes_gcm::{
    Aes256Gcm, KeyInit as _, Nonce,
    aead::{Aead as _, Payload},
};
use jc_memory_protocol::BlobId;
use sha2::{Digest as _, Sha256};

use super::{
    BlobError, BlobOptions, Compression, Encryption,
    format::{AES_GCM_NONCE_LEN, CHECKSUM_LEN, FLAG_AES_GCM, FLAG_ZSTD, build_header, parse},
};

pub(crate) fn encode(plaintext: &[u8], options: &BlobOptions) -> Result<Vec<u8>, BlobError> {
    let original_len = u64::try_from(plaintext.len()).map_err(|_| BlobError::TooLarge {
        actual: u64::MAX,
        limit: options.max_blob_bytes,
    })?;
    let mut flags = 0;
    let mut payload = match options.compression {
        Compression::None => plaintext.to_vec(),
        Compression::Zstd { level } => {
            flags |= FLAG_ZSTD;
            zstd::stream::encode_all(plaintext, level)?
        }
    };

    let mut nonce = Vec::new();
    if matches!(options.encryption, Encryption::Aes256Gcm { .. }) {
        flags |= FLAG_AES_GCM;
        nonce.resize(AES_GCM_NONCE_LEN, 0);
        getrandom::fill(&mut nonce).map_err(|_| BlobError::RandomnessUnavailable)?;
    }

    let header = build_header(flags, original_len, &nonce);
    if let Encryption::Aes256Gcm { key } = &options.encryption {
        let cipher =
            Aes256Gcm::new_from_slice(key.as_ref()).map_err(|_| BlobError::EncryptionFailed)?;
        let nonce = Nonce::try_from(nonce.as_slice()).map_err(|_| BlobError::EncryptionFailed)?;
        payload = cipher
            .encrypt(
                &nonce,
                Payload {
                    msg: &payload,
                    aad: &header,
                },
            )
            .map_err(|_| BlobError::EncryptionFailed)?;
    }

    let checksum = Sha256::digest(plaintext);
    let mut encoded = Vec::with_capacity(header.len() + payload.len() + CHECKSUM_LEN);
    encoded.extend_from_slice(&header);
    encoded.extend_from_slice(&payload);
    encoded.extend_from_slice(&checksum);
    Ok(encoded)
}

pub(crate) fn decode(
    encoded: &[u8],
    expected_id: &BlobId,
    options: &BlobOptions,
) -> Result<Vec<u8>, BlobError> {
    let envelope = parse(encoded)?;
    if envelope.original_len > options.max_blob_bytes {
        return Err(BlobError::TooLarge {
            actual: envelope.original_len,
            limit: options.max_blob_bytes,
        });
    }

    let footer_id = id_from_digest(envelope.checksum);
    if &footer_id != expected_id {
        return Err(BlobError::ChecksumMismatch {
            expected: expected_id.clone(),
            actual: footer_id,
        });
    }

    let mut payload = if envelope.flags & FLAG_AES_GCM == 0 {
        envelope.payload.to_vec()
    } else if let Encryption::Aes256Gcm { key } = &options.encryption {
        let cipher =
            Aes256Gcm::new_from_slice(key.as_ref()).map_err(|_| BlobError::DecryptionFailed)?;
        let nonce = Nonce::try_from(envelope.nonce).map_err(|_| BlobError::DecryptionFailed)?;
        cipher
            .decrypt(
                &nonce,
                Payload {
                    msg: envelope.payload,
                    aad: envelope.authenticated_header,
                },
            )
            .map_err(|_| BlobError::DecryptionFailed)?
    } else {
        return Err(BlobError::DecryptionFailed);
    };

    if envelope.flags & FLAG_ZSTD != 0 {
        let decoder = zstd::stream::read::Decoder::new(payload.as_slice())?;
        let mut limited = decoder.take(options.max_blob_bytes.saturating_add(1));
        let mut decompressed = Vec::new();
        limited.read_to_end(&mut decompressed)?;
        payload = decompressed;
    }

    let actual_len = u64::try_from(payload.len()).unwrap_or(u64::MAX);
    if actual_len != envelope.original_len {
        return Err(BlobError::LengthMismatch {
            expected: envelope.original_len,
            actual: actual_len,
        });
    }

    let actual_id = id_from_digest(Sha256::digest(&payload).into());
    if &actual_id != expected_id {
        return Err(BlobError::ChecksumMismatch {
            expected: expected_id.clone(),
            actual: actual_id,
        });
    }

    Ok(payload)
}

pub(crate) fn plaintext_id(plaintext: &[u8]) -> BlobId {
    id_from_digest(Sha256::digest(plaintext).into())
}

fn id_from_digest(digest: [u8; CHECKSUM_LEN]) -> BlobId {
    BlobId::from_str(&hex::encode(digest)).expect("SHA-256 always forms a valid blob identifier")
}
