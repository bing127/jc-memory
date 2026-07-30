//! Memory Packet 返回前的独立授权与来源复核。

use std::collections::{BTreeMap, BTreeSet};

use jc_memory_protocol::{
    AccessContext, Decision, DenyReason, EpisodeView, EvidenceView, FragmentId, MemoryPacket,
    MemoryScope, Operation,
};
use thiserror::Error;

use crate::AclEngine;

/// Packet 返回前发现的安全不变量错误。
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum SecurityInvariantViolation {
    /// 返回阶段的 ACL 复核拒绝了请求。
    #[error("packet access denied: {0:?}")]
    AccessDenied(DenyReason),
    /// Packet 引用了未出现在授权来源表中的 Fragment。
    #[error("packet references unknown fragment {0}")]
    UnknownFragment(FragmentId),
    /// Packet 引用了另一个 Scope 的 Fragment。
    #[error("packet references fragment {0} from another scope")]
    FragmentScopeMismatch(FragmentId),
}

/// 在序列化前复核整个 Packet 的 ACL 和 Fragment 来源。
#[derive(Clone, Copy, Debug, Default)]
pub struct OutputGuard;

impl OutputGuard {
    /// 验证整个 Packet；失败时不返回经过删减的部分结果。
    ///
    /// # Errors
    ///
    /// ACL 复核失败、Fragment 来源未知或来源 Scope 不一致时返回错误。
    pub fn filter_and_assert(
        self,
        context: &AccessContext,
        operation: Operation,
        resource: &MemoryScope,
        packet: &MemoryPacket,
        fragment_scopes: &BTreeMap<FragmentId, MemoryScope>,
    ) -> Result<(), SecurityInvariantViolation> {
        if let Decision::Deny(reason) = AclEngine.authorize(context, operation, resource) {
            audit_denial(context, operation, None, "access_denied");
            return Err(SecurityInvariantViolation::AccessDenied(reason));
        }

        for fragment_id in referenced_fragments(packet) {
            let Some(scope) = fragment_scopes.get(&fragment_id) else {
                audit_denial(context, operation, Some(fragment_id), "unknown_fragment");
                return Err(SecurityInvariantViolation::UnknownFragment(fragment_id));
            };
            if scope != resource {
                audit_denial(context, operation, Some(fragment_id), "scope_mismatch");
                return Err(SecurityInvariantViolation::FragmentScopeMismatch(
                    fragment_id,
                ));
            }
        }
        Ok(())
    }
}

fn referenced_fragments(packet: &MemoryPacket) -> BTreeSet<FragmentId> {
    let mut ids = BTreeSet::new();
    for state in packet
        .current_states
        .iter()
        .chain(&packet.historical_states)
    {
        collect_evidence(&mut ids, &state.evidence);
    }
    for surface in &packet.activated_surfaces {
        collect_evidence(&mut ids, &surface.evidence);
    }
    for episode in packet.episodes.iter().chain(&packet.past_failures) {
        collect_episode(&mut ids, episode);
    }
    for point in &packet.points {
        ids.extend(point.source_fragments.iter().copied());
    }
    for procedure in &packet.procedures {
        collect_evidence(&mut ids, &procedure.evidence);
    }
    for knowledge in packet.constraints.iter().chain(&packet.preferences) {
        collect_evidence(&mut ids, &knowledge.evidence);
    }
    for conflict in &packet.conflicts {
        collect_evidence(&mut ids, &conflict.evidence);
    }
    collect_evidence(&mut ids, &packet.evidence);
    ids
}

fn collect_episode(ids: &mut BTreeSet<FragmentId>, episode: &EpisodeView) {
    ids.insert(episode.fragment_id);
    collect_evidence(ids, &episode.evidence);
}

fn collect_evidence(ids: &mut BTreeSet<FragmentId>, evidence: &[EvidenceView]) {
    ids.extend(evidence.iter().map(|item| item.fragment_id));
}

fn audit_denial(
    context: &AccessContext,
    operation: Operation,
    fragment_id: Option<FragmentId>,
    reason: &'static str,
) {
    tracing::warn!(
        target: "jc_memory::security_audit",
        principal_id = %context.principal_id,
        request_id = %context.request_id,
        ?operation,
        fragment_id = fragment_id.map(|id| id.to_string()),
        reason,
        result = "deny",
        "安全返回复核拒绝 Memory Packet"
    );
}
