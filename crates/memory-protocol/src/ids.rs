use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};
use thiserror::Error;
use uuid::Uuid;

/// An error produced while parsing a strongly typed identifier.
#[derive(Debug, Error)]
pub enum IdParseError {
    /// The value is not a valid UUID.
    #[error("invalid UUID: {0}")]
    Uuid(#[from] uuid::Error),
    /// A blob identifier is not a lowercase SHA-256 digest.
    #[error("blob identifier must be exactly 64 lowercase hexadecimal characters")]
    InvalidBlobDigest,
}

macro_rules! uuid_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            /// Creates a time-ordered UUID v7 identifier.
            #[must_use]
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }

            /// Returns the underlying UUID.
            #[must_use]
            pub const fn as_uuid(&self) -> &Uuid {
                &self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }

        impl FromStr for $name {
            type Err = IdParseError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Ok(Self(Uuid::parse_str(value)?))
            }
        }
    };
}

uuid_id!(/// Identifies a cognitive fragment.
    FragmentId);
uuid_id!(/// Identifies an immutable memory event.
    EventId);
uuid_id!(/// Identifies an isolated memory namespace.
    NamespaceId);
uuid_id!(/// Identifies a tenant within a namespace.
    TenantId);
uuid_id!(/// Identifies a user.
    UserId);
uuid_id!(/// Identifies an agent.
    AgentId);
uuid_id!(/// Identifies a project.
    ProjectId);
uuid_id!(/// Identifies a session.
    SessionId);
uuid_id!(/// Identifies a distributed trace.
    TraceId);
uuid_id!(/// Identifies an evidence reference.
    EvidenceId);

/// A lowercase SHA-256 digest identifying plaintext blob content.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(try_from = "String", into = "String")]
pub struct BlobId(String);

impl BlobId {
    /// Returns the lowercase hexadecimal digest.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for BlobId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl FromStr for BlobId {
    type Err = IdParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::try_from(value.to_owned())
    }
}

impl TryFrom<String> for BlobId {
    type Error = IdParseError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let valid = value.len() == 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
        if valid {
            Ok(Self(value))
        } else {
            Err(IdParseError::InvalidBlobDigest)
        }
    }
}

impl From<BlobId> for String {
    fn from(value: BlobId) -> Self {
        value.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_ids_do_not_lose_their_wire_value() {
        let id = NamespaceId::new();
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(serde_json::from_str::<NamespaceId>(&json).unwrap(), id);
        assert_eq!(NamespaceId::from_str(&id.to_string()).unwrap(), id);
    }

    #[test]
    fn blob_id_accepts_only_lowercase_sha256() {
        let valid = "a".repeat(64);
        assert_eq!(BlobId::from_str(&valid).unwrap().as_str(), valid);
        assert!(BlobId::from_str(&"A".repeat(64)).is_err());
        assert!(BlobId::from_str("abc").is_err());
        assert!(BlobId::from_str(&"z".repeat(64)).is_err());
    }
}
