//! Graphiti Node/Edge 到候选 Point/Edge 的来源保真适配器。

pub mod config;
pub mod mapping;

pub use config::{GraphitiConfig, GraphitiConfigError};
pub use mapping::{
    GraphitiEdge, GraphitiEdgeCandidate, GraphitiEntity, GraphitiMappedNode, GraphitiMappingError,
    GraphitiNode, GraphitiProvenance,
};

/// Graphiti 来源保真适配器。
#[derive(Clone, Debug)]
pub struct GraphitiAdapter {
    config: GraphitiConfig,
}

impl GraphitiAdapter {
    /// 创建一个 Graphiti 适配器。
    #[must_use]
    pub const fn new(config: GraphitiConfig) -> Self {
        Self { config }
    }

    /// 将 Graphiti Node 映射为 Point 和 Entity。
    ///
    /// # Errors
    ///
    /// 外部标识缺失时返回映射错误。
    pub fn map_node(&self, node: GraphitiNode) -> Result<GraphitiMappedNode, GraphitiMappingError> {
        mapping::map_node(&self.config, node)
    }

    /// 将 Graphiti Edge 映射为保留时间与来源的候选边。
    ///
    /// # Errors
    ///
    /// 时间区间无效、外部标识缺失或端点不满足图约束时返回映射错误。
    pub fn map_edge(
        &self,
        edge: GraphitiEdge,
        source: &jc_memory_graph::CognitivePoint,
        target: &jc_memory_graph::CognitivePoint,
    ) -> Result<GraphitiEdgeCandidate, GraphitiMappingError> {
        mapping::map_edge(&self.config, edge, source, target)
    }
}
