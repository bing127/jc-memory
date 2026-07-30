//! Stable domain and wire types for Jiangcheng Memory.

/// Embedded 与远程服务共享的命令 Envelope。
pub mod command;
/// Immutable event protocol types.
pub mod event;
/// Cognitive fragment domain types.
pub mod fragment;
/// Cognitive graph protocol types.
pub mod graph;
/// Strongly typed identifiers used across protocol boundaries.
pub mod ids;
/// Model-independent Memory Packet wire types.
pub mod packet;
/// Rust 领域协议与生成 Protobuf 类型之间的转换。
pub mod protobuf;
/// Recall request protocol types.
pub mod recall;
/// Hierarchical memory ownership scope.
pub mod scope;
/// 访问控制协议类型。
pub mod security;
/// Cognitive surface protocol types.
pub mod surface;

/// 由稳定 Protobuf 契约在构建目录中生成的类型。
#[allow(missing_docs)]
pub mod generated {
    /// 疆诚记忆 Protobuf 包。
    pub mod jiangcheng {
        /// 记忆服务 Protobuf 包。
        pub mod memory {
            /// v1 稳定协议，由 Prost 自动生成。
            #[allow(clippy::all, clippy::pedantic, missing_docs)]
            pub mod v1 {
                include!(concat!(env!("OUT_DIR"), "/jiangcheng.memory.v1.rs"));
            }
        }
    }
}

pub use command::{
    API_VERSION_V1, AccessScope, CommandErrorCode, MemoryCommand, MemoryRequest, MemoryResponse,
    ObserveCommand, ObserveResult, RequestValidationError, ResponseEnvelope,
};
pub use event::{CognitiveStage, EventDraft, MemoryEvent};
pub use fragment::{
    CognitiveFragment, EvidenceRef, FragmentContent, FragmentKind, FragmentSource, Metadata,
    Perspective, SecurityLabel, SourceType, ValidityStatus, VerificationMethod,
};
pub use graph::{EdgeKind, PointCreationError, PointKind};
pub use ids::{
    AgentId, BlobId, EdgeId, EventId, EvidenceId, FragmentId, IdParseError, NamespaceId, PointId,
    ProjectId, RecallId, SessionId, StateId, StateVersionId, SurfaceId, TenantId, TraceId, UserId,
};
pub use packet::{
    ConflictView, Coverage, EpisodeView, EvidenceView, KnowledgeKind, KnowledgeView, MemoryPacket,
    MissingEvidence, PointView, ProcedureView, RecallPath, StateView, SurfaceView,
};
pub use protobuf::ProtoConversionError;
pub use recall::RecallMode;
pub use scope::MemoryScope;
pub use security::{AccessContext, Decision, DenyReason, Grant, Operation};
pub use surface::{SurfaceKind, SurfaceStatus};
