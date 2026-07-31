//! Rust 领域协议与生成 Protobuf 类型之间的无损转换。

use std::{collections::BTreeSet, str::FromStr};

use chrono::{DateTime, Utc};

use crate::{
    AccessScope, CognitiveFragment, EvidenceRef, FragmentContent, FragmentKind, FragmentSource,
    IdParseError, MemoryCommand, MemoryRequest, MemoryResponse, MemoryScope, ObserveCommand,
    ObserveResult, Perspective, RequestValidationError, ResponseEnvelope, SecurityLabel,
    SourceType, ValidityStatus, VerificationMethod, generated::jiangcheng::memory::v1 as proto,
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
            MemoryCommand::Recall(_) => {
                return Err(ProtoConversionError::Unsupported {
                    kind: "recall command",
                });
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
            MemoryResponse::Recall(_) => {
                return Err(ProtoConversionError::Unsupported {
                    kind: "recall response",
                });
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
