//! 多来源候选融合契约。

use chrono::{DateTime, Utc};
use jc_memory_protocol::{EventId, FragmentId};
use jc_memory_retrieval::{CandidateSource, RetrievalCandidate, fuse_rrf};

#[test]
fn rrf_merges_sources_and_is_independent_of_input_list_order() {
    let fragment_a = FragmentId::new();
    let fragment_b = FragmentId::new();
    let lexical = vec![
        candidate(fragment_a, CandidateSource::Lexical, 1, 0.9),
        candidate(fragment_b, CandidateSource::Lexical, 2, 0.8),
    ];
    let trigram = vec![
        candidate(fragment_b, CandidateSource::Trigram, 1, 0.7),
        candidate(fragment_a, CandidateSource::Trigram, 2, 0.6),
    ];
    let alias = vec![candidate(fragment_a, CandidateSource::Alias, 1, 0.5)];
    let entity = vec![candidate(fragment_b, CandidateSource::Entity, 1, 0.4)];
    let recent = vec![candidate(fragment_a, CandidateSource::Recent, 1, 0.3)];

    let forward = fuse_rrf(
        &[
            lexical.clone(),
            trigram.clone(),
            alias.clone(),
            entity.clone(),
            recent.clone(),
        ],
        60,
        10,
    )
    .expect("按正序融合候选");
    let reverse =
        fuse_rrf(&[recent, entity, alias, trigram, lexical], 60, 10).expect("按逆序融合候选");

    assert_eq!(forward, reverse);
    assert_eq!(forward.len(), 2);
    assert_eq!(forward[0].fragment_id, fragment_a);
    assert_eq!(forward[1].fragment_id, fragment_b);
    assert_eq!(forward[0].sources.len(), 4);
    assert_eq!(forward[1].sources.len(), 3);
}

#[test]
fn rrf_counts_each_fragment_source_once_and_uses_stable_tie_breaks() {
    let mut ids = [FragmentId::new(), FragmentId::new()];
    ids.sort();
    let older = DateTime::<Utc>::UNIX_EPOCH;
    let newer = older + chrono::Duration::seconds(1);
    let duplicate_source = vec![
        candidate_at(ids[1], CandidateSource::Lexical, 2, 0.9, newer),
        candidate_at(ids[1], CandidateSource::Lexical, 1, 0.8, newer),
    ];
    let tied = vec![
        candidate_at(ids[0], CandidateSource::Entity, 1, 0.8, older),
        candidate_at(ids[1], CandidateSource::Entity, 1, 0.8, newer),
    ];

    let fused = fuse_rrf(&[duplicate_source, tied], 60, 10).expect("融合重复来源候选");

    assert_eq!(fused[0].fragment_id, ids[1]);
    assert_eq!(fused[0].sources.len(), 2);
    let expected = 1.0 / 61.0 + 1.0 / 61.0;
    assert!((fused[0].rrf_score - expected).abs() < f64::EPSILON);
}

#[test]
fn rrf_rejects_invalid_rank_score_and_limit() {
    let invalid_rank = candidate(FragmentId::new(), CandidateSource::Lexical, 0, 0.5);
    assert!(fuse_rrf(&[vec![invalid_rank]], 60, 10).is_err());

    let invalid_score = candidate(FragmentId::new(), CandidateSource::Trigram, 1, f64::NAN);
    assert!(fuse_rrf(&[vec![invalid_score]], 60, 10).is_err());
    let infinite_score = candidate(FragmentId::new(), CandidateSource::Recent, 1, f64::INFINITY);
    assert!(fuse_rrf(&[vec![infinite_score]], 60, 10).is_err());
    assert!(fuse_rrf(&[], 0, 10).is_err());
    assert!(fuse_rrf(&[], 60, 0).is_err());
}

fn candidate(
    fragment_id: FragmentId,
    source: CandidateSource,
    source_rank: u32,
    source_score: f64,
) -> RetrievalCandidate {
    candidate_at(
        fragment_id,
        source,
        source_rank,
        source_score,
        DateTime::<Utc>::UNIX_EPOCH,
    )
}

fn candidate_at(
    fragment_id: FragmentId,
    source: CandidateSource,
    source_rank: u32,
    source_score: f64,
    occurred_at: DateTime<Utc>,
) -> RetrievalCandidate {
    RetrievalCandidate {
        fragment_id,
        event_id: EventId::new(),
        source,
        source_rank,
        source_score,
        occurred_at,
    }
}
