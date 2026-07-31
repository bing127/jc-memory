//! Graphiti Node/Edge 映射。

use chrono::{DateTime, Utc};
use jc_memory_graph::{CognitiveEdge, CognitivePoint, EdgeCandidate, EvidenceLink};
use jc_memory_protocol::{EdgeKind, FragmentId, MemoryScope, PointId, PointKind, ValidityStatus};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::config::GraphitiConfig;

/// Graphiti 节点输入。
#[derive(Clone, Debug, PartialEq)]
pub struct GraphitiNode {
    /// Graphiti 节点的外部标识。
    pub external_id: String,
    /// Graphiti 实体类型。
    pub entity_type: String,
    /// 预先分配的疆诚记忆 Point 标识。
    pub point_id: PointId,
    /// 支撑节点的权威 Fragment 标识。
    pub fragment_id: FragmentId,
    /// 节点所属 Scope。
    pub scope: MemoryScope,
    /// 节点 Point 类型。
    pub kind: PointKind,
    /// 节点发生时间。
    pub occurred_at: DateTime<Utc>,
    /// 从来源继承的置信度。
    pub confidence: f32,
    /// 从来源继承的有效性。
    pub validity: ValidityStatus,
}

/// Graphiti 边输入。
#[derive(Clone, Debug, PartialEq)]
pub struct GraphitiEdge {
    /// Graphiti 边的外部标识。
    pub external_id: String,
    /// 源节点外部标识。
    pub source_node_id: String,
    /// 目标节点外部标识。
    pub target_node_id: String,
    /// 边语义。
    pub kind: EdgeKind,
    /// 边生效时间。
    pub valid_at: DateTime<Utc>,
    /// 边失效时间。
    pub invalid_at: Option<DateTime<Utc>>,
    /// 来源边置信度。
    pub confidence: f32,
    /// 支撑边的权威 Fragment 标识。
    pub evidence_fragment_id: FragmentId,
    /// 是否由模型推断得到。
    pub inferred: bool,
    /// 推断模型版本。
    pub model_version_id: Option<String>,
}

/// Graphiti 关系中可重建的来源信息。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GraphitiProvenance {
    /// 来源平台版本。
    pub source_version: String,
    /// 外部节点或边标识。
    pub external_id: String,
    /// 是否为推断关系。
    pub inferred: bool,
}

/// Graphiti 实体视图。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GraphitiEntity {
    /// 外部实体标识。
    pub external_id: String,
    /// 外部实体类型。
    pub entity_type: String,
    /// 对应的 Point 标识。
    pub point_id: PointId,
}

/// Graphiti 节点映射结果。
#[derive(Clone, Debug, PartialEq)]
pub struct GraphitiMappedNode {
    /// 派生 Point。
    pub point: CognitivePoint,
    /// 外部实体映射。
    pub entity: GraphitiEntity,
    /// 来源元数据。
    pub provenance: GraphitiProvenance,
}

/// Graphiti 候选边映射结果。
#[derive(Clone, Debug, PartialEq)]
pub struct GraphitiEdgeCandidate {
    /// 尚未写入图仓库的候选边。
    pub edge: EdgeCandidate,
    /// 来源元数据。
    pub provenance: GraphitiProvenance,
}

/// Graphiti 映射错误。
#[derive(Clone, Debug, Error, PartialEq)]
pub enum GraphitiMappingError {
    /// 外部标识为空。
    #[error("Graphiti {kind} external_id 不能为空")]
    EmptyExternalId {
        /// 发生错误的输入类型。
        kind: &'static str,
    },
    /// 时间区间不合法。
    #[error("Graphiti invalid_at 必须晚于 valid_at")]
    InvalidTemporalInterval,
    /// 边构造失败。
    #[error("构造 Graphiti 边失败: {0}")]
    Edge(#[from] jc_memory_graph::EdgeCreationError),
}

pub(crate) fn map_node(
    config: &GraphitiConfig,
    node: GraphitiNode,
) -> Result<GraphitiMappedNode, GraphitiMappingError> {
    if node.external_id.trim().is_empty() {
        return Err(GraphitiMappingError::EmptyExternalId { kind: "Node" });
    }
    if node.entity_type.trim().is_empty() {
        return Err(GraphitiMappingError::EmptyExternalId {
            kind: "Node entity_type",
        });
    }
    Ok(GraphitiMappedNode {
        point: CognitivePoint {
            id: node.point_id,
            scope: node.scope,
            kind: node.kind,
            source_fragments: vec![node.fragment_id],
            occurred_at: node.occurred_at,
            confidence: node.confidence,
            validity: node.validity,
        },
        entity: GraphitiEntity {
            external_id: node.external_id.clone(),
            entity_type: node.entity_type,
            point_id: node.point_id,
        },
        provenance: GraphitiProvenance {
            source_version: config.source_version.clone(),
            external_id: node.external_id,
            inferred: false,
        },
    })
}

pub(crate) fn map_edge(
    config: &GraphitiConfig,
    edge: GraphitiEdge,
    source: &CognitivePoint,
    target: &CognitivePoint,
) -> Result<GraphitiEdgeCandidate, GraphitiMappingError> {
    if edge.external_id.trim().is_empty() {
        return Err(GraphitiMappingError::EmptyExternalId { kind: "Edge" });
    }
    if edge
        .invalid_at
        .is_some_and(|invalid_at| invalid_at <= edge.valid_at)
    {
        return Err(GraphitiMappingError::InvalidTemporalInterval);
    }
    let mut mapped = CognitiveEdge::new(
        source,
        target,
        edge.kind,
        EvidenceLink {
            fragment_id: edge.evidence_fragment_id,
            event_id: None,
            evidence_ref_id: None,
        },
    )?;
    mapped.valid_from = edge.valid_at;
    mapped.valid_to = edge.invalid_at;
    mapped.confidence = edge.confidence;
    mapped.model_version_id = edge.model_version_id;
    // Graphiti 的推断关系在人工或工具确认前始终保持未验证。
    mapped.human_confirmed = false;
    mapped.tool_verified = false;
    Ok(GraphitiEdgeCandidate {
        edge: mapped,
        provenance: GraphitiProvenance {
            source_version: config.source_version.clone(),
            external_id: edge.external_id,
            inferred: edge.inferred,
        },
    })
}
