//! Million Fragments 可恢复压力测试核心库。

pub mod checkpoint;
pub mod generate;
pub mod load;
pub mod measure;

pub use checkpoint::{
    Checkpoint, CheckpointError, config_digest, load_checkpoint, resume_sequence, save_checkpoint,
};
pub use generate::{GeneratedFragment, GenerationOutcome, Generator, generate_smoke, resume_smoke};
pub use measure::{ReportError, StressReport, throughput_per_second};
