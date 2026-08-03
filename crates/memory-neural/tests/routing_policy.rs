//! 离线路由策略的严格解析和自定义关键词行为测试。

use jc_memory_activation::{RecallLimits, RecallQuery};
use jc_memory_neural::{DeterministicFallback, FallbackRoutingPolicy, QueryRouter};
use jc_memory_protocol::{MemoryScope, NamespaceId, RecallMode};

fn valid_yaml() -> &'static str {
    r#"
schema_version: 1
policy_version: fixture-routing-1
causal_keywords: [fixture-cause]
current_state_keywords: [fixture-current]
procedural_keywords: [fixture-how]
"#
}

fn query(text: &str) -> RecallQuery {
    RecallQuery {
        scope: MemoryScope::new(NamespaceId::new()),
        mode: RecallMode::Semantic,
        text: text.to_owned(),
        entities: Vec::new(),
        at_time: None,
        limits: RecallLimits {
            max_seeds: 1,
            max_neighbors: 1,
            max_hops: 1,
            max_nodes: 1,
            timeout_ms: 100,
        },
    }
}

#[test]
fn 路由策略严格拒绝未知字段空词项重复词项和非法版本() {
    let policy = FallbackRoutingPolicy::from_yaml_str(valid_yaml()).expect("解析路由策略");
    assert_eq!(policy.version(), "fixture-routing-1");
    assert_eq!(policy.sha256().len(), 64);
    assert!(
        FallbackRoutingPolicy::from_yaml_str(&format!("{}\nunknown: true", valid_yaml())).is_err()
    );
    assert!(
        FallbackRoutingPolicy::from_yaml_str(&valid_yaml().replace("fixture-cause", " ")).is_err()
    );
    assert!(
        FallbackRoutingPolicy::from_yaml_str(
            &valid_yaml().replace("fixture-cause", "fixture-cause, fixture-cause")
        )
        .is_err()
    );
    assert!(
        FallbackRoutingPolicy::from_yaml_str(
            &valid_yaml().replace("schema_version: 1", "schema_version: 2")
        )
        .is_err()
    );
}

#[tokio::test]
async fn 自定义关键词真实改变离线路由() {
    let policy = FallbackRoutingPolicy::from_yaml_str(valid_yaml()).expect("解析路由策略");
    let fallback = DeterministicFallback::with_routing_policy(policy);

    assert_eq!(
        fallback.route(&query("fixture-cause")).await.unwrap().mode,
        RecallMode::Causal
    );
    assert_eq!(
        fallback
            .route(&query("fixture-current"))
            .await
            .unwrap()
            .mode,
        RecallMode::CurrentState
    );
    assert_eq!(
        fallback.route(&query("fixture-how")).await.unwrap().mode,
        RecallMode::Procedural
    );
}

#[test]
fn 内置路由策略可读取版本和摘要() {
    let policy = FallbackRoutingPolicy::builtin().expect("加载内置路由策略");
    assert!(!policy.version().is_empty());
    assert_eq!(policy.sha256().len(), 64);
}
