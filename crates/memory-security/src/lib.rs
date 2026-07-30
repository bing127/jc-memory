//! 疆诚记忆的默认拒绝访问控制与安全边界。

mod access;
mod acl;
mod audit;
mod delete;
mod export;
mod guard;
mod injection;
mod key;
mod poisoning;
mod quarantine;
mod retention;
mod sensitive;

pub use acl::AclEngine;
pub use audit::{AuditRecord, AuditResult};
pub use delete::{GovernanceError, GovernanceService, HardDeleteReceipt};
pub use export::GovernanceExportReceipt;
pub use guard::{OutputGuard, SecurityInvariantViolation};
pub use jc_memory_protocol::{AccessContext, Decision, DenyReason, Grant, Operation};
pub use key::{InMemoryKeyProvider, KeyError, KeyProvider, KeyRotationMetadata};
pub use quarantine::{
    Admission, QuarantinePipeline, QuarantineReason, RiskAssessment, RiskContext, RiskLabel,
};
pub use retention::{RetentionPlan, RetentionPolicy};
