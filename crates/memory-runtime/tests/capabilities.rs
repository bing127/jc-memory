//! Capability 注册、ACL 解析和统计契约测试。

use std::{collections::BTreeSet, sync::Arc};

use async_trait::async_trait;
use jc_memory_protocol::{AccessContext, EvidenceId, Grant, MemoryScope, NamespaceId, Operation};
use jc_memory_runtime::{
    CapabilityDescriptor, CapabilityError, CapabilityExecutor, CapabilityKind, CapabilityRegistry,
    CapabilityRequest, CapabilityStatsStore, Permission, RiskLevel,
};
use semver::{Version, VersionReq};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

struct EchoExecutor;

#[async_trait]
impl CapabilityExecutor for EchoExecutor {
    async fn execute(
        &self,
        arguments: Value,
        _cancellation: CancellationToken,
    ) -> Result<Value, CapabilityError> {
        Ok(arguments)
    }
}

fn descriptor(version: &str, permissions: &[Permission]) -> CapabilityDescriptor {
    CapabilityDescriptor {
        id: "fixture.echo".into(),
        version: Version::parse(version).unwrap(),
        kind: CapabilityKind::Tool,
        source: "fixture".into(),
        permissions: permissions.iter().cloned().collect(),
        input_schema: json!({"type":"object"}),
        output_schema: json!({"type":"object"}),
        risk: RiskLevel::Low,
        requires_approval: false,
        surfaces: Vec::new(),
    }
}

fn access(scope: &MemoryScope, operation: Operation) -> AccessContext {
    let mut context = AccessContext::new("principal", scope.clone());
    context.grants.insert(Grant::for_scope(operation, scope));
    context
}

#[test]
fn registry_rejects_invalid_and_duplicate_versions() {
    let registry = CapabilityRegistry::new();
    assert!(
        registry
            .register(descriptor("1.0.0", &[]), Arc::new(EchoExecutor))
            .is_ok()
    );
    assert!(
        registry
            .register(descriptor("1.0.0", &[]), Arc::new(EchoExecutor))
            .is_err()
    );
    let mut invalid = descriptor("1.0.1", &[]);
    invalid.id = " ".into();
    assert!(registry.register(invalid, Arc::new(EchoExecutor)).is_err());
}

#[test]
fn resolve_requires_scope_grants_for_declared_permissions() {
    let registry = CapabilityRegistry::new();
    registry
        .register(
            descriptor("1.2.0", &[Permission::ReadMemory]),
            Arc::new(EchoExecutor),
        )
        .unwrap();
    let scope = MemoryScope::new(NamespaceId::new());
    let request = CapabilityRequest {
        id: "fixture.echo".into(),
        version: VersionReq::parse("^1").unwrap(),
    };
    assert!(
        registry
            .resolve(&request, &AccessContext::new("principal", scope.clone()))
            .is_err()
    );
    let resolved = registry
        .resolve(&request, &access(&scope, Operation::Recall))
        .expect("Recall Grant 应满足 ReadMemory");
    assert_eq!(
        resolved.descriptor.version,
        Version::parse("1.2.0").unwrap()
    );
}

#[test]
fn resolve_selects_highest_matching_version_and_exposes_executor() {
    let registry = CapabilityRegistry::new();
    registry
        .register(descriptor("1.0.0", &[]), Arc::new(EchoExecutor))
        .unwrap();
    registry
        .register(descriptor("1.4.0", &[]), Arc::new(EchoExecutor))
        .unwrap();
    let scope = MemoryScope::new(NamespaceId::new());
    let request = CapabilityRequest {
        id: "fixture.echo".into(),
        version: VersionReq::parse("^1").unwrap(),
    };
    let resolved = registry
        .resolve(&request, &AccessContext::new("principal", scope))
        .unwrap();
    assert_eq!(
        resolved.descriptor.version,
        Version::parse("1.4.0").unwrap()
    );
    assert!(Arc::ptr_eq(
        &resolved.executor,
        &registry.executor(&resolved.descriptor).unwrap()
    ));
}

#[test]
fn stats_rank_only_after_minimum_samples_with_beta_smoothing() {
    let store = CapabilityStatsStore::new();
    let version = Version::parse("1.0.0").unwrap();
    let evidence = EvidenceId::new();
    store.record("fixture.echo", &version, true, None, [evidence]);
    let hidden = store.rank("fixture.echo", 2);
    assert!(hidden.is_empty(), "样本不足时不得排序");
    store.record("fixture.echo", &version, false, Some("timeout"), []);
    let ranked = store.rank("fixture.echo", 2);
    assert_eq!(ranked.len(), 1);
    assert_eq!(ranked[0].stats.runs, 2);
    assert_eq!(ranked[0].stats.failures, 1);
    assert_eq!(ranked[0].stats.failure_modes.get("timeout"), Some(&1));
    assert_eq!(ranked[0].stats.evidence, BTreeSet::from([evidence]));
    assert!((ranked[0].smoothed_success_rate - 0.5).abs() < f64::EPSILON);
}

#[test]
fn stats_do_not_create_facts_from_model_outcomes() {
    let store = CapabilityStatsStore::new();
    let version = Version::parse("1.0.0").unwrap();
    store.record("fixture.echo", &version, true, None, []);
    let stats = store.get("fixture.echo", &version).unwrap();
    assert_eq!(stats.evidence.len(), 0);
    assert_eq!(stats.runs, 1);
}
