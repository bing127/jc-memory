//! 疆诚记忆的默认拒绝访问控制与安全边界。

mod access;
mod acl;

pub use acl::AclEngine;
pub use jc_memory_protocol::{AccessContext, Decision, DenyReason, Grant, Operation};
