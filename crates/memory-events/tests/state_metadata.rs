//! 显式 State metadata 的权威写入前校验契约。

use chrono::Utc;
use jc_memory_events::validation::{
    FragmentValidationError, validate_fragment, validate_state_metadata,
};
use jc_memory_protocol::{
    CognitiveFragment, FragmentContent, FragmentId, FragmentKind, FragmentSource, MemoryScope,
    NamespaceId, Perspective, SecurityLabel, SourceType, StateDeclaration, ValidityStatus,
};

#[test]
fn parses_state_metadata_for_supported_fact_kinds() {
    for kind in [
        FragmentKind::Fact,
        FragmentKind::StateTransition,
        FragmentKind::Correction,
    ] {
        let fragment = state_fragment(
            kind,
            ValidityStatus::Unverified,
            serde_json::json!({
                "key": "project.deployment_strategy",
                "value": "blue_green",
                "reason": "保留可回滚的发布路径"
            }),
        );

        let declaration = validate_state_metadata(&fragment)
            .expect("合法 State metadata")
            .expect("应解析 State");
        assert_eq!(declaration.key, "project.deployment_strategy");
        assert_eq!(declaration.value, serde_json::json!("blue_green"));
        validate_fragment(&fragment).expect("合法 State Fragment");
    }
}

#[test]
fn rejects_invalid_state_shape() {
    let fragment = state_fragment(
        FragmentKind::Fact,
        ValidityStatus::Unverified,
        serde_json::json!(["不是对象"]),
    );

    assert_eq!(
        validate_state_metadata(&fragment),
        Err(FragmentValidationError::InvalidStateShape)
    );
}

#[test]
fn rejects_empty_oversized_and_non_ascii_state_keys() {
    for key in [
        String::new(),
        "a".repeat(257),
        "project/strategy".to_owned(),
    ] {
        let fragment = state_fragment(
            FragmentKind::Fact,
            ValidityStatus::Unverified,
            state_value(&key, serde_json::json!("canary"), "更新发布策略"),
        );
        assert_eq!(
            validate_state_metadata(&fragment),
            Err(FragmentValidationError::InvalidStateKey),
            "应拒绝 State key: {key:?}"
        );
    }
}

#[test]
fn rejects_empty_and_oversized_state_reasons() {
    for reason in ["   ".to_owned(), "理".repeat(2_049)] {
        let fragment = state_fragment(
            FragmentKind::Fact,
            ValidityStatus::Unverified,
            state_value(
                "project.deployment_strategy",
                serde_json::json!("canary"),
                &reason,
            ),
        );
        assert_eq!(
            validate_state_metadata(&fragment),
            Err(FragmentValidationError::InvalidStateReason)
        );
    }
}

#[test]
fn rejects_state_values_larger_than_one_mebibyte() {
    let fragment = state_fragment(
        FragmentKind::Fact,
        ValidityStatus::Unverified,
        state_value(
            "project.deployment_strategy",
            serde_json::Value::String("x".repeat(1_048_576)),
            "更新发布策略",
        ),
    );

    assert_eq!(
        validate_state_metadata(&fragment),
        Err(FragmentValidationError::StateValueTooLarge)
    );
}

#[test]
fn rejects_state_metadata_on_non_fact_kinds() {
    for kind in [
        FragmentKind::Prediction,
        FragmentKind::Inference,
        FragmentKind::Plan,
    ] {
        let fragment = state_fragment(
            kind,
            ValidityStatus::Unverified,
            state_value(
                "project.deployment_strategy",
                serde_json::json!("canary"),
                "尚未成为事实",
            ),
        );
        assert_eq!(
            validate_state_metadata(&fragment),
            Err(FragmentValidationError::StateKindNotAllowed),
            "应拒绝 Fragment Kind: {kind:?}"
        );
    }
}

#[test]
fn preserves_parseable_state_metadata_for_hidden_validity() {
    for validity in [ValidityStatus::Quarantined, ValidityStatus::Invalidated] {
        let fragment = state_fragment(
            FragmentKind::Fact,
            validity,
            state_value(
                "project.deployment_strategy",
                serde_json::json!("blue_green"),
                "保留权威原文",
            ),
        );

        assert!(
            validate_state_metadata(&fragment)
                .expect("隐藏状态仍应通过结构校验")
                .is_some()
        );
        validate_fragment(&fragment).expect("校验层不得删除隐藏状态 metadata");
    }
}

fn state_value(key: &str, value: serde_json::Value, reason: &str) -> serde_json::Value {
    serde_json::to_value(StateDeclaration {
        key: key.to_owned(),
        value,
        reason: reason.to_owned(),
    })
    .expect("序列化 StateDeclaration")
}

fn state_fragment(
    kind: FragmentKind,
    validity: ValidityStatus,
    state: serde_json::Value,
) -> CognitiveFragment {
    let now = Utc::now();
    let mut metadata = serde_json::Map::new();
    metadata.insert("state".to_owned(), state);
    CognitiveFragment {
        id: FragmentId::new(),
        scope: MemoryScope::new(NamespaceId::new()),
        session_id: None,
        trace_id: None,
        kind,
        content: FragmentContent::Text {
            text: "显式状态声明".to_owned(),
        },
        source: FragmentSource {
            source_type: SourceType::System,
            source_id: "state-metadata-test".to_owned(),
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
        validity,
        security_label: SecurityLabel::Internal,
        evidence: Vec::new(),
        metadata,
    }
}
