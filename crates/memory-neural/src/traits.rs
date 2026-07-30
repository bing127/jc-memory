//! 可插拔神经能力接口与版本化派生输出。

use async_trait::async_trait;
use jc_memory_activation::{IndexedPoint, RecallQuery};
use jc_memory_graph::{CognitivePoint, EdgeCandidate};
use jc_memory_protocol::{
    CognitiveFragment, CognitiveStage, FragmentId, PointId, RecallMode, SurfaceId,
};
use jc_memory_surface::CognitiveSurface;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::ModelManifest;

/// 神经能力调用错误。
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum NeuralError {
    /// 模型清单字段不合法。
    #[error("invalid model manifest: {0}")]
    InvalidManifest(String),
    /// 模型清单无法编码为校验输入。
    #[error("failed to encode model manifest: {0}")]
    ManifestEncoding(String),
    /// 输入批量超过模型清单限制。
    #[error("batch size {actual} exceeds maximum {maximum}")]
    BatchTooLarge {
        /// 实际批量。
        actual: usize,
        /// 允许的最大批量。
        maximum: usize,
    },
    /// 主模型或外部 Provider 调用失败。
    #[error("neural provider failed: {0}")]
    Provider(String),
    /// 确定性降级实现也无法返回结果。
    #[error("deterministic fallback failed: {0}")]
    Fallback(String),
}

/// 一个 Fragment 的稳定稀疏向量派生结果。
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Embedding {
    /// 来源 Fragment。
    pub fragment_id: FragmentId,
    /// 固定维度向量。
    pub values: Vec<f32>,
    /// 生成该结果的模型版本。
    pub model_version: String,
}

/// 一个 Fragment 的认知阶段分类结果。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Classification {
    /// 来源 Fragment。
    pub fragment_id: FragmentId,
    /// 分类得到的认知阶段。
    pub stage: CognitiveStage,
    /// 生成该结果的模型版本。
    pub model_version: String,
}

/// 一个 Fragment 的重要性评分。
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ImportanceScore {
    /// 来源 Fragment。
    pub fragment_id: FragmentId,
    /// 有界评分。
    pub score: f32,
    /// 生成该结果的模型版本。
    pub model_version: String,
}

/// 一次查询路由决策。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RoutingDecision {
    /// 选择的召回模式。
    pub mode: RecallMode,
    /// 生成该结果的模型版本。
    pub model_version: String,
}

/// 一个稳定重排后的候选。
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct RankedCandidate {
    /// 候选 Point。
    pub point_id: PointId,
    /// 有界排序分数。
    pub score: f32,
    /// 生成该结果的模型版本。
    pub model_version: String,
}

/// 一个 Point 对某个 Surface 的归属评分。
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct SurfaceScore {
    /// 候选 Point。
    pub point_id: PointId,
    /// 候选 Surface。
    pub surface_id: SurfaceId,
    /// 有界归属评分。
    pub score: f32,
    /// 生成该结果的模型版本。
    pub model_version: String,
}

/// 一对 Fragment 的矛盾评分。
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ContradictionScore {
    /// 左侧 Fragment。
    pub left_fragment_id: FragmentId,
    /// 右侧 Fragment。
    pub right_fragment_id: FragmentId,
    /// 有界矛盾评分。
    pub score: f32,
    /// 生成该结果的模型版本。
    pub model_version: String,
}

/// 把 Fragment 批量编码为稳定向量。
#[async_trait]
pub trait FragmentEncoder: Send + Sync {
    /// 编码输入，并保持输出与输入顺序一致。
    async fn encode(&self, batch: &[CognitiveFragment]) -> Result<Vec<Embedding>, NeuralError>;
    /// 返回编码器模型清单。
    fn manifest(&self) -> &ModelManifest;
}

/// 把 Fragment 批量映射到认知阶段。
#[async_trait]
pub trait EventClassifier: Send + Sync {
    /// 分类输入，并保持输出与输入顺序一致。
    async fn classify(
        &self,
        batch: &[CognitiveFragment],
    ) -> Result<Vec<Classification>, NeuralError>;
    /// 返回分类器模型清单。
    fn manifest(&self) -> &ModelManifest;
}

/// 预测 Fragment 的派生重要性。
#[async_trait]
pub trait ImportancePredictor: Send + Sync {
    /// 计算有界重要性评分。
    async fn predict_importance(
        &self,
        batch: &[CognitiveFragment],
    ) -> Result<Vec<ImportanceScore>, NeuralError>;
    /// 返回重要性模型清单。
    fn manifest(&self) -> &ModelManifest;
}

/// 为边候选附加确定性评分和模型版本。
#[async_trait]
pub trait EdgePredictor: Send + Sync {
    /// 处理边候选，并保持输出与输入顺序一致。
    async fn predict_edges(
        &self,
        batch: &[EdgeCandidate],
    ) -> Result<Vec<EdgeCandidate>, NeuralError>;
    /// 返回边预测模型清单。
    fn manifest(&self) -> &ModelManifest;
}

/// 根据查询内容选择召回模式。
#[async_trait]
pub trait QueryRouter: Send + Sync {
    /// 返回规则路由决策。
    async fn route(&self, query: &RecallQuery) -> Result<RoutingDecision, NeuralError>;
    /// 返回路由模型清单。
    fn manifest(&self) -> &ModelManifest;
}

/// 对候选 Point 进行稳定重排。
#[async_trait]
pub trait Reranker: Send + Sync {
    /// 返回按分数和 Point ID 稳定排序的候选。
    async fn rerank(
        &self,
        query: &RecallQuery,
        candidates: &[IndexedPoint],
    ) -> Result<Vec<RankedCandidate>, NeuralError>;
    /// 返回重排模型清单。
    fn manifest(&self) -> &ModelManifest;
}

/// 计算 Point 对现有 Surface 的归属分数。
#[async_trait]
pub trait SurfaceAssigner: Send + Sync {
    /// 返回所有正分归属候选。
    async fn assign(
        &self,
        points: &[CognitivePoint],
        surfaces: &[CognitiveSurface],
    ) -> Result<Vec<SurfaceScore>, NeuralError>;
    /// 返回归属模型清单。
    fn manifest(&self) -> &ModelManifest;
}

/// 检测批量 Fragment 中的成对矛盾。
#[async_trait]
pub trait ContradictionDetector: Send + Sync {
    /// 返回所有成对矛盾评分。
    async fn detect(
        &self,
        batch: &[CognitiveFragment],
    ) -> Result<Vec<ContradictionScore>, NeuralError>;
    /// 返回矛盾检测模型清单。
    fn manifest(&self) -> &ModelManifest;
}
