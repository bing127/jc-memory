//! 版本化 Quarantine 策略的严格解析与真实准入行为测试。

use chrono::Utc;
use jc_memory_protocol::{
    CognitiveFragment, FragmentContent, FragmentId, FragmentKind, FragmentSource, MemoryScope,
    NamespaceId, Perspective, SecurityLabel, SourceType, ValidityStatus,
};
use jc_memory_security::{Admission, QuarantinePipeline, QuarantinePolicy, RiskContext, RiskLabel};

fn valid_yaml() -> &'static str {
    r"
schema_version: 1
policy_version: fixture-1
quarantine_threshold: 0.8
repeated_disputed_threshold: 2
prompt_injection:
  instruction_override_markers:
    - ignore fixture instruction
  hidden_prompt_markers:
    - fixture hidden prompt
  disclosure_markers:
    - fixture disclose
  score: 0.95
sensitive_data:
  markers:
    - fixture-secret=
  score: 0.9
repeated_disputed_claim:
  score: 0.85
suspicious_tool_result:
  verified_markers:
    - fixture verified=true
  authorization_markers:
    - fixture authorization=granted
  score: 0.9
cross_scope_reference_score: 1.0
    "
}

fn fragment(text: &str) -> CognitiveFragment {
    let now = Utc::now();
    CognitiveFragment {
        id: FragmentId::new(),
        scope: MemoryScope::new(NamespaceId::new()),
        session_id: None,
        trace_id: None,
        kind: FragmentKind::Fact,
        content: FragmentContent::Text {
            text: text.to_owned(),
        },
        source: FragmentSource {
            source_type: SourceType::System,
            source_id: "quarantine-policy-test".to_owned(),
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

#[test]
fn 严格解析拒绝未知字段空词项重复词项和非法分数() {
    let policy = QuarantinePolicy::from_yaml_str(valid_yaml()).expect("解析有效策略");
    assert_eq!(policy.version(), "fixture-1");
    assert_eq!(policy.sha256().len(), 64);

    assert!(QuarantinePolicy::from_yaml_str(&format!("{}\nunknown: true", valid_yaml())).is_err());
    assert!(
        QuarantinePolicy::from_yaml_str(&valid_yaml().replace("- fixture-secret=", "- ' '"))
            .is_err()
    );
    assert!(
        QuarantinePolicy::from_yaml_str(&valid_yaml().replace(
            "- fixture-secret=",
            "- fixture-secret=\n    - fixture-secret="
        ))
        .is_err()
    );
    assert!(
        QuarantinePolicy::from_yaml_str(&valid_yaml().replace("score: 0.9", "score: .NaN"))
            .is_err()
    );
    assert!(
        QuarantinePolicy::from_yaml_str(
            &valid_yaml().replace("quarantine_threshold: 0.8", "quarantine_threshold: 1.1")
        )
        .is_err()
    );
    assert!(
        QuarantinePolicy::from_yaml_str(&valid_yaml().replace(
            "repeated_disputed_threshold: 2",
            "repeated_disputed_threshold: 0"
        ))
        .is_err()
    );
}

#[test]
fn 自定义词项能够触发检测且原始内容保持不变() {
    let policy = QuarantinePolicy::from_yaml_str(valid_yaml()).expect("解析 Fixture 策略");
    let pipeline = QuarantinePipeline::new(policy);
    let original = fragment("IGNORE FIXTURE INSTRUCTION and fixture disclose");
    let original_content = original.content.clone();
    let original_evidence = original.evidence.clone();

    let assessment = pipeline.assess(&original, RiskContext::default());
    assert!(assessment.labels.contains(&RiskLabel::PromptInjection));
    assert_eq!(original.content, original_content);
    assert_eq!(original.evidence, original_evidence);
    assert!(matches!(
        pipeline.admission(&original, &assessment),
        Admission::Quarantine { .. }
    ));
}

#[test]
fn 自定义隔离阈值会影响准入结果() {
    let low_threshold =
        valid_yaml().replace("quarantine_threshold: 0.8", "quarantine_threshold: 0.99");
    let policy = QuarantinePolicy::from_yaml_str(&low_threshold).expect("解析高阈值策略");
    let pipeline = QuarantinePipeline::new(policy);
    let fragment = fragment("fixture-secret=");
    let assessment = pipeline.assess(&fragment, RiskContext::default());

    assert!(assessment.labels.contains(&RiskLabel::SensitiveData));
    assert!(matches!(
        pipeline.admission(&fragment, &assessment),
        Admission::Accept
    ));
}

#[test]
fn 内置策略可读取稳定版本和摘要() {
    let policy = QuarantinePolicy::builtin().expect("加载内置策略");
    assert!(!policy.version().is_empty());
    assert_eq!(policy.sha256().len(), 64);
}
