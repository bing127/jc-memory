//! Policy、Approval、Tool 执行和 Commit 闭环测试。

use std::{
    collections::BTreeSet,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use async_trait::async_trait;
use jc_memory_protocol::{AccessContext, Grant, MemoryScope, NamespaceId, Operation};
use jc_memory_runtime::{
    ApprovalDecision, ApprovalStore, RiskLevel, ToolCommit, ToolDescriptor, ToolError,
    ToolExecutor, ToolIdempotency, ToolInvocation, ToolLoop, ToolRegistry,
};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

struct CountingTool {
    calls: Arc<AtomicUsize>,
    fail: bool,
}

#[async_trait]
impl ToolExecutor for CountingTool {
    async fn execute(
        &self,
        _arguments: Value,
        _cancellation: CancellationToken,
    ) -> Result<Value, ToolError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.fail {
            Err(ToolError::Execution("fixture failure".into()))
        } else {
            Ok(json!({"ok":true}))
        }
    }
}

fn access(scope: &MemoryScope) -> AccessContext {
    let mut access = AccessContext::new("principal", scope.clone());
    access
        .grants
        .insert(Grant::for_scope(Operation::Mutate, scope));
    access
}

fn descriptor(risk: RiskLevel, requires_approval: bool) -> ToolDescriptor {
    ToolDescriptor {
        id: "fixture.tool".into(),
        version: "1.0.0".parse().unwrap(),
        permissions: BTreeSet::default(),
        risk,
        requires_approval,
        idempotency: ToolIdempotency::Idempotent,
        input_schema: json!({"type":"object"}),
        output_schema: json!({"type":"object"}),
    }
}

fn invocation() -> ToolInvocation {
    ToolInvocation {
        id: "fixture.tool".into(),
        version: "^1".parse().unwrap(),
        arguments: json!({"value":1}),
        prediction: Some("工具应成功".into()),
    }
}

#[tokio::test]
async fn high_risk_tool_requires_approval_without_execution() {
    let calls = Arc::new(AtomicUsize::new(0));
    let registry = ToolRegistry::new();
    registry
        .register(
            descriptor(RiskLevel::High, true),
            Arc::new(CountingTool {
                calls: calls.clone(),
                fail: false,
            }),
        )
        .unwrap();
    let scope = MemoryScope::new(NamespaceId::new());
    let loop_ = ToolLoop::new(registry, ApprovalStore::new());
    let result = loop_
        .execute(&access(&scope), invocation(), CancellationToken::new())
        .await;
    assert!(matches!(result, Err(ToolError::ApprovalRequired(_))));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn rejected_approval_does_not_execute_tool() {
    let calls = Arc::new(AtomicUsize::new(0));
    let registry = ToolRegistry::new();
    registry
        .register(
            descriptor(RiskLevel::High, true),
            Arc::new(CountingTool {
                calls: calls.clone(),
                fail: false,
            }),
        )
        .unwrap();
    let scope = MemoryScope::new(NamespaceId::new());
    let approvals = ApprovalStore::new();
    let loop_ = ToolLoop::new(registry, approvals.clone());
    let pending = loop_
        .request_approval(&access(&scope), &invocation())
        .unwrap();
    approvals
        .decide(pending.id, ApprovalDecision::Rejected)
        .unwrap();
    let result = loop_
        .execute(&access(&scope), invocation(), CancellationToken::new())
        .await;
    assert!(matches!(result, Err(ToolError::ApprovalRejected(_))));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn approved_success_commits_validation_relation() {
    let registry = ToolRegistry::new();
    registry
        .register(
            descriptor(RiskLevel::High, true),
            Arc::new(CountingTool {
                calls: Arc::new(AtomicUsize::new(0)),
                fail: false,
            }),
        )
        .unwrap();
    let scope = MemoryScope::new(NamespaceId::new());
    let approvals = ApprovalStore::new();
    let loop_ = ToolLoop::new(registry, approvals.clone());
    let pending = loop_
        .request_approval(&access(&scope), &invocation())
        .unwrap();
    approvals
        .decide(pending.id, ApprovalDecision::Approved)
        .unwrap();
    let commit = loop_
        .execute(&access(&scope), invocation(), CancellationToken::new())
        .await
        .unwrap();
    assert!(commit.tool_result.success);
    assert_eq!(
        commit.prediction_relation,
        Some(jc_memory_runtime::PredictionRelation::ValidatedBy)
    );
    assert!(!commit.writes_fact);
}

#[tokio::test]
async fn failed_tool_commits_disproof_without_fact_promotion() {
    let registry = ToolRegistry::new();
    registry
        .register(
            descriptor(RiskLevel::Low, false),
            Arc::new(CountingTool {
                calls: Arc::new(AtomicUsize::new(0)),
                fail: true,
            }),
        )
        .unwrap();
    let scope = MemoryScope::new(NamespaceId::new());
    let loop_ = ToolLoop::new(registry, ApprovalStore::new());
    let commit: ToolCommit = loop_
        .execute(&access(&scope), invocation(), CancellationToken::new())
        .await
        .unwrap();
    assert!(!commit.tool_result.success);
    assert_eq!(
        commit.prediction_relation,
        Some(jc_memory_runtime::PredictionRelation::DisprovedBy)
    );
    assert!(!commit.writes_fact);
    assert_eq!(commit.outcome.kind, "failure");
}
