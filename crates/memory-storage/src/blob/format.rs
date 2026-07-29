use crate::BlobError;

pub(crate) const FLAG_ZSTD: u8 = 0b0000_0001;
pub(crate) const FLAG_AES_GCM: u8 = 0b0000_0010;
pub(crate) const AES_GCM_NONCE_LEN: usize = 12;
pub(crate) const CHECKSUM_LEN: usize = 32;

const MAGIC: &[u8; 4] = b"JCB1";
const FIXED_HEADER_LEN: usize = 14;
const KNOWN_FLAGS: u8 = FLAG_ZSTD | FLAG_AES_GCM;

pub(crate) struct Envelope<'a> {
    pub flags: u8,
    pub original_len: u64,
    pub nonce: &'a [u8],
    pub payload: &'a [u8],
    pub checksum: [u8; CHECKSUM_LEN],
    pub authenticated_header: &'a [u8],
}

pub(crate) fn build_header(flags: u8, original_len: u64, nonce: &[u8]) -> Vec<u8> {
    let mut header = Vec::with_capacity(FIXED_HEADER_LEN + nonce.len());
    header.extend_from_slice(MAGIC);
    header.push(flags);
    header.push(u8::try_from(nonce.len()).expect("JCB1 nonce length fits in one byte"));
    header.extend_from_slice(&original_len.to_be_bytes());
    header.extend_from_slice(nonce);
    header
}

pub(crate) fn parse(encoded: &[u8]) -> Result<Envelope<'_>, BlobError> {
    if encoded.len() < FIXED_HEADER_LEN + CHECKSUM_LEN {
        return Err(BlobError::InvalidHeader {
            reason: "envelope is truncated",
        });
    }
    if &encoded[0..MAGIC.len()] != MAGIC {
        return Err(BlobError::InvalidHeader {
            reason: "magic is not JCB1",
        });
    }

    let flags = encoded[4];
    if flags & !KNOWN_FLAGS != 0 {
        return Err(BlobError::UnsupportedFlags { flags });
    }

    let nonce_len = usize::from(encoded[5]);
    let encrypted = flags & FLAG_AES_GCM != 0;
    if (encrypted && nonce_len != AES_GCM_NONCE_LEN) || (!encrypted && nonce_len != 0) {
        return Err(BlobError::InvalidHeader {
            reason: "nonce length does not match encryption flags",
        });
    }

    let header_end = FIXED_HEADER_LEN
        .checked_add(nonce_len)
        .ok_or(BlobError::InvalidHeader {
            reason: "header length overflow",
        })?;
    let payload_end = encoded
        .len()
        .checked_sub(CHECKSUM_LEN)
        .ok_or(BlobError::InvalidHeader {
            reason: "checksum footer is truncated",
        })?;
    if header_end > payload_end {
        return Err(BlobError::InvalidHeader {
            reason: "nonce or payload is truncated",
        });
    }

    let original_len =
        u64::from_be_bytes(encoded[6..FIXED_HEADER_LEN].try_into().map_err(|_| {
            BlobError::InvalidHeader {
                reason: "original length is truncated",
            }
        })?);
    let checksum = encoded[payload_end..]
        .try_into()
        .map_err(|_| BlobError::InvalidHeader {
            reason: "checksum footer is truncated",
        })?;

    Ok(Envelope {
        flags,
        original_len,
        nonce: &encoded[FIXED_HEADER_LEN..header_end],
        payload: &encoded[header_end..payload_end],
        checksum,
        authenticated_header: &encoded[..header_end],
    })
}
