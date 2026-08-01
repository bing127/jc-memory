//! Core 可信 State Recall 与普通文本兼容性验收测试。

mod common;

use chrono::{DateTime, Duration, Utc};
use jc_memory_core::{MemoryConfig, MemoryEngine, RecallLimits, RecallQuery};
use jc_memory_protocol::{
    AccessContext, EventId, EvidenceId, EvidenceRef, FragmentContent, FragmentId, MemoryPacket,
    MemoryScope, NamespaceId, RecallMode, ValidityStatus, VerificationMethod,
};

#[tokio::test]
async fn current_historical_and_semantic_recall_return_explicit_state_evidence() {
    let data_dir = tempfile::tempdir().expect("创建可信 State 数据目录");
    let engine = MemoryEngine::open(MemoryConfig::local(data_dir.path()))
        .await
        .expect("打开记忆引擎");
    let scope = MemoryScope::new(NamespaceId::new());
    let context = common::access_context(&scope);
    let fixture = observe_state_history(&engine, &context, &scope).await;

    let current = engine
        .recall(
            &context,
            query(
                scope.clone(),
                RecallMode::CurrentState,
                "project.deployment_strategy",
                None,
                8,
            ),
        )
        .await
        .expect("召回当前 State");
    assert_current_state(&current, &fixture);

    let historical = engine
        .recall(
            &context,
            query(
                scope.clone(),
                RecallMode::HistoricalState,
                "project.deployment_strategy",
                Some(fixture.canary_time),
                8,
            ),
        )
        .await
        .expect("按切换时刻召回 State");
    assert_eq!(
        historical.current_states[0].value,
        serde_json::json!("canary")
    );
    assert_eq!(
        historical.historical_states[0].value,
        serde_json::json!("blue_green")
    );

    let before_switch = engine
        .recall(
            &context,
            query(
                scope.clone(),
                RecallMode::HistoricalState,
                "project.deployment_strategy",
                Some(fixture.canary_time - Duration::microseconds(1)),
                8,
            ),
        )
        .await
        .expect("召回切换前 State");
    assert_eq!(
        before_switch.current_states[0].value,
        serde_json::json!("blue_green")
    );
    assert!(before_switch.historical_states.is_empty());

    let semantic = engine
        .recall(
            &context,
            query(scope, RecallMode::Semantic, "canary", None, 8),
        )
        .await
        .expect("普通语义 Recall 附带相关 State");
    assert_eq!(semantic.current_states.len(), 1);
    assert_eq!(
        semantic.current_states[0].value,
        serde_json::json!("canary")
    );
    assert!(semantic.historical_states.is_empty());
}

struct StateHistoryFixture {
    canary_time: DateTime<Utc>,
    blue_event_id: EventId,
    canary_event_id: EventId,
    canary_fragment_id: FragmentId,
    evidence_id: EvidenceId,
}

async fn observe_state_history(
    engine: &MemoryEngine,
    context: &AccessContext,
    scope: &MemoryScope,
) -> StateHistoryFixture {
    let now = Utc::now();
    let blue_time = now - Duration::seconds(20);
    let canary_time = now - Duration::seconds(10);
    let mut blue = common::state_fragment(
        scope.clone(),
        "project.deployment_strategy",
        serde_json::json!("blue_green"),
        "生产环境先使用蓝绿发布",
        blue_time,
    );
    blue.content = FragmentContent::Text {
        text: "部署策略 blue_green".to_owned(),
    };
    let blue_id = blue.id;
    let blue_receipt = engine
        .observe(context, blue)
        .await
        .expect("写入 blue_green State");
    let evidence_id = EvidenceId::new();
    let mut canary = common::state_fragment(
        scope.clone(),
        "project.deployment_strategy",
        serde_json::json!("canary"),
        "验证通过后切换为金丝雀发布",
        canary_time,
    );
    canary.content = FragmentContent::Text {
        text: "部署策略 canary".to_owned(),
    };
    canary.validity = ValidityStatus::Verified;
    canary.evidence.push(EvidenceRef {
        id: evidence_id,
        fragment_id: Some(blue_id),
        event_id: None,
        method: VerificationMethod::Manual,
        note: Some("人工确认部署策略变更".to_owned()),
    });
    let canary_id = canary.id;
    let canary_receipt = engine
        .observe(context, canary)
        .await
        .expect("写入 canary State");

    StateHistoryFixture {
        canary_time,
        blue_event_id: blue_receipt.event_id,
        canary_event_id: canary_receipt.event_id,
        canary_fragment_id: canary_id,
        evidence_id,
    }
}

fn assert_current_state(current: &MemoryPacket, fixture: &StateHistoryFixture) {
    assert_eq!(current.current_states.len(), 1);
    assert_eq!(current.current_states[0].value, serde_json::json!("canary"));
    assert_eq!(
        current.current_states[0].reason.as_deref(),
        Some("验证通过后切换为金丝雀发布")
    );
    assert_eq!(
        current.current_states[0].source_event_id,
        Some(fixture.canary_event_id)
    );
    assert_eq!(
        current.current_states[0].evidence[0].evidence_ref_id,
        Some(fixture.evidence_id)
    );
    assert_eq!(current.historical_states.len(), 1);
    assert_eq!(
        current.historical_states[0].value,
        serde_json::json!("blue_green")
    );
    assert_eq!(
        current.historical_states[0].valid_to,
        Some(fixture.canary_time)
    );
    assert_eq!(
        current.historical_states[0].source_event_id,
        Some(fixture.blue_event_id)
    );
    assert_eq!(current.coverage.evidence_count, current.evidence.len());
    assert!(current.evidence.iter().any(|item| {
        item.fragment_id == fixture.canary_fragment_id
            && item.evidence_ref_id == Some(fixture.evidence_id)
    }));
}

#[tokio::test]
async fn simultaneous_state_values_remain_visible_as_conflict() {
    let data_dir = tempfile::tempdir().expect("创建 State 冲突数据目录");
    let engine = MemoryEngine::open(MemoryConfig::local(data_dir.path()))
        .await
        .expect("打开记忆引擎");
    let scope = MemoryScope::new(NamespaceId::new());
    let context = common::access_context(&scope);
    let occurred_at = Utc::now() - Duration::seconds(5);
    for value in ["blue_green", "canary"] {
        engine
            .observe(
                &context,
                common::state_fragment(
                    scope.clone(),
                    "project.deployment_strategy",
                    serde_json::json!(value),
                    "同刻竞争的显式状态",
                    occurred_at,
                ),
            )
            .await
            .expect("写入同刻 State");
    }

    let packet = engine
        .recall(
            &context,
            query(
                scope,
                RecallMode::CurrentState,
                "project.deployment_strategy",
                None,
                8,
            ),
        )
        .await
        .expect("召回同刻 State 冲突");
    assert_eq!(packet.current_states.len(), 2);
    assert_eq!(packet.conflicts.len(), 1);
    assert_eq!(packet.conflicts[0].point_ids.len(), 2);
    assert_eq!(packet.conflicts[0].evidence.len(), 2);
}

#[tokio::test]
async fn ordinary_postgresql_mysql_text_remains_exact_recall_content_on_sqlite() {
    let data_dir = tempfile::tempdir().expect("创建普通文本兼容数据目录");
    let engine = MemoryEngine::open(MemoryConfig::local(data_dir.path()))
        .await
        .expect("打开记忆引擎");
    let mut fragment = common::observation("项目计划从 PostgreSQL 迁移到 MySQL");
    fragment.kind = jc_memory_protocol::FragmentKind::Fact;
    let scope = fragment.scope.clone();
    let context = common::access_context(&scope);
    let fragment_id = fragment.id;
    engine
        .observe(&context, fragment)
        .await
        .expect("写入普通数据库迁移正文");

    let packet = engine
        .recall(
            &context,
            query(
                scope,
                RecallMode::Exact,
                "项目计划从 PostgreSQL 迁移到 MySQL",
                None,
                4,
            ),
        )
        .await
        .expect("精确召回普通正文");
    assert_eq!(packet.points[0].source_fragments, vec![fragment_id]);
    assert_eq!(
        packet.episodes[0].summary,
        "项目计划从 PostgreSQL 迁移到 MySQL"
    );
    assert!(packet.current_states.is_empty());

    engine.close().await;
    let database_files = std::fs::read_dir(data_dir.path())
        .expect("读取数据目录")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| {
            name.contains("sqlite") || name.contains("postgres") || name.contains("mysql")
        })
        .collect::<Vec<_>>();
    assert_eq!(database_files, vec!["memory.sqlite3"]);
}

fn query(
    scope: MemoryScope,
    mode: RecallMode,
    text: &str,
    at_time: Option<DateTime<Utc>>,
    max_nodes: usize,
) -> RecallQuery {
    RecallQuery {
        scope,
        mode,
        text: text.to_owned(),
        entities: Vec::new(),
        at_time,
        limits: RecallLimits {
            max_seeds: max_nodes,
            max_neighbors: max_nodes,
            max_hops: 2,
            max_nodes,
            timeout_ms: 500,
        },
    }
}
