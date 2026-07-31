//! 神经能力接口与完全离线的确定性降级实现。

/// 显式时间驱动的三态熔断器与超时降级包装。
pub mod circuit_breaker;
/// 完全离线的确定性神经能力实现。
pub mod fallback;
/// 模型身份与执行契约清单。
pub mod manifest;
/// ONNX Manifest 校验、CPU Session 和确定性 Fallback。
pub mod onnx;
/// 神经能力接口及派生输出类型。
pub mod traits;

pub use circuit_breaker::{
    CircuitBreaker, CircuitState, FallbackReason, FallbackResult, with_timeout_fallback,
};
pub use fallback::DeterministicFallback;
pub use manifest::{DevicePolicy, ModelManifest, ModelManifestSpec};
pub use traits::{
    Classification, ContradictionDetector, ContradictionScore, EdgePredictor, Embedding,
    EventClassifier, FragmentEncoder, ImportancePredictor, ImportanceScore, NeuralError,
    QueryRouter, RankedCandidate, Reranker, RoutingDecision, SurfaceAssigner, SurfaceScore,
};
