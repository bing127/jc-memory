//! 零外部模型的确定性查询规划与候选融合。

mod dictionary;
mod fusion;
mod normalize;
mod plan;
mod trace;

pub use dictionary::{AliasDictionary, DictionaryError};
pub use fusion::{CandidateSource, FusedCandidate, FusionError, RetrievalCandidate, fuse_rrf};
pub use normalize::{
    alias_character_trigrams, character_trigrams, lexical_terms, normalize_text, trigram_similarity,
};
pub use plan::{LexicalQuery, QueryPlanner, RetrievalError, RetrievalLimits, RetrievalPlan};
pub use trace::RetrievalTrace;
