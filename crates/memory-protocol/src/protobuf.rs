//! Rust 领域协议与生成 Protobuf 类型之间的无损转换。

use std::{
    collections::{BTreeMap, BTreeSet},
    str::FromStr,
};

use chrono::{DateTime, Utc};

use crate::{
    AccessScope, CognitiveFragment, CommitCommand, CommitResult, ConflictView, Coverage, EdgeKind,
    EpisodeView, EvidenceRef, EvidenceView, ExplainResult, FragmentContent, FragmentKind,
    FragmentSource, IdParseError, KnowledgeKind, KnowledgeView, MemoryCommand, MemoryPacket,
    MemoryRequest, MemoryResponse, MemoryScope, MissingEvidence, MutationAction, MutationCommand,
    MutationResult, ObserveCommand, ObserveResult, Perspective, PointKind, PointView,
    ProcedureView, RecallLimits, RecallMode, RecallPath, RequestValidationError, ResponseEnvelope,
    SecurityLabel, SourceType, StateDeclaration, StateView, StatsCommand, StatsResult,
    SurfaceStatus, SurfaceView, ValidityStatus, VerificationMethod,
    generated::jiangcheng::memory::v1 as proto,
};

/// 领域协议与 Protobuf 之间的稳定转换错误。
#[derive(Debug, thiserror::Error)]
pub enum ProtoConversionError {
    /// 必需的 message、oneof 或 JSON 值不存在。
    #[error("protobuf field {field} is required")]
    MissingField {
        /// 缺失字段的稳定路径。
        field: &'static str,
    },
    /// 强类型标识无法从 wire 字符串解析。
    #[error("protobuf field {field} contains an invalid identifier: {source}")]
    InvalidIdentifier {
        /// 无效标识字段的稳定路径。
        field: &'static str,
        /// 原始标识解析错误。
        #[source]
        source: IdParseError,
    },
    /// 枚举值未知或使用了保留的 Unspecified 值。
    #[error("protobuf field {field} contains unknown enum value {value}")]
    UnknownEnum {
        /// 无效枚举字段的稳定路径。
        field: &'static str,
        /// 收到的整数值。
        value: i32,
    },
    /// Protobuf Timestamp 超出 Chrono 可表示范围。
    #[error("protobuf field {field} contains invalid timestamp {seconds}:{nanos}")]
    InvalidTimestamp {
        /// 无效时间字段的稳定路径。
        field: &'static str,
        /// Unix 秒。
        seconds: i64,
        /// 纳秒部分。
        nanos: i32,
    },
    /// Protobuf JSON 浮点值不是有效 JSON Number。
    #[error("protobuf JSON contains a non-finite double")]
    InvalidJsonNumber,
    /// Protobuf JSON 对象包含重复键，无法无损转换成 JSON Object。
    #[error("protobuf JSON object contains duplicate key {key}")]
    DuplicateJsonKey {
        /// 重复的对象键。
        key: String,
    },
    /// 请求 Envelope 字段未通过领域协议校验。
    #[error(transparent)]
    Request(#[from] RequestValidationError),
    /// 当前 Protobuf 契约尚未声明该命令或响应。
    #[error("protobuf contract does not support {kind}")]
    Unsupported {
        /// 尚未支持的命令或响应名称。
        kind: &'static str,
    },
    /// 数值字段无法转换到领域类型。
    #[error("protobuf field {field} contains an out-of-range value {value}")]
    InvalidNumeric {
        /// 数值字段的稳定路径。
        field: &'static str,
        /// 收到的无符号数值。
        value: u64,
    },
    /// Protobuf repeated map 条目包含重复键。
    #[error("protobuf field {field} contains duplicate key {key}")]
    DuplicateMapKey {
        /// 重复键所在字段。
        field: &'static str,
        /// 重复键内容。
        key: String,
    },
}

impl TryFrom<MemoryRequest> for proto::RequestEnvelope {
    type Error = ProtoConversionError;

    fn try_from(request: MemoryRequest) -> Result<Self, Self::Error> {
        request.validate()?;
        let command = match request.command {
            MemoryCommand::Observe(command) => {
                proto::request_envelope::Command::Observe(proto::ObserveCommand {
                    fragment: Some(command.fragment.try_into()?),
                })
            }
            MemoryCommand::Commit(command) => {
                proto::request_envelope::Command::Commit(proto::CommitCommand {
                    fragments: command
                        .fragments
                        .into_iter()
                        .map(TryInto::try_into)
                        .collect::<Result<_, ProtoConversionError>>()?,
                })
            }
            MemoryCommand::Recall(command) => {
                proto::request_envelope::Command::Recall(proto::RecallCommand {
                    mode: recall_mode_to_proto(command.mode),
                    text: command.text,
                    entities: command.entities,
                    at_time: command.at_time.map(datetime_to_proto),
                    limits: Some(recall_limits_to_proto(command.limits)),
                })
            }
            MemoryCommand::Stats(_) => {
                proto::request_envelope::Command::Stats(proto::StatsCommand {})
            }
            MemoryCommand::Correct(command) => {
                proto::request_envelope::Command::Correct(mutation_command_to_proto(command))
            }
            MemoryCommand::Forget(command) => {
                proto::request_envelope::Command::Forget(mutation_command_to_proto(command))
            }
            MemoryCommand::Explain(command) => {
                proto::request_envelope::Command::Explain(proto::RecallCommand {
                    mode: recall_mode_to_proto(command.mode),
                    text: command.text,
                    entities: command.entities,
                    at_time: command.at_time.map(datetime_to_proto),
                    limits: Some(recall_limits_to_proto(command.limits)),
                })
            }
        };
        Ok(Self {
            api_version: request.api_version,
            request_id: request.request_id,
            access_scope: Some(request.access_scope.into()),
            command: Some(command),
        })
    }
}

impl TryFrom<proto::RequestEnvelope> for MemoryRequest {
    type Error = ProtoConversionError;

    fn try_from(request: proto::RequestEnvelope) -> Result<Self, Self::Error> {
        let command = match required(request.command, "command")? {
            proto::request_envelope::Command::Observe(command) => {
                MemoryCommand::Observe(ObserveCommand {
                    fragment: required(command.fragment, "observe.fragment")?.try_into()?,
                })
            }
            proto::request_envelope::Command::Commit(command) => {
                MemoryCommand::Commit(CommitCommand {
                    fragments: command
                        .fragments
                        .into_iter()
                        .map(TryInto::try_into)
                        .collect::<Result<_, ProtoConversionError>>()?,
                })
            }
            proto::request_envelope::Command::Recall(command) => {
                MemoryCommand::Recall(recall_command_from_proto(command)?)
            }
            proto::request_envelope::Command::Stats(_) => MemoryCommand::Stats(StatsCommand),
            proto::request_envelope::Command::Correct(command) => {
                MemoryCommand::Correct(mutation_command_from_proto(command)?)
            }
            proto::request_envelope::Command::Forget(command) => {
                MemoryCommand::Forget(mutation_command_from_proto(command)?)
            }
            proto::request_envelope::Command::Explain(command) => {
                MemoryCommand::Explain(recall_command_from_proto(command)?)
            }
        };
        let request = Self {
            api_version: request.api_version,
            request_id: request.request_id,
            access_scope: required(request.access_scope, "access_scope")?.try_into()?,
            command,
        };
        request.validate()?;
        Ok(request)
    }
}

impl TryFrom<ResponseEnvelope> for proto::ResponseEnvelope {
    type Error = ProtoConversionError;

    fn try_from(envelope: ResponseEnvelope) -> Result<Self, Self::Error> {
        let response = match envelope.response {
            MemoryResponse::Observe(result) => {
                proto::response_envelope::Response::Observe(result.into())
            }
            MemoryResponse::Commit(result) => {
                proto::response_envelope::Response::Commit(result.into())
            }
            MemoryResponse::Recall(packet) => {
                proto::response_envelope::Response::Recall(memory_packet_to_proto(packet))
            }
            MemoryResponse::Stats(result) => {
                proto::response_envelope::Response::Stats(result.into())
            }
            MemoryResponse::Mutation(result) => {
                proto::response_envelope::Response::Mutation(result.into())
            }
            MemoryResponse::Explain(result) => {
                proto::response_envelope::Response::Explain(result.into())
            }
        };
        Ok(Self {
            api_version: envelope.api_version,
            request_id: envelope.request_id,
            response: Some(response),
        })
    }
}

impl TryFrom<proto::ResponseEnvelope> for ResponseEnvelope {
    type Error = ProtoConversionError;

    fn try_from(envelope: proto::ResponseEnvelope) -> Result<Self, Self::Error> {
        let response = match required(envelope.response, "response")? {
            proto::response_envelope::Response::Observe(result) => {
                MemoryResponse::Observe(result.try_into()?)
            }
            proto::response_envelope::Response::Commit(result) => {
                MemoryResponse::Commit(result.try_into()?)
            }
            proto::response_envelope::Response::Recall(packet) => {
                MemoryResponse::Recall(memory_packet_from_proto(packet)?)
            }
            proto::response_envelope::Response::Stats(result) => {
                MemoryResponse::Stats(result.try_into()?)
            }
            proto::response_envelope::Response::Mutation(result) => {
                MemoryResponse::Mutation(result.try_into()?)
            }
            proto::response_envelope::Response::Explain(result) => {
                MemoryResponse::Explain(result.try_into()?)
            }
        };
        Ok(Self {
            api_version: envelope.api_version,
            request_id: envelope.request_id,
            response,
        })
    }
}

impl From<AccessScope> for proto::AccessScope {
    fn from(scope: AccessScope) -> Self {
        Self {
            namespace_id: scope.namespace_id.to_string(),
            tenant_id: scope.tenant_id.map(|value| value.to_string()),
            user_id: scope.user_id.map(|value| value.to_string()),
            agent_id: scope.agent_id.map(|value| value.to_string()),
            project_id: scope.project_id.map(|value| value.to_string()),
        }
    }
}

impl TryFrom<proto::AccessScope> for AccessScope {
    type Error = ProtoConversionError;

    fn try_from(scope: proto::AccessScope) -> Result<Self, Self::Error> {
        Ok(Self {
            namespace_id: parse_id("access_scope.namespace_id", &scope.namespace_id)?,
            tenant_id: parse_optional_id("access_scope.tenant_id", scope.tenant_id)?,
            user_id: parse_optional_id("access_scope.user_id", scope.user_id)?,
            agent_id: parse_optional_id("access_scope.agent_id", scope.agent_id)?,
            project_id: parse_optional_id("access_scope.project_id", scope.project_id)?,
        })
    }
}

impl TryFrom<CognitiveFragment> for proto::CognitiveFragment {
    type Error = ProtoConversionError;

    fn try_from(fragment: CognitiveFragment) -> Result<Self, Self::Error> {
        Ok(Self {
            id: fragment.id.to_string(),
            scope: Some(AccessScope::from(fragment.scope).into()),
            session_id: fragment.session_id.map(|value| value.to_string()),
            trace_id: fragment.trace_id.map(|value| value.to_string()),
            kind: fragment_kind_to_proto(fragment.kind),
            content: Some(fragment_content_to_proto(fragment.content)),
            source: Some(fragment_source_to_proto(fragment.source)),
            perspective: perspective_to_proto(fragment.perspective),
            occurred_at: Some(datetime_to_proto(fragment.occurred_at)),
            observed_at: Some(datetime_to_proto(fragment.observed_at)),
            confidence: fragment.confidence,
            importance: fragment.importance,
            validity: validity_to_proto(fragment.validity),
            security_label: security_label_to_proto(fragment.security_label),
            evidence: fragment
                .evidence
                .into_iter()
                .map(evidence_to_proto)
                .collect(),
            metadata: Some(json_object_to_proto(fragment.metadata)),
        })
    }
}

impl TryFrom<proto::CognitiveFragment> for CognitiveFragment {
    type Error = ProtoConversionError;

    fn try_from(fragment: proto::CognitiveFragment) -> Result<Self, Self::Error> {
        let scope: AccessScope = required(fragment.scope, "fragment.scope")?.try_into()?;
        Ok(Self {
            id: parse_id("fragment.id", &fragment.id)?,
            scope: MemoryScope::from(scope),
            session_id: parse_optional_id("fragment.session_id", fragment.session_id)?,
            trace_id: parse_optional_id("fragment.trace_id", fragment.trace_id)?,
            kind: fragment_kind_from_proto(fragment.kind)?,
            content: fragment_content_from_proto(required(fragment.content, "fragment.content")?)?,
            source: fragment_source_from_proto(required(fragment.source, "fragment.source")?)?,
            perspective: perspective_from_proto(fragment.perspective)?,
            occurred_at: datetime_from_proto(
                "fragment.occurred_at",
                required(fragment.occurred_at, "fragment.occurred_at")?,
            )?,
            observed_at: datetime_from_proto(
                "fragment.observed_at",
                required(fragment.observed_at, "fragment.observed_at")?,
            )?,
            confidence: fragment.confidence,
            importance: fragment.importance,
            validity: validity_from_proto(fragment.validity)?,
            security_label: security_label_from_proto(fragment.security_label)?,
            evidence: fragment
                .evidence
                .into_iter()
                .map(evidence_from_proto)
                .collect::<Result<_, _>>()?,
            metadata: json_object_from_proto(required(fragment.metadata, "fragment.metadata")?)?,
        })
    }
}

impl From<ObserveResult> for proto::ObserveResult {
    fn from(result: ObserveResult) -> Self {
        Self {
            fragment_id: result.fragment_id.to_string(),
            event_id: result.event_id.to_string(),
            sequence: result.sequence,
            content_hash: result.content_hash.to_string(),
            created: result.created,
            quarantined: result.quarantined,
        }
    }
}

impl TryFrom<proto::ObserveResult> for ObserveResult {
    type Error = ProtoConversionError;

    fn try_from(result: proto::ObserveResult) -> Result<Self, Self::Error> {
        Ok(Self {
            fragment_id: parse_id("observe_result.fragment_id", &result.fragment_id)?,
            event_id: parse_id("observe_result.event_id", &result.event_id)?,
            sequence: result.sequence,
            content_hash: parse_id("observe_result.content_hash", &result.content_hash)?,
            created: result.created,
            quarantined: result.quarantined,
        })
    }
}

impl From<CommitResult> for proto::CommitResult {
    fn from(result: CommitResult) -> Self {
        Self {
            receipts: result.receipts.into_iter().map(Into::into).collect(),
        }
    }
}

impl TryFrom<proto::CommitResult> for CommitResult {
    type Error = ProtoConversionError;

    fn try_from(result: proto::CommitResult) -> Result<Self, Self::Error> {
        Ok(Self {
            receipts: result
                .receipts
                .into_iter()
                .map(TryInto::try_into)
                .collect::<Result<_, ProtoConversionError>>()?,
        })
    }
}

impl From<StatsResult> for proto::StatsResult {
    fn from(result: StatsResult) -> Self {
        Self {
            scope: Some(result.scope.into()),
            fragment_count: result.fragment_count,
            event_count: result.event_count,
        }
    }
}

impl TryFrom<proto::StatsResult> for StatsResult {
    type Error = ProtoConversionError;

    fn try_from(result: proto::StatsResult) -> Result<Self, Self::Error> {
        Ok(Self {
            scope: required(result.scope, "stats_result.scope")?.try_into()?,
            fragment_count: result.fragment_count,
            event_count: result.event_count,
        })
    }
}

fn mutation_command_to_proto(command: MutationCommand) -> proto::MutationCommand {
    proto::MutationCommand {
        target_fragment_id: command.target_fragment_id.to_string(),
        content: Some(fragment_content_to_proto(command.content)),
        source: Some(fragment_source_to_proto(command.source)),
        perspective: perspective_to_proto(command.perspective),
        occurred_at: Some(datetime_to_proto(command.occurred_at)),
        verification_method: verification_method_to_proto(command.verification_method),
        reason: command.reason,
        state: command.state.map(state_declaration_to_proto),
    }
}

fn mutation_command_from_proto(
    command: proto::MutationCommand,
) -> Result<MutationCommand, ProtoConversionError> {
    Ok(MutationCommand {
        target_fragment_id: parse_id("mutation.target_fragment_id", &command.target_fragment_id)?,
        content: fragment_content_from_proto(required(command.content, "mutation.content")?)?,
        source: fragment_source_from_proto(required(command.source, "mutation.source")?)?,
        perspective: perspective_from_proto(command.perspective)?,
        occurred_at: datetime_from_proto(
            "mutation.occurred_at",
            required(command.occurred_at, "mutation.occurred_at")?,
        )?,
        verification_method: verification_method_from_proto(command.verification_method)?,
        reason: command.reason,
        state: command
            .state
            .map(state_declaration_from_proto)
            .transpose()?,
    })
}

fn state_declaration_to_proto(value: StateDeclaration) -> proto::StateDeclaration {
    proto::StateDeclaration {
        key: value.key,
        value: Some(json_value_to_proto(value.value)),
        reason: value.reason,
    }
}

fn state_declaration_from_proto(
    value: proto::StateDeclaration,
) -> Result<StateDeclaration, ProtoConversionError> {
    Ok(StateDeclaration {
        key: value.key,
        value: json_value_from_proto(required(value.value, "mutation.state.value")?)?,
        reason: value.reason,
    })
}

impl From<MutationResult> for proto::MutationResult {
    fn from(result: MutationResult) -> Self {
        Self {
            action: mutation_action_to_proto(result.action),
            target_fragment_id: result.target_fragment_id.to_string(),
            fragment_id: result.fragment_id.to_string(),
            event_id: result.event_id.to_string(),
            sequence: result.sequence,
            relation: result.relation.map(edge_kind_to_proto),
        }
    }
}

impl TryFrom<proto::MutationResult> for MutationResult {
    type Error = ProtoConversionError;

    fn try_from(result: proto::MutationResult) -> Result<Self, Self::Error> {
        Ok(Self {
            action: mutation_action_from_proto(result.action)?,
            target_fragment_id: parse_id(
                "mutation_result.target_fragment_id",
                &result.target_fragment_id,
            )?,
            fragment_id: parse_id("mutation_result.fragment_id", &result.fragment_id)?,
            event_id: parse_id("mutation_result.event_id", &result.event_id)?,
            sequence: result.sequence,
            relation: result.relation.map(edge_kind_from_proto).transpose()?,
        })
    }
}

impl From<ExplainResult> for proto::ExplainResult {
    fn from(result: ExplainResult) -> Self {
        Self {
            seeds: result
                .seeds
                .into_iter()
                .map(|seed| proto::ExplainSeed {
                    point_id: seed.point_id.to_string(),
                    score: seed.score,
                })
                .collect(),
            traversals: result
                .traversals
                .into_iter()
                .map(|traversal| proto::ExplainTraversal {
                    edge_id: traversal.edge_id.to_string(),
                    source_point_id: traversal.source_point_id.to_string(),
                    target_point_id: traversal.target_point_id.to_string(),
                    kind: edge_kind_to_proto(traversal.kind),
                    score: traversal.score,
                    depth: traversal.depth as u64,
                })
                .collect(),
            surfaces: result
                .surfaces
                .into_iter()
                .map(|surface| proto::ExplainSurface {
                    surface_id: surface.surface_id.to_string(),
                    version: surface.version,
                    activation: surface.activation,
                })
                .collect(),
            inferred_edges: result
                .inferred_edges
                .into_iter()
                .map(|id| id.to_string())
                .collect(),
            verified_fragments: result
                .verified_fragments
                .into_iter()
                .map(|id| id.to_string())
                .collect(),
            limits: Some(recall_limits_to_proto(result.limits)),
            truncated: result.truncated,
        }
    }
}

impl TryFrom<proto::ExplainResult> for ExplainResult {
    type Error = ProtoConversionError;

    fn try_from(result: proto::ExplainResult) -> Result<Self, Self::Error> {
        Ok(Self {
            seeds: result
                .seeds
                .into_iter()
                .map(|seed| {
                    Ok(crate::ExplainSeed {
                        point_id: parse_id("explain.seed.point_id", &seed.point_id)?,
                        score: seed.score,
                    })
                })
                .collect::<Result<_, ProtoConversionError>>()?,
            traversals: result
                .traversals
                .into_iter()
                .map(|traversal| {
                    Ok(crate::ExplainTraversal {
                        edge_id: parse_id("explain.traversal.edge_id", &traversal.edge_id)?,
                        source_point_id: parse_id(
                            "explain.traversal.source_point_id",
                            &traversal.source_point_id,
                        )?,
                        target_point_id: parse_id(
                            "explain.traversal.target_point_id",
                            &traversal.target_point_id,
                        )?,
                        kind: edge_kind_from_proto(traversal.kind)?,
                        score: traversal.score,
                        depth: numeric("explain.traversal.depth", traversal.depth)?,
                    })
                })
                .collect::<Result<_, ProtoConversionError>>()?,
            surfaces: result
                .surfaces
                .into_iter()
                .map(|surface| {
                    Ok(crate::ExplainSurface {
                        surface_id: parse_id("explain.surface.surface_id", &surface.surface_id)?,
                        version: surface.version,
                        activation: surface.activation,
                    })
                })
                .collect::<Result<_, ProtoConversionError>>()?,
            inferred_edges: result
                .inferred_edges
                .into_iter()
                .map(|id| parse_id("explain.inferred_edges", &id))
                .collect::<Result<_, _>>()?,
            verified_fragments: result
                .verified_fragments
                .into_iter()
                .map(|id| parse_id("explain.verified_fragments", &id))
                .collect::<Result<_, _>>()?,
            limits: recall_limits_from_proto(required(result.limits, "explain.limits")?)?,
            truncated: result.truncated,
        })
    }
}

fn recall_command_from_proto(
    command: proto::RecallCommand,
) -> Result<crate::RecallCommand, ProtoConversionError> {
    Ok(crate::RecallCommand {
        mode: recall_mode_from_proto(command.mode)?,
        text: command.text,
        entities: command.entities,
        at_time: command
            .at_time
            .map(|value| datetime_from_proto("recall.at_time", value))
            .transpose()?,
        limits: recall_limits_from_proto(required(command.limits, "recall.limits")?)?,
    })
}

fn recall_limits_to_proto(limits: RecallLimits) -> proto::RecallLimits {
    proto::RecallLimits {
        max_seeds: limits.max_seeds as u64,
        max_neighbors: limits.max_neighbors as u64,
        max_hops: limits.max_hops as u64,
        max_nodes: limits.max_nodes as u64,
        timeout_ms: limits.timeout_ms,
    }
}

fn recall_limits_from_proto(
    limits: proto::RecallLimits,
) -> Result<RecallLimits, ProtoConversionError> {
    Ok(RecallLimits {
        max_seeds: numeric("recall.limits.max_seeds", limits.max_seeds)?,
        max_neighbors: numeric("recall.limits.max_neighbors", limits.max_neighbors)?,
        max_hops: numeric("recall.limits.max_hops", limits.max_hops)?,
        max_nodes: numeric("recall.limits.max_nodes", limits.max_nodes)?,
        timeout_ms: limits.timeout_ms,
    })
}

fn memory_packet_to_proto(packet: MemoryPacket) -> proto::MemoryPacket {
    proto::MemoryPacket {
        query_id: packet.query_id.to_string(),
        intent: recall_mode_to_proto(packet.intent),
        current_states: packet
            .current_states
            .into_iter()
            .map(state_view_to_proto)
            .collect(),
        historical_states: packet
            .historical_states
            .into_iter()
            .map(state_view_to_proto)
            .collect(),
        activated_surfaces: packet
            .activated_surfaces
            .into_iter()
            .map(surface_view_to_proto)
            .collect(),
        episodes: packet
            .episodes
            .into_iter()
            .map(episode_view_to_proto)
            .collect(),
        points: packet.points.into_iter().map(point_view_to_proto).collect(),
        paths: packet.paths.into_iter().map(recall_path_to_proto).collect(),
        procedures: packet
            .procedures
            .into_iter()
            .map(procedure_view_to_proto)
            .collect(),
        constraints: packet
            .constraints
            .into_iter()
            .map(knowledge_view_to_proto)
            .collect(),
        preferences: packet
            .preferences
            .into_iter()
            .map(knowledge_view_to_proto)
            .collect(),
        past_failures: packet
            .past_failures
            .into_iter()
            .map(episode_view_to_proto)
            .collect(),
        evidence: packet
            .evidence
            .into_iter()
            .map(|value| evidence_view_to_proto(&value))
            .collect(),
        conflicts: packet
            .conflicts
            .into_iter()
            .map(conflict_view_to_proto)
            .collect(),
        missing_evidence: packet
            .missing_evidence
            .into_iter()
            .map(missing_evidence_to_proto)
            .collect(),
        coverage: Some(coverage_to_proto(packet.coverage)),
        confidence: packet.confidence,
        recommended_context: packet.recommended_context,
    }
}

fn memory_packet_from_proto(
    packet: proto::MemoryPacket,
) -> Result<MemoryPacket, ProtoConversionError> {
    Ok(MemoryPacket {
        query_id: parse_id("recall.query_id", &packet.query_id)?,
        intent: recall_mode_from_proto(packet.intent)?,
        current_states: packet
            .current_states
            .into_iter()
            .map(state_view_from_proto)
            .collect::<Result<_, _>>()?,
        historical_states: packet
            .historical_states
            .into_iter()
            .map(state_view_from_proto)
            .collect::<Result<_, _>>()?,
        activated_surfaces: packet
            .activated_surfaces
            .into_iter()
            .map(surface_view_from_proto)
            .collect::<Result<_, _>>()?,
        episodes: packet
            .episodes
            .into_iter()
            .map(episode_view_from_proto)
            .collect::<Result<_, _>>()?,
        points: packet
            .points
            .into_iter()
            .map(point_view_from_proto)
            .collect::<Result<_, _>>()?,
        paths: packet
            .paths
            .into_iter()
            .map(recall_path_from_proto)
            .collect::<Result<_, _>>()?,
        procedures: packet
            .procedures
            .into_iter()
            .map(procedure_view_from_proto)
            .collect::<Result<_, _>>()?,
        constraints: packet
            .constraints
            .into_iter()
            .map(knowledge_view_from_proto)
            .collect::<Result<_, _>>()?,
        preferences: packet
            .preferences
            .into_iter()
            .map(knowledge_view_from_proto)
            .collect::<Result<_, _>>()?,
        past_failures: packet
            .past_failures
            .into_iter()
            .map(episode_view_from_proto)
            .collect::<Result<_, _>>()?,
        evidence: packet
            .evidence
            .into_iter()
            .map(evidence_view_from_proto)
            .collect::<Result<_, _>>()?,
        conflicts: packet
            .conflicts
            .into_iter()
            .map(conflict_view_from_proto)
            .collect::<Result<_, _>>()?,
        missing_evidence: packet
            .missing_evidence
            .into_iter()
            .map(missing_evidence_from_proto)
            .collect::<Result<_, _>>()?,
        coverage: coverage_from_proto(required(packet.coverage, "recall.coverage")?)?,
        confidence: packet.confidence,
        recommended_context: packet.recommended_context,
    })
}

fn evidence_view_to_proto(value: &EvidenceView) -> proto::EvidenceView {
    proto::EvidenceView {
        fragment_id: value.fragment_id.to_string(),
        event_id: value.event_id.map(|id| id.to_string()),
        evidence_ref_id: value.evidence_ref_id.map(|id| id.to_string()),
    }
}

fn evidence_view_from_proto(
    value: proto::EvidenceView,
) -> Result<EvidenceView, ProtoConversionError> {
    Ok(EvidenceView {
        fragment_id: parse_id("recall.evidence.fragment_id", &value.fragment_id)?,
        event_id: parse_optional_id("recall.evidence.event_id", value.event_id)?,
        evidence_ref_id: parse_optional_id(
            "recall.evidence.evidence_ref_id",
            value.evidence_ref_id,
        )?,
    })
}

fn state_view_to_proto(value: StateView) -> proto::StateView {
    proto::StateView {
        state_id: value.state_id.to_string(),
        version_id: value.version_id.to_string(),
        key: value.key,
        value: Some(json_value_to_proto(value.value)),
        valid_from: Some(datetime_to_proto(value.valid_from)),
        valid_to: value.valid_to.map(datetime_to_proto),
        validity: validity_to_proto(value.validity),
        evidence: value.evidence.iter().map(evidence_view_to_proto).collect(),
        reason: value.reason,
        source_event_id: value.source_event_id.map(|id| id.to_string()),
    }
}

fn state_view_from_proto(value: proto::StateView) -> Result<StateView, ProtoConversionError> {
    Ok(StateView {
        state_id: parse_id("recall.state.state_id", &value.state_id)?,
        version_id: parse_id("recall.state.version_id", &value.version_id)?,
        key: value.key,
        value: json_value_from_proto(required(value.value, "recall.state.value")?)?,
        valid_from: datetime_from_proto(
            "recall.state.valid_from",
            required(value.valid_from, "recall.state.valid_from")?,
        )?,
        valid_to: value
            .valid_to
            .map(|time| datetime_from_proto("recall.state.valid_to", time))
            .transpose()?,
        validity: validity_from_proto(value.validity)?,
        evidence: value
            .evidence
            .into_iter()
            .map(evidence_view_from_proto)
            .collect::<Result<_, _>>()?,
        reason: value.reason,
        source_event_id: parse_optional_id("recall.state.source_event_id", value.source_event_id)?,
    })
}

fn surface_view_to_proto(value: SurfaceView) -> proto::SurfaceView {
    proto::SurfaceView {
        surface_id: value.surface_id.to_string(),
        version: value.version,
        status: surface_status_to_proto(value.status),
        member_points: value
            .member_points
            .into_iter()
            .map(|(point_id, weight)| proto::SurfaceMemberPoint {
                point_id: point_id.to_string(),
                weight,
            })
            .collect(),
        confidence: value.confidence,
        evidence: value.evidence.iter().map(evidence_view_to_proto).collect(),
    }
}

fn surface_view_from_proto(value: proto::SurfaceView) -> Result<SurfaceView, ProtoConversionError> {
    let mut member_points = BTreeMap::new();
    for member in value.member_points {
        let point_id = parse_id("recall.surface.member_points.point_id", &member.point_id)?;
        if member_points.insert(point_id, member.weight).is_some() {
            return Err(ProtoConversionError::DuplicateMapKey {
                field: "recall.surface.member_points",
                key: member.point_id,
            });
        }
    }
    Ok(SurfaceView {
        surface_id: parse_id("recall.surface.surface_id", &value.surface_id)?,
        version: value.version,
        status: surface_status_from_proto(value.status)?,
        member_points,
        confidence: value.confidence,
        evidence: value
            .evidence
            .into_iter()
            .map(evidence_view_from_proto)
            .collect::<Result<_, _>>()?,
    })
}

fn episode_view_to_proto(value: EpisodeView) -> proto::EpisodeView {
    proto::EpisodeView {
        event_id: value.event_id.to_string(),
        fragment_id: value.fragment_id.to_string(),
        occurred_at: Some(datetime_to_proto(value.occurred_at)),
        summary: value.summary,
        evidence: value.evidence.iter().map(evidence_view_to_proto).collect(),
    }
}

fn episode_view_from_proto(value: proto::EpisodeView) -> Result<EpisodeView, ProtoConversionError> {
    Ok(EpisodeView {
        event_id: parse_id("recall.episode.event_id", &value.event_id)?,
        fragment_id: parse_id("recall.episode.fragment_id", &value.fragment_id)?,
        occurred_at: datetime_from_proto(
            "recall.episode.occurred_at",
            required(value.occurred_at, "recall.episode.occurred_at")?,
        )?,
        summary: value.summary,
        evidence: value
            .evidence
            .into_iter()
            .map(evidence_view_from_proto)
            .collect::<Result<_, _>>()?,
    })
}

fn point_view_to_proto(value: PointView) -> proto::PointView {
    proto::PointView {
        point_id: value.point_id.to_string(),
        kind: point_kind_to_proto(value.kind),
        confidence: value.confidence,
        validity: validity_to_proto(value.validity),
        source_fragments: value
            .source_fragments
            .into_iter()
            .map(|id| id.to_string())
            .collect(),
    }
}

fn point_view_from_proto(value: proto::PointView) -> Result<PointView, ProtoConversionError> {
    Ok(PointView {
        point_id: parse_id("recall.point.point_id", &value.point_id)?,
        kind: point_kind_from_proto(value.kind)?,
        confidence: value.confidence,
        validity: validity_from_proto(value.validity)?,
        source_fragments: value
            .source_fragments
            .into_iter()
            .map(|id| parse_id("recall.point.source_fragments", &id))
            .collect::<Result<_, _>>()?,
    })
}

fn recall_path_to_proto(value: RecallPath) -> proto::RecallPath {
    proto::RecallPath {
        point_ids: value
            .point_ids
            .into_iter()
            .map(|id| id.to_string())
            .collect(),
        edge_ids: value
            .edge_ids
            .into_iter()
            .map(|id| id.to_string())
            .collect(),
        score: value.score,
    }
}

fn recall_path_from_proto(value: proto::RecallPath) -> Result<RecallPath, ProtoConversionError> {
    Ok(RecallPath {
        point_ids: value
            .point_ids
            .into_iter()
            .map(|id| parse_id("recall.path.point_ids", &id))
            .collect::<Result<_, _>>()?,
        edge_ids: value
            .edge_ids
            .into_iter()
            .map(|id| parse_id("recall.path.edge_ids", &id))
            .collect::<Result<_, _>>()?,
        score: value.score,
    })
}

fn procedure_view_to_proto(value: ProcedureView) -> proto::ProcedureView {
    proto::ProcedureView {
        procedure_key: value.procedure_key,
        sample_count: value.sample_count,
        success_rate: value.success_rate,
        evidence: value.evidence.iter().map(evidence_view_to_proto).collect(),
    }
}

fn procedure_view_from_proto(
    value: proto::ProcedureView,
) -> Result<ProcedureView, ProtoConversionError> {
    Ok(ProcedureView {
        procedure_key: value.procedure_key,
        sample_count: value.sample_count,
        success_rate: value.success_rate,
        evidence: value
            .evidence
            .into_iter()
            .map(evidence_view_from_proto)
            .collect::<Result<_, _>>()?,
    })
}

fn knowledge_view_to_proto(value: KnowledgeView) -> proto::KnowledgeView {
    proto::KnowledgeView {
        point_id: value.point_id.to_string(),
        kind: knowledge_kind_to_proto(value.kind),
        value: Some(json_value_to_proto(value.value)),
        confidence: value.confidence,
        validity: validity_to_proto(value.validity),
        evidence: value.evidence.iter().map(evidence_view_to_proto).collect(),
    }
}

fn knowledge_view_from_proto(
    value: proto::KnowledgeView,
) -> Result<KnowledgeView, ProtoConversionError> {
    Ok(KnowledgeView {
        point_id: parse_id("recall.knowledge.point_id", &value.point_id)?,
        kind: knowledge_kind_from_proto(value.kind)?,
        value: json_value_from_proto(required(value.value, "recall.knowledge.value")?)?,
        confidence: value.confidence,
        validity: validity_from_proto(value.validity)?,
        evidence: value
            .evidence
            .into_iter()
            .map(evidence_view_from_proto)
            .collect::<Result<_, _>>()?,
    })
}

fn conflict_view_to_proto(value: ConflictView) -> proto::ConflictView {
    proto::ConflictView {
        point_ids: value
            .point_ids
            .into_iter()
            .map(|id| id.to_string())
            .collect(),
        edge_ids: value
            .edge_ids
            .into_iter()
            .map(|id| id.to_string())
            .collect(),
        evidence: value.evidence.iter().map(evidence_view_to_proto).collect(),
    }
}

fn conflict_view_from_proto(
    value: proto::ConflictView,
) -> Result<ConflictView, ProtoConversionError> {
    Ok(ConflictView {
        point_ids: value
            .point_ids
            .into_iter()
            .map(|id| parse_id("recall.conflict.point_ids", &id))
            .collect::<Result<_, _>>()?,
        edge_ids: value
            .edge_ids
            .into_iter()
            .map(|id| parse_id("recall.conflict.edge_ids", &id))
            .collect::<Result<_, _>>()?,
        evidence: value
            .evidence
            .into_iter()
            .map(evidence_view_from_proto)
            .collect::<Result<_, _>>()?,
    })
}

fn missing_evidence_to_proto(value: MissingEvidence) -> proto::MissingEvidence {
    proto::MissingEvidence {
        point_id: value.point_id.to_string(),
        expected_edge_kinds: value
            .expected_edge_kinds
            .into_iter()
            .map(edge_kind_to_proto)
            .collect(),
        description: value.description,
    }
}

fn missing_evidence_from_proto(
    value: proto::MissingEvidence,
) -> Result<MissingEvidence, ProtoConversionError> {
    Ok(MissingEvidence {
        point_id: parse_id("recall.missing_evidence.point_id", &value.point_id)?,
        expected_edge_kinds: value
            .expected_edge_kinds
            .into_iter()
            .map(edge_kind_from_proto)
            .collect::<Result<_, _>>()?,
        description: value.description,
    })
}

fn coverage_to_proto(value: Coverage) -> proto::Coverage {
    proto::Coverage {
        seed_count: value.seed_count as u64,
        visited_nodes: value.visited_nodes as u64,
        max_depth: value.max_depth as u64,
        evidence_count: value.evidence_count as u64,
        missing_evidence_count: value.missing_evidence_count as u64,
        truncated: value.truncated,
    }
}

fn coverage_from_proto(value: proto::Coverage) -> Result<Coverage, ProtoConversionError> {
    Ok(Coverage {
        seed_count: numeric("recall.coverage.seed_count", value.seed_count)?,
        visited_nodes: numeric("recall.coverage.visited_nodes", value.visited_nodes)?,
        max_depth: numeric("recall.coverage.max_depth", value.max_depth)?,
        evidence_count: numeric("recall.coverage.evidence_count", value.evidence_count)?,
        missing_evidence_count: numeric(
            "recall.coverage.missing_evidence_count",
            value.missing_evidence_count,
        )?,
        truncated: value.truncated,
    })
}

fn fragment_content_to_proto(content: FragmentContent) -> proto::FragmentContent {
    let value = match content {
        FragmentContent::Text { text } => proto::fragment_content::Value::Text(text),
        FragmentContent::Json { value } => {
            proto::fragment_content::Value::Json(json_value_to_proto(value))
        }
        FragmentContent::Blob {
            blob_id,
            media_type,
            byte_len,
        } => proto::fragment_content::Value::Blob(proto::BlobContent {
            blob_id: blob_id.to_string(),
            media_type,
            byte_len,
        }),
    };
    proto::FragmentContent { value: Some(value) }
}

fn fragment_content_from_proto(
    content: proto::FragmentContent,
) -> Result<FragmentContent, ProtoConversionError> {
    match required(content.value, "fragment.content.value")? {
        proto::fragment_content::Value::Text(text) => Ok(FragmentContent::Text { text }),
        proto::fragment_content::Value::Json(value) => Ok(FragmentContent::Json {
            value: json_value_from_proto(value)?,
        }),
        proto::fragment_content::Value::Blob(blob) => Ok(FragmentContent::Blob {
            blob_id: parse_id("fragment.content.blob.blob_id", &blob.blob_id)?,
            media_type: blob.media_type,
            byte_len: blob.byte_len,
        }),
    }
}

fn fragment_source_to_proto(source: FragmentSource) -> proto::FragmentSource {
    proto::FragmentSource {
        source_type: source_type_to_proto(source.source_type),
        source_id: source.source_id,
        actor_id: source.actor_id,
        observer_id: source.observer_id,
        tool_id: source.tool_id,
        uri: source.uri,
    }
}

fn fragment_source_from_proto(
    source: proto::FragmentSource,
) -> Result<FragmentSource, ProtoConversionError> {
    Ok(FragmentSource {
        source_type: source_type_from_proto(source.source_type)?,
        source_id: source.source_id,
        actor_id: source.actor_id,
        observer_id: source.observer_id,
        tool_id: source.tool_id,
        uri: source.uri,
    })
}

fn evidence_to_proto(evidence: EvidenceRef) -> proto::EvidenceRef {
    proto::EvidenceRef {
        id: evidence.id.to_string(),
        fragment_id: evidence.fragment_id.map(|value| value.to_string()),
        event_id: evidence.event_id.map(|value| value.to_string()),
        method: verification_method_to_proto(evidence.method),
        note: evidence.note,
    }
}

fn evidence_from_proto(evidence: proto::EvidenceRef) -> Result<EvidenceRef, ProtoConversionError> {
    Ok(EvidenceRef {
        id: parse_id("fragment.evidence.id", &evidence.id)?,
        fragment_id: parse_optional_id("fragment.evidence.fragment_id", evidence.fragment_id)?,
        event_id: parse_optional_id("fragment.evidence.event_id", evidence.event_id)?,
        method: verification_method_from_proto(evidence.method)?,
        note: evidence.note,
    })
}

fn json_value_to_proto(value: serde_json::Value) -> proto::JsonValue {
    let kind = match value {
        serde_json::Value::Null => proto::json_value::Kind::NullValue(true),
        serde_json::Value::Bool(value) => proto::json_value::Kind::BoolValue(value),
        serde_json::Value::Number(value) => {
            if let Some(value) = value.as_i64() {
                proto::json_value::Kind::Int64Value(value)
            } else if let Some(value) = value.as_u64() {
                proto::json_value::Kind::Uint64Value(value)
            } else {
                proto::json_value::Kind::DoubleValue(
                    value.as_f64().expect("serde_json::Number 始终可转换"),
                )
            }
        }
        serde_json::Value::String(value) => proto::json_value::Kind::StringValue(value),
        serde_json::Value::Array(values) => proto::json_value::Kind::ArrayValue(proto::JsonArray {
            values: values.into_iter().map(json_value_to_proto).collect(),
        }),
        serde_json::Value::Object(values) => {
            proto::json_value::Kind::ObjectValue(json_object_to_proto(values))
        }
    };
    proto::JsonValue { kind: Some(kind) }
}

fn json_value_from_proto(
    value: proto::JsonValue,
) -> Result<serde_json::Value, ProtoConversionError> {
    match required(value.kind, "json_value.kind")? {
        proto::json_value::Kind::NullValue(_) => Ok(serde_json::Value::Null),
        proto::json_value::Kind::BoolValue(value) => Ok(serde_json::Value::Bool(value)),
        proto::json_value::Kind::Int64Value(value) => Ok(serde_json::Value::Number(value.into())),
        proto::json_value::Kind::Uint64Value(value) => Ok(serde_json::Value::Number(value.into())),
        proto::json_value::Kind::DoubleValue(value) => serde_json::Number::from_f64(value)
            .map(serde_json::Value::Number)
            .ok_or(ProtoConversionError::InvalidJsonNumber),
        proto::json_value::Kind::StringValue(value) => Ok(serde_json::Value::String(value)),
        proto::json_value::Kind::ArrayValue(values) => Ok(serde_json::Value::Array(
            values
                .values
                .into_iter()
                .map(json_value_from_proto)
                .collect::<Result<_, _>>()?,
        )),
        proto::json_value::Kind::ObjectValue(values) => {
            Ok(serde_json::Value::Object(json_object_from_proto(values)?))
        }
    }
}

fn json_object_to_proto(values: serde_json::Map<String, serde_json::Value>) -> proto::JsonObject {
    proto::JsonObject {
        entries: values
            .into_iter()
            .map(|(key, value)| proto::JsonObjectEntry {
                key,
                value: Some(json_value_to_proto(value)),
            })
            .collect(),
    }
}

fn json_object_from_proto(
    values: proto::JsonObject,
) -> Result<serde_json::Map<String, serde_json::Value>, ProtoConversionError> {
    let mut keys = BTreeSet::new();
    let mut object = serde_json::Map::new();
    for entry in values.entries {
        if !keys.insert(entry.key.clone()) {
            return Err(ProtoConversionError::DuplicateJsonKey { key: entry.key });
        }
        object.insert(
            entry.key,
            json_value_from_proto(required(entry.value, "json_object.entries.value")?)?,
        );
    }
    Ok(object)
}

fn datetime_to_proto(value: DateTime<Utc>) -> prost_types::Timestamp {
    prost_types::Timestamp {
        seconds: value.timestamp(),
        nanos: i32::try_from(value.timestamp_subsec_nanos()).expect("Timestamp 纳秒始终不超过 i32"),
    }
}

fn datetime_from_proto(
    field: &'static str,
    value: prost_types::Timestamp,
) -> Result<DateTime<Utc>, ProtoConversionError> {
    let nanos = u32::try_from(value.nanos).ok();
    nanos
        .filter(|nanos| *nanos < 1_000_000_000)
        .and_then(|nanos| DateTime::from_timestamp(value.seconds, nanos))
        .ok_or(ProtoConversionError::InvalidTimestamp {
            field,
            seconds: value.seconds,
            nanos: value.nanos,
        })
}

fn required<T>(value: Option<T>, field: &'static str) -> Result<T, ProtoConversionError> {
    value.ok_or(ProtoConversionError::MissingField { field })
}

fn parse_id<T>(field: &'static str, value: &str) -> Result<T, ProtoConversionError>
where
    T: FromStr<Err = IdParseError>,
{
    value
        .parse()
        .map_err(|source| ProtoConversionError::InvalidIdentifier { field, source })
}

fn parse_optional_id<T>(
    field: &'static str,
    value: Option<String>,
) -> Result<Option<T>, ProtoConversionError>
where
    T: FromStr<Err = IdParseError>,
{
    value.map(|value| parse_id(field, &value)).transpose()
}

fn numeric<T>(field: &'static str, value: u64) -> Result<T, ProtoConversionError>
where
    T: TryFrom<u64>,
{
    T::try_from(value).map_err(|_| ProtoConversionError::InvalidNumeric { field, value })
}

fn recall_mode_to_proto(value: RecallMode) -> i32 {
    (match value {
        RecallMode::Exact => proto::RecallMode::Exact,
        RecallMode::Semantic => proto::RecallMode::Semantic,
        RecallMode::Temporal => proto::RecallMode::Temporal,
        RecallMode::Causal => proto::RecallMode::Causal,
        RecallMode::Entity => proto::RecallMode::Entity,
        RecallMode::Associative => proto::RecallMode::Associative,
        RecallMode::Procedural => proto::RecallMode::Procedural,
        RecallMode::CurrentState => proto::RecallMode::CurrentState,
        RecallMode::HistoricalState => proto::RecallMode::HistoricalState,
        RecallMode::DeepReconstruction => proto::RecallMode::DeepReconstruction,
    }) as i32
}

fn mutation_action_to_proto(value: MutationAction) -> i32 {
    (match value {
        MutationAction::Correct => proto::MutationAction::Correct,
        MutationAction::Forget => proto::MutationAction::Forget,
    }) as i32
}

fn mutation_action_from_proto(value: i32) -> Result<MutationAction, ProtoConversionError> {
    match proto::MutationAction::try_from(value).ok() {
        Some(proto::MutationAction::Correct) => Ok(MutationAction::Correct),
        Some(proto::MutationAction::Forget) => Ok(MutationAction::Forget),
        Some(proto::MutationAction::Unspecified) | None => {
            Err(unknown("mutation_result.action", value))
        }
    }
}

fn recall_mode_from_proto(value: i32) -> Result<RecallMode, ProtoConversionError> {
    match proto::RecallMode::try_from(value).ok() {
        Some(proto::RecallMode::Exact) => Ok(RecallMode::Exact),
        Some(proto::RecallMode::Semantic) => Ok(RecallMode::Semantic),
        Some(proto::RecallMode::Temporal) => Ok(RecallMode::Temporal),
        Some(proto::RecallMode::Causal) => Ok(RecallMode::Causal),
        Some(proto::RecallMode::Entity) => Ok(RecallMode::Entity),
        Some(proto::RecallMode::Associative) => Ok(RecallMode::Associative),
        Some(proto::RecallMode::Procedural) => Ok(RecallMode::Procedural),
        Some(proto::RecallMode::CurrentState) => Ok(RecallMode::CurrentState),
        Some(proto::RecallMode::HistoricalState) => Ok(RecallMode::HistoricalState),
        Some(proto::RecallMode::DeepReconstruction) => Ok(RecallMode::DeepReconstruction),
        Some(proto::RecallMode::Unspecified) | None => Err(unknown("recall.mode", value)),
    }
}

fn point_kind_to_proto(value: PointKind) -> i32 {
    (match value {
        PointKind::Fact => proto::PointKind::Fact,
        PointKind::Decision => proto::PointKind::Decision,
        PointKind::Constraint => proto::PointKind::Constraint,
        PointKind::StateChange => proto::PointKind::StateChange,
        PointKind::Action => proto::PointKind::Action,
        PointKind::Result => proto::PointKind::Result,
        PointKind::Failure => proto::PointKind::Failure,
        PointKind::Preference => proto::PointKind::Preference,
        PointKind::Correction => proto::PointKind::Correction,
    }) as i32
}

fn point_kind_from_proto(value: i32) -> Result<PointKind, ProtoConversionError> {
    match proto::PointKind::try_from(value).ok() {
        Some(proto::PointKind::Fact) => Ok(PointKind::Fact),
        Some(proto::PointKind::Decision) => Ok(PointKind::Decision),
        Some(proto::PointKind::Constraint) => Ok(PointKind::Constraint),
        Some(proto::PointKind::StateChange) => Ok(PointKind::StateChange),
        Some(proto::PointKind::Action) => Ok(PointKind::Action),
        Some(proto::PointKind::Result) => Ok(PointKind::Result),
        Some(proto::PointKind::Failure) => Ok(PointKind::Failure),
        Some(proto::PointKind::Preference) => Ok(PointKind::Preference),
        Some(proto::PointKind::Correction) => Ok(PointKind::Correction),
        Some(proto::PointKind::Unspecified) | None => Err(unknown("recall.point.kind", value)),
    }
}

fn surface_status_to_proto(value: SurfaceStatus) -> i32 {
    (match value {
        SurfaceStatus::Active => proto::SurfaceStatus::Active,
        SurfaceStatus::Frozen => proto::SurfaceStatus::Frozen,
        SurfaceStatus::Inactive => proto::SurfaceStatus::Inactive,
        SurfaceStatus::Split => proto::SurfaceStatus::Split,
        SurfaceStatus::Merged => proto::SurfaceStatus::Merged,
    }) as i32
}

fn surface_status_from_proto(value: i32) -> Result<SurfaceStatus, ProtoConversionError> {
    match proto::SurfaceStatus::try_from(value).ok() {
        Some(proto::SurfaceStatus::Active) => Ok(SurfaceStatus::Active),
        Some(proto::SurfaceStatus::Frozen) => Ok(SurfaceStatus::Frozen),
        Some(proto::SurfaceStatus::Inactive) => Ok(SurfaceStatus::Inactive),
        Some(proto::SurfaceStatus::Split) => Ok(SurfaceStatus::Split),
        Some(proto::SurfaceStatus::Merged) => Ok(SurfaceStatus::Merged),
        Some(proto::SurfaceStatus::Unspecified) | None => {
            Err(unknown("recall.surface.status", value))
        }
    }
}

fn knowledge_kind_to_proto(value: KnowledgeKind) -> i32 {
    (match value {
        KnowledgeKind::Fact => proto::KnowledgeKind::Fact,
        KnowledgeKind::Preference => proto::KnowledgeKind::Preference,
        KnowledgeKind::Constraint => proto::KnowledgeKind::Constraint,
    }) as i32
}

fn knowledge_kind_from_proto(value: i32) -> Result<KnowledgeKind, ProtoConversionError> {
    match proto::KnowledgeKind::try_from(value).ok() {
        Some(proto::KnowledgeKind::Fact) => Ok(KnowledgeKind::Fact),
        Some(proto::KnowledgeKind::Preference) => Ok(KnowledgeKind::Preference),
        Some(proto::KnowledgeKind::Constraint) => Ok(KnowledgeKind::Constraint),
        Some(proto::KnowledgeKind::Unspecified) | None => {
            Err(unknown("recall.knowledge.kind", value))
        }
    }
}

fn edge_kind_to_proto(value: EdgeKind) -> i32 {
    (match value {
        EdgeKind::TemporalBefore => proto::EdgeKind::TemporalBefore,
        EdgeKind::TemporalAfter => proto::EdgeKind::TemporalAfter,
        EdgeKind::Causes => proto::EdgeKind::Causes,
        EdgeKind::CausedBy => proto::EdgeKind::CausedBy,
        EdgeKind::Supports => proto::EdgeKind::Supports,
        EdgeKind::Contradicts => proto::EdgeKind::Contradicts,
        EdgeKind::Supersedes => proto::EdgeKind::Supersedes,
        EdgeKind::DependsOn => proto::EdgeKind::DependsOn,
        EdgeKind::PartOf => proto::EdgeKind::PartOf,
        EdgeKind::SameEntity => proto::EdgeKind::SameEntity,
        EdgeKind::CoActivated => proto::EdgeKind::CoActivated,
        EdgeKind::PredictedBy => proto::EdgeKind::PredictedBy,
        EdgeKind::ValidatedBy => proto::EdgeKind::ValidatedBy,
        EdgeKind::DisprovedBy => proto::EdgeKind::DisprovedBy,
        EdgeKind::RecalledBy => proto::EdgeKind::RecalledBy,
        EdgeKind::UsedInDecision => proto::EdgeKind::UsedInDecision,
        EdgeKind::ExecutedAs => proto::EdgeKind::ExecutedAs,
        EdgeKind::ResultedIn => proto::EdgeKind::ResultedIn,
    }) as i32
}

fn edge_kind_from_proto(value: i32) -> Result<EdgeKind, ProtoConversionError> {
    match proto::EdgeKind::try_from(value).ok() {
        Some(proto::EdgeKind::TemporalBefore) => Ok(EdgeKind::TemporalBefore),
        Some(proto::EdgeKind::TemporalAfter) => Ok(EdgeKind::TemporalAfter),
        Some(proto::EdgeKind::Causes) => Ok(EdgeKind::Causes),
        Some(proto::EdgeKind::CausedBy) => Ok(EdgeKind::CausedBy),
        Some(proto::EdgeKind::Supports) => Ok(EdgeKind::Supports),
        Some(proto::EdgeKind::Contradicts) => Ok(EdgeKind::Contradicts),
        Some(proto::EdgeKind::Supersedes) => Ok(EdgeKind::Supersedes),
        Some(proto::EdgeKind::DependsOn) => Ok(EdgeKind::DependsOn),
        Some(proto::EdgeKind::PartOf) => Ok(EdgeKind::PartOf),
        Some(proto::EdgeKind::SameEntity) => Ok(EdgeKind::SameEntity),
        Some(proto::EdgeKind::CoActivated) => Ok(EdgeKind::CoActivated),
        Some(proto::EdgeKind::PredictedBy) => Ok(EdgeKind::PredictedBy),
        Some(proto::EdgeKind::ValidatedBy) => Ok(EdgeKind::ValidatedBy),
        Some(proto::EdgeKind::DisprovedBy) => Ok(EdgeKind::DisprovedBy),
        Some(proto::EdgeKind::RecalledBy) => Ok(EdgeKind::RecalledBy),
        Some(proto::EdgeKind::UsedInDecision) => Ok(EdgeKind::UsedInDecision),
        Some(proto::EdgeKind::ExecutedAs) => Ok(EdgeKind::ExecutedAs),
        Some(proto::EdgeKind::ResultedIn) => Ok(EdgeKind::ResultedIn),
        Some(proto::EdgeKind::Unspecified) | None => Err(unknown("recall.edge_kind", value)),
    }
}

fn fragment_kind_to_proto(value: FragmentKind) -> i32 {
    (match value {
        FragmentKind::Observation => proto::FragmentKind::Observation,
        FragmentKind::Claim => proto::FragmentKind::Claim,
        FragmentKind::Inference => proto::FragmentKind::Inference,
        FragmentKind::Prediction => proto::FragmentKind::Prediction,
        FragmentKind::Decision => proto::FragmentKind::Decision,
        FragmentKind::Action => proto::FragmentKind::Action,
        FragmentKind::ToolCall => proto::FragmentKind::ToolCall,
        FragmentKind::ToolResult => proto::FragmentKind::ToolResult,
        FragmentKind::Outcome => proto::FragmentKind::Outcome,
        FragmentKind::Correction => proto::FragmentKind::Correction,
        FragmentKind::Constraint => proto::FragmentKind::Constraint,
        FragmentKind::Preference => proto::FragmentKind::Preference,
        FragmentKind::Fact => proto::FragmentKind::Fact,
        FragmentKind::Failure => proto::FragmentKind::Failure,
        FragmentKind::Success => proto::FragmentKind::Success,
        FragmentKind::Plan => proto::FragmentKind::Plan,
        FragmentKind::Approval => proto::FragmentKind::Approval,
        FragmentKind::Rejection => proto::FragmentKind::Rejection,
        FragmentKind::StateTransition => proto::FragmentKind::StateTransition,
    }) as i32
}

fn fragment_kind_from_proto(value: i32) -> Result<FragmentKind, ProtoConversionError> {
    match proto::FragmentKind::try_from(value).ok() {
        Some(proto::FragmentKind::Observation) => Ok(FragmentKind::Observation),
        Some(proto::FragmentKind::Claim) => Ok(FragmentKind::Claim),
        Some(proto::FragmentKind::Inference) => Ok(FragmentKind::Inference),
        Some(proto::FragmentKind::Prediction) => Ok(FragmentKind::Prediction),
        Some(proto::FragmentKind::Decision) => Ok(FragmentKind::Decision),
        Some(proto::FragmentKind::Action) => Ok(FragmentKind::Action),
        Some(proto::FragmentKind::ToolCall) => Ok(FragmentKind::ToolCall),
        Some(proto::FragmentKind::ToolResult) => Ok(FragmentKind::ToolResult),
        Some(proto::FragmentKind::Outcome) => Ok(FragmentKind::Outcome),
        Some(proto::FragmentKind::Correction) => Ok(FragmentKind::Correction),
        Some(proto::FragmentKind::Constraint) => Ok(FragmentKind::Constraint),
        Some(proto::FragmentKind::Preference) => Ok(FragmentKind::Preference),
        Some(proto::FragmentKind::Fact) => Ok(FragmentKind::Fact),
        Some(proto::FragmentKind::Failure) => Ok(FragmentKind::Failure),
        Some(proto::FragmentKind::Success) => Ok(FragmentKind::Success),
        Some(proto::FragmentKind::Plan) => Ok(FragmentKind::Plan),
        Some(proto::FragmentKind::Approval) => Ok(FragmentKind::Approval),
        Some(proto::FragmentKind::Rejection) => Ok(FragmentKind::Rejection),
        Some(proto::FragmentKind::StateTransition) => Ok(FragmentKind::StateTransition),
        Some(proto::FragmentKind::Unspecified) | None => Err(unknown("fragment.kind", value)),
    }
}

fn source_type_to_proto(value: SourceType) -> i32 {
    (match value {
        SourceType::User => proto::SourceType::User,
        SourceType::Agent => proto::SourceType::Agent,
        SourceType::Model => proto::SourceType::Model,
        SourceType::Tool => proto::SourceType::Tool,
        SourceType::Import => proto::SourceType::Import,
        SourceType::System => proto::SourceType::System,
    }) as i32
}

fn source_type_from_proto(value: i32) -> Result<SourceType, ProtoConversionError> {
    match proto::SourceType::try_from(value).ok() {
        Some(proto::SourceType::User) => Ok(SourceType::User),
        Some(proto::SourceType::Agent) => Ok(SourceType::Agent),
        Some(proto::SourceType::Model) => Ok(SourceType::Model),
        Some(proto::SourceType::Tool) => Ok(SourceType::Tool),
        Some(proto::SourceType::Import) => Ok(SourceType::Import),
        Some(proto::SourceType::System) => Ok(SourceType::System),
        Some(proto::SourceType::Unspecified) | None => {
            Err(unknown("fragment.source.source_type", value))
        }
    }
}

fn perspective_to_proto(value: Perspective) -> i32 {
    (match value {
        Perspective::FirstPerson => proto::Perspective::FirstPerson,
        Perspective::SecondPerson => proto::Perspective::SecondPerson,
        Perspective::ThirdPerson => proto::Perspective::ThirdPerson,
        Perspective::System => proto::Perspective::System,
        Perspective::Tool => proto::Perspective::Tool,
    }) as i32
}

fn perspective_from_proto(value: i32) -> Result<Perspective, ProtoConversionError> {
    match proto::Perspective::try_from(value).ok() {
        Some(proto::Perspective::FirstPerson) => Ok(Perspective::FirstPerson),
        Some(proto::Perspective::SecondPerson) => Ok(Perspective::SecondPerson),
        Some(proto::Perspective::ThirdPerson) => Ok(Perspective::ThirdPerson),
        Some(proto::Perspective::System) => Ok(Perspective::System),
        Some(proto::Perspective::Tool) => Ok(Perspective::Tool),
        Some(proto::Perspective::Unspecified) | None => Err(unknown("fragment.perspective", value)),
    }
}

fn validity_to_proto(value: ValidityStatus) -> i32 {
    (match value {
        ValidityStatus::Unverified => proto::ValidityStatus::Unverified,
        ValidityStatus::Verified => proto::ValidityStatus::Verified,
        ValidityStatus::Disputed => proto::ValidityStatus::Disputed,
        ValidityStatus::Superseded => proto::ValidityStatus::Superseded,
        ValidityStatus::Invalidated => proto::ValidityStatus::Invalidated,
        ValidityStatus::Quarantined => proto::ValidityStatus::Quarantined,
    }) as i32
}

fn validity_from_proto(value: i32) -> Result<ValidityStatus, ProtoConversionError> {
    match proto::ValidityStatus::try_from(value).ok() {
        Some(proto::ValidityStatus::Unverified) => Ok(ValidityStatus::Unverified),
        Some(proto::ValidityStatus::Verified) => Ok(ValidityStatus::Verified),
        Some(proto::ValidityStatus::Disputed) => Ok(ValidityStatus::Disputed),
        Some(proto::ValidityStatus::Superseded) => Ok(ValidityStatus::Superseded),
        Some(proto::ValidityStatus::Invalidated) => Ok(ValidityStatus::Invalidated),
        Some(proto::ValidityStatus::Quarantined) => Ok(ValidityStatus::Quarantined),
        Some(proto::ValidityStatus::Unspecified) | None => Err(unknown("fragment.validity", value)),
    }
}

fn security_label_to_proto(value: SecurityLabel) -> i32 {
    (match value {
        SecurityLabel::Public => proto::SecurityLabel::Public,
        SecurityLabel::Internal => proto::SecurityLabel::Internal,
        SecurityLabel::Confidential => proto::SecurityLabel::Confidential,
        SecurityLabel::Restricted => proto::SecurityLabel::Restricted,
    }) as i32
}

fn security_label_from_proto(value: i32) -> Result<SecurityLabel, ProtoConversionError> {
    match proto::SecurityLabel::try_from(value).ok() {
        Some(proto::SecurityLabel::Public) => Ok(SecurityLabel::Public),
        Some(proto::SecurityLabel::Internal) => Ok(SecurityLabel::Internal),
        Some(proto::SecurityLabel::Confidential) => Ok(SecurityLabel::Confidential),
        Some(proto::SecurityLabel::Restricted) => Ok(SecurityLabel::Restricted),
        Some(proto::SecurityLabel::Unspecified) | None => {
            Err(unknown("fragment.security_label", value))
        }
    }
}

fn verification_method_to_proto(value: VerificationMethod) -> i32 {
    (match value {
        VerificationMethod::Manual => proto::VerificationMethod::Manual,
        VerificationMethod::Tool => proto::VerificationMethod::Tool,
        VerificationMethod::ExternalSource => proto::VerificationMethod::ExternalSource,
        VerificationMethod::CrossEvidence => proto::VerificationMethod::CrossEvidence,
    }) as i32
}

fn verification_method_from_proto(value: i32) -> Result<VerificationMethod, ProtoConversionError> {
    match proto::VerificationMethod::try_from(value).ok() {
        Some(proto::VerificationMethod::Manual) => Ok(VerificationMethod::Manual),
        Some(proto::VerificationMethod::Tool) => Ok(VerificationMethod::Tool),
        Some(proto::VerificationMethod::ExternalSource) => Ok(VerificationMethod::ExternalSource),
        Some(proto::VerificationMethod::CrossEvidence) => Ok(VerificationMethod::CrossEvidence),
        Some(proto::VerificationMethod::Unspecified) | None => {
            Err(unknown("fragment.evidence.method", value))
        }
    }
}

const fn unknown(field: &'static str, value: i32) -> ProtoConversionError {
    ProtoConversionError::UnknownEnum { field, value }
}
