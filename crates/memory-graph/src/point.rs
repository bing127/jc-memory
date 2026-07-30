//! Atomic cognitive graph points.

use chrono::{DateTime, Utc};
use jc_memory_protocol::{
    CognitiveFragment, FragmentId, MemoryScope, PointCreationError, PointId, PointKind,
    ValidityStatus,
};
use serde::{Deserialize, Serialize};

/// A graph point derived from one or more authoritative fragments.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct CognitivePoint {
    /// Point identifier.
    pub id: PointId,
    /// Ownership boundary inherited from the source fragment.
    pub scope: MemoryScope,
    /// Atomic change category.
    pub kind: PointKind,
    /// Authoritative fragments supporting this derived point.
    pub source_fragments: Vec<FragmentId>,
    /// When the represented change occurred.
    pub occurred_at: DateTime<Utc>,
    /// Confidence inherited from the source fragment.
    pub confidence: f32,
    /// Current validity inherited from the source fragment.
    pub validity: ValidityStatus,
}

impl TryFrom<&CognitiveFragment> for CognitivePoint {
    type Error = PointCreationError;

    fn try_from(fragment: &CognitiveFragment) -> Result<Self, Self::Error> {
        let kind = PointKind::try_from(fragment.kind)?;
        Ok(Self {
            id: PointId::new(),
            scope: fragment.scope.clone(),
            kind,
            source_fragments: vec![fragment.id],
            occurred_at: fragment.occurred_at,
            confidence: fragment.confidence,
            validity: fragment.validity,
        })
    }
}
