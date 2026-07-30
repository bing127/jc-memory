//! Core 操作的检索前授权与返回前安全复核。

use std::collections::BTreeMap;

use jc_memory_protocol::{
    AccessContext, Decision, FragmentId, MemoryPacket, MemoryScope, Operation,
};
use jc_memory_security::{AclEngine, OutputGuard};

use crate::{MemoryError, engine::short_hash};

pub(crate) fn authorize(
    context: &AccessContext,
    operation: Operation,
    resource: &MemoryScope,
) -> Result<(), MemoryError> {
    match AclEngine.authorize(context, operation, resource) {
        Decision::Allow => Ok(()),
        Decision::Deny(reason) => {
            tracing::warn!(
                target: "jc_memory::security_audit",
                principal_id = %context.principal_id,
                request_id = %context.request_id,
                ?operation,
                namespace_hash = %short_hash(&resource.namespace_id.to_string()),
                tenant_hash = resource.tenant_id.map(|id| short_hash(&id.to_string())),
                ?reason,
                result = "deny",
                "安全预授权拒绝核心操作"
            );
            Err(MemoryError::AccessDenied { operation, reason })
        }
    }
}

pub(crate) fn validate_packet(
    context: &AccessContext,
    operation: Operation,
    resource: &MemoryScope,
    packet: &MemoryPacket,
    fragment_scopes: &BTreeMap<FragmentId, MemoryScope>,
) -> Result<(), MemoryError> {
    OutputGuard
        .filter_and_assert(context, operation, resource, packet, fragment_scopes)
        .map_err(MemoryError::SecurityInvariantViolation)
}
