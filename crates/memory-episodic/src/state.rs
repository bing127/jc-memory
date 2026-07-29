//! Versioned, evidence-linked state chains.

use std::{collections::BTreeMap, sync::Arc};

use chrono::{DateTime, Utc};
use jc_memory_graph::{CognitiveEdge, CognitivePoint, EdgeCreationError, EvidenceLink, PointKind};
use jc_memory_protocol::{EdgeKind, MemoryScope, StateId, StateVersionId, ValidityStatus};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use tokio::sync::Mutex;

/// A state observation's graph point and authoritative evidence link.
#[derive(Clone, Debug, PartialEq)]
pub struct StateEvidence {
    /// State-change point derived from the authoritative Fragment.
    pub point: CognitivePoint,
    /// Fragment, Event, or `EvidenceRef` supporting the state value.
    pub link: EvidenceLink,
}

/// One immutable value in a state's effective-time version chain.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct StateVersion {
    /// Version identifier.
    pub id: StateVersionId,
    /// Stable identifier shared by every version of the state key.
    pub state_id: StateId,
    /// Structured state value.
    pub value: Value,
    /// Inclusive beginning of the effective interval.
    pub valid_from: DateTime<Utc>,
    /// Exclusive end of the effective interval, when superseded.
    pub valid_to: Option<DateTime<Utc>>,
    /// Validity of this state version.
    pub validity: ValidityStatus,
    /// Authoritative evidence supporting this version.
    pub evidence: Vec<EvidenceLink>,
    /// Immediately preceding version, when one exists.
    pub previous_version_id: Option<StateVersionId>,
}

/// Result of resolving one state key at an effective time.
#[derive(Clone, Debug, PartialEq)]
pub enum StateResolution {
    /// Exactly one effective version exists.
    Resolved(StateVersion),
    /// Competing versions are effective and must remain visible to callers.
    Conflicted(Vec<StateVersion>),
    /// No state version is effective at the requested time.
    Missing,
}

/// Atomic result of recording or superseding a state value.
#[derive(Clone, Debug, PartialEq)]
pub struct StateMutation {
    /// Newly created state version.
    pub version: StateVersion,
    /// Evidence-linked graph edge for an unambiguous replacement.
    pub supersedes_edge: Option<CognitiveEdge>,
}

/// A rejected state-chain mutation.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum StateRepositoryError {
    /// State keys cannot be empty.
    #[error("state key cannot be empty")]
    EmptyKey,
    /// Initial state already exists and requires an explicit supersede operation.
    #[error("state key already exists")]
    StateAlreadyExists,
    /// Supersede was requested for an unknown key.
    #[error("state key does not exist")]
    MissingState,
    /// An unresolved conflict must be handled before a later replacement.
    #[error("state key has unresolved competing current versions")]
    ExistingConflict,
    /// A replacement predates the current version.
    #[error("replacement cannot predate the current state version")]
    OutOfOrder,
    /// Repeating the same value at the same effective time is not a replacement.
    #[error("same value already exists at the requested effective time")]
    SameTimeDuplicate,
    /// Evidence point belongs to another scope.
    #[error("state evidence point must belong to the repository scope")]
    EvidenceScopeMismatch,
    /// Evidence point is not a state change.
    #[error("state evidence point must have kind state_change")]
    EvidencePointKind,
    /// Evidence point time differs from the version's effective time.
    #[error("state evidence point time must equal the version effective time")]
    EvidenceTimeMismatch,
    /// Evidence link does not reference a source Fragment of the point.
    #[error("state evidence link must reference a source Fragment of the point")]
    EvidenceFragmentMismatch,
    /// The generated supersession edge violates graph endpoint invariants.
    #[error(transparent)]
    Edge(#[from] EdgeCreationError),
}

#[derive(Debug, Default)]
struct StateStore {
    chains: BTreeMap<String, StateChain>,
}

#[derive(Debug)]
struct StateChain {
    id: StateId,
    versions: Vec<StateVersion>,
    version_points: BTreeMap<StateVersionId, CognitivePoint>,
}

/// Scope-bound repository that mutates each state chain under one async transaction lock.
#[derive(Clone, Debug)]
pub struct StateRepository {
    scope: MemoryScope,
    store: Arc<Mutex<StateStore>>,
}

impl StateRepository {
    /// Creates an empty repository for one exact memory scope.
    #[must_use]
    pub fn new(scope: MemoryScope) -> Self {
        Self {
            scope,
            store: Arc::new(Mutex::new(StateStore::default())),
        }
    }

    /// Records the first version of a state key.
    ///
    /// # Errors
    ///
    /// Returns [`StateRepositoryError`] when the key already exists or evidence
    /// does not match the repository scope, state kind, time, and Fragment.
    pub async fn record(
        &self,
        key: &str,
        value: Value,
        effective_at: DateTime<Utc>,
        evidence: StateEvidence,
    ) -> Result<StateMutation, StateRepositoryError> {
        validate_key(key)?;
        self.validate_evidence(effective_at, &evidence)?;
        let mut store = self.store.lock().await;
        if store.chains.contains_key(key) {
            return Err(StateRepositoryError::StateAlreadyExists);
        }

        let state_id = StateId::new();
        let version = StateVersion {
            id: StateVersionId::new(),
            state_id,
            value,
            valid_from: effective_at,
            valid_to: None,
            validity: evidence.point.validity,
            evidence: vec![evidence.link],
            previous_version_id: None,
        };
        let mut version_points = BTreeMap::new();
        version_points.insert(version.id, evidence.point);
        store.chains.insert(
            key.to_owned(),
            StateChain {
                id: state_id,
                versions: vec![version.clone()],
                version_points,
            },
        );
        Ok(StateMutation {
            version,
            supersedes_edge: None,
        })
    }

    /// Replaces the unique current value or records a visible same-time conflict.
    ///
    /// The old interval closure, new version, point association, and optional
    /// `Supersedes` edge are committed while holding one transaction lock.
    ///
    /// # Errors
    ///
    /// Returns [`StateRepositoryError`] for missing or conflicted state, an
    /// out-of-order replacement, a same-time duplicate, or invalid evidence.
    pub async fn supersede(
        &self,
        key: &str,
        value: Value,
        effective_at: DateTime<Utc>,
        evidence: StateEvidence,
    ) -> Result<StateMutation, StateRepositoryError> {
        validate_key(key)?;
        self.validate_evidence(effective_at, &evidence)?;
        let mut store = self.store.lock().await;
        let chain = store
            .chains
            .get_mut(key)
            .ok_or(StateRepositoryError::MissingState)?;
        let current_indices: Vec<_> = chain
            .versions
            .iter()
            .enumerate()
            .filter_map(|(index, version)| version.valid_to.is_none().then_some(index))
            .collect();
        if current_indices.len() != 1 {
            return Err(StateRepositoryError::ExistingConflict);
        }
        let current_index = current_indices[0];
        let current = chain.versions[current_index].clone();
        if effective_at < current.valid_from {
            return Err(StateRepositoryError::OutOfOrder);
        }
        if effective_at == current.valid_from && value == current.value {
            return Err(StateRepositoryError::SameTimeDuplicate);
        }

        let same_time_conflict = effective_at == current.valid_from;
        let version = StateVersion {
            id: StateVersionId::new(),
            state_id: chain.id,
            value,
            valid_from: effective_at,
            valid_to: None,
            validity: if same_time_conflict {
                ValidityStatus::Disputed
            } else {
                evidence.point.validity
            },
            evidence: vec![evidence.link.clone()],
            previous_version_id: Some(current.id),
        };

        let supersedes_edge = if same_time_conflict {
            None
        } else {
            let previous_point = chain
                .version_points
                .get(&current.id)
                .ok_or(StateRepositoryError::EvidenceFragmentMismatch)?;
            let mut edge = CognitiveEdge::new(
                &evidence.point,
                previous_point,
                EdgeKind::Supersedes,
                evidence.link.clone(),
            )?;
            edge.valid_from = effective_at;
            Some(edge)
        };

        if !same_time_conflict {
            chain.versions[current_index].valid_to = Some(effective_at);
            chain.versions[current_index].validity = ValidityStatus::Superseded;
        }
        chain.version_points.insert(version.id, evidence.point);
        chain.versions.push(version.clone());

        Ok(StateMutation {
            version,
            supersedes_edge,
        })
    }

    /// Resolves the current state without hiding competing versions.
    pub async fn current(&self, key: &str) -> StateResolution {
        let store = self.store.lock().await;
        let Some(chain) = store.chains.get(key) else {
            return StateResolution::Missing;
        };
        resolve(
            chain
                .versions
                .iter()
                .filter(|version| version.valid_to.is_none()),
        )
    }

    /// Resolves the state at an effective time using half-open validity intervals.
    pub async fn at(&self, key: &str, effective_at: DateTime<Utc>) -> StateResolution {
        let store = self.store.lock().await;
        let Some(chain) = store.chains.get(key) else {
            return StateResolution::Missing;
        };
        resolve(chain.versions.iter().filter(|version| {
            version.valid_from <= effective_at
                && version
                    .valid_to
                    .is_none_or(|valid_to| effective_at < valid_to)
        }))
    }

    /// Returns all versions in deterministic effective-time and ID order.
    pub async fn history(&self, key: &str) -> Vec<StateVersion> {
        let store = self.store.lock().await;
        let Some(chain) = store.chains.get(key) else {
            return Vec::new();
        };
        let mut versions = chain.versions.clone();
        versions.sort_by_key(|version| (version.valid_from, version.id));
        versions
    }

    fn validate_evidence(
        &self,
        effective_at: DateTime<Utc>,
        evidence: &StateEvidence,
    ) -> Result<(), StateRepositoryError> {
        if evidence.point.scope != self.scope {
            return Err(StateRepositoryError::EvidenceScopeMismatch);
        }
        if evidence.point.kind != PointKind::StateChange {
            return Err(StateRepositoryError::EvidencePointKind);
        }
        if evidence.point.occurred_at != effective_at {
            return Err(StateRepositoryError::EvidenceTimeMismatch);
        }
        if !evidence
            .point
            .source_fragments
            .contains(&evidence.link.fragment_id)
        {
            return Err(StateRepositoryError::EvidenceFragmentMismatch);
        }
        Ok(())
    }
}

fn validate_key(key: &str) -> Result<(), StateRepositoryError> {
    if key.trim().is_empty() {
        Err(StateRepositoryError::EmptyKey)
    } else {
        Ok(())
    }
}

fn resolve<'a>(versions: impl IntoIterator<Item = &'a StateVersion>) -> StateResolution {
    let mut versions: Vec<_> = versions.into_iter().cloned().collect();
    versions.sort_by_key(|version| (version.valid_from, version.id));
    match versions.len() {
        0 => StateResolution::Missing,
        1 => versions
            .pop()
            .map_or(StateResolution::Missing, StateResolution::Resolved),
        _ => StateResolution::Conflicted(versions),
    }
}
