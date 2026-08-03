//! Embedded Engine 外部 Quarantine 策略加载契约测试。

use chrono::Utc;
use jc_memory_core::{MemoryConfig, MemoryEngine};
use jc_memory_protocol::{
    AccessContext, CognitiveFragment, FragmentContent, FragmentId, FragmentKind, FragmentSource,
    Grant, MemoryScope, NamespaceId, Operation, Perspective, SecurityLabel, SourceType,
    ValidityStatus,
};
use jc_memory_security::RiskLabel;

const API_KEY_SCOPE: &str = "fixture";

fn policy_yaml() -> &'static str {
    r"
schema_version: 1
policy_version: external-fixture-1
quarantine_threshold: 0.8
repeated_disputed_threshold: 2
prompt_injection:
  instruction_override_markers: [fixture-only-injection]
  hidden_prompt_markers: [fixture-only-hidden]
  disclosure_markers: [fixture-only-disclose]
  score: 0.95
sensitive_data:
  markers: [fixture-only-secret=]
  score: 0.9
repeated_disputed_claim:
  score: 0.85
suspicious_tool_result:
  verified_markers: [fixture-only-verified=true]
  authorization_markers: [fixture-only-authorization=granted]
  score: 0.9
cross_scope_reference_score: 1.0
    "
}

fn context(scope: &MemoryScope) -> AccessContext {
    let mut context = AccessContext::new(API_KEY_SCOPE, scope.clone());
    context.grants.extend(
        [Operation::Observe, Operation::Recall]
            .into_iter()
            .map(|operation| Grant::for_scope(operation, scope)),
    );
    context
}

fn fragment(scope: MemoryScope, text: &str) -> CognitiveFragment {
    let now = Utc::now();
    CognitiveFragment {
        id: FragmentId::new(),
        scope,
        session_id: None,
        trace_id: None,
        kind: FragmentKind::Fact,
        content: FragmentContent::Text {
            text: text.to_owned(),
        },
        source: FragmentSource {
            source_type: SourceType::System,
            source_id: "external-policy-test".to_owned(),
            actor_id: None,
            observer_id: None,
            tool_id: None,
            uri: None,
        },
        perspective: Perspective::System,
        occurred_at: now,
        observed_at: now,
        confidence: 1.0,
        importance: 0.5,
        validity: ValidityStatus::Unverified,
        security_label: SecurityLabel::Internal,
        evidence: Vec::new(),
        metadata: serde_json::Map::new(),
    }
}

#[tokio::test]
async fn engine_uses_external_security_policy_for_observe() {
    let data = tempfile::tempdir().expect("创建临时数据目录");
    let rules = data.path().join("quarantine.yaml");
    tokio::fs::write(&rules, policy_yaml())
        .await
        .expect("写入外部策略");
    let engine =
        MemoryEngine::open(MemoryConfig::local(data.path()).with_security_rules_file(rules))
            .await
            .expect("加载外部策略");
    let scope = MemoryScope::new(NamespaceId::new());
    let receipt = engine
        .observe(
            &context(&scope),
            fragment(scope, "fixture-only-injection fixture-only-disclose"),
        )
        .await
        .expect("写入 Fixture Fragment");

    assert!(receipt.quarantined);
    assert!(
        receipt
            .risk_assessment
            .labels
            .contains(&RiskLabel::PromptInjection)
    );
    assert_eq!(receipt.risk_assessment.policy_version, "external-fixture-1");
}

#[tokio::test]
async fn engine_fails_closed_when_external_security_policy_is_missing() {
    let data = tempfile::tempdir().expect("创建临时数据目录");
    let missing = data.path().join("missing-quarantine.yaml");
    let result =
        MemoryEngine::open(MemoryConfig::local(data.path()).with_security_rules_file(missing))
            .await;
    let Err(error) = result else {
        panic!("缺失策略必须阻止 Engine 启动");
    };

    assert!(error.to_string().contains("quarantine"));
}
