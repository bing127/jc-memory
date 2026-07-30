//! 疆诚记忆的默认拒绝访问控制与安全边界。

mod access;
mod acl;
mod guard;
mod injection;
mod poisoning;
mod quarantine;
mod sensitive;

pub use acl::AclEngine;
pub use guard::{OutputGuard, SecurityInvariantViolation};
pub use jc_memory_protocol::{AccessContext, Decision, DenyReason, Grant, Operation};
pub use quarantine::{
    Admission, QuarantinePipeline, QuarantineReason, RiskAssessment, RiskContext, RiskLabel,
};
