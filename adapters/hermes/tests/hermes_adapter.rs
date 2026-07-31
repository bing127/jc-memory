//! Hermes 适配器的来源映射、配置和生命周期契约测试。

use std::{
    str::FromStr as _,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use async_trait::async_trait;
use chrono::{TimeZone, Utc};
use jc_memory_adapter_hermes::{
    HermesConfig, HermesMemoryProvider, HermesObservation, HermesTurn, MemoryProvider,
    PrefetchQuery,
};
use jc_memory_core::{CommandBus, CommandError, EmbeddedCommandBus, MemoryConfig, MemoryEngine};
use jc_memory_protocol::{
    AccessContext, CommitResult, MemoryCommand, MemoryPacket, MemoryRequest, MemoryResponse,
    RecallMode, ResponseEnvelope,
};
use jc_memory_runtime::{FinalResponse, ModelOutput, ModelResponse, ToolResultRecord};
use tokio::sync::{Mutex, Notify};

#[derive(Clone, Default)]
struct RecordingBus {
    requests: Arc<Mutex<Vec<MemoryRequest>>>,
    release_commit: Arc<Notify>,
    block_commit: Arc<AtomicBool>,
}

#[async_trait]
impl CommandBus for RecordingBus {
    async fn execute(
        &self,
        _access: &AccessContext,
        request: MemoryRequest,
    ) -> Result<ResponseEnvelope, CommandError> {
        let response = match &request.command {
            MemoryCommand::Commit(command) => {
                self.requests.lock().await.push(request.clone());
                if self.block_commit.load(Ordering::Acquire) {
                    self.release_commit.notified().await;
                }
                MemoryResponse::Commit(CommitResult {
                    receipts: command
                        .fragments
                        .iter()
                        .map(|fragment| jc_memory_protocol::ObserveResult {
                            fragment_id: fragment.id,
                            event_id: jc_memory_protocol::EventId::new(),
                            sequence: 1,
                            content_hash: jc_memory_protocol::BlobId::from_str(
                                "0000000000000000000000000000000000000000000000000000000000000000",
                            )
                            .expect("构造 fixture 内容哈希"),
                            created: true,
                            quarantined: false,
                        })
                        .collect(),
                })
            }
            MemoryCommand::Recall(_) => {
                self.requests.lock().await.push(request.clone());
                MemoryResponse::Recall(MemoryPacket::empty(
                    jc_memory_protocol::RecallId::new(),
                    RecallMode::Exact,
                ))
            }
            _ => unreachable!("fixture bus only accepts commit and recall"),
        };
        Ok(ResponseEnvelope::for_request(&request, response))
    }

    async fn shutdown(&self) -> Result<(), CommandError> {
        Ok(())
    }
}

fn access() -> AccessContext {
    let scope = jc_memory_protocol::MemoryScope::new(jc_memory_protocol::NamespaceId::new());
    let mut access = AccessContext::new("hermes-test", scope.clone());
    access.grants.extend(
        [
            jc_memory_protocol::Operation::Observe,
            jc_memory_protocol::Operation::Recall,
            jc_memory_protocol::Operation::Explain,
            jc_memory_protocol::Operation::Mutate,
        ]
        .into_iter()
        .map(|operation| jc_memory_protocol::Grant::for_scope(operation, &scope)),
    );
    access
}

fn config() -> HermesConfig {
    serde_json::from_str(
        r#"{
          "hermes_version": "0.19.0",
          "interface_version": "hermes.observer.v1",
          "recall_limits": {
            "max_seeds": 3,
            "max_neighbors": 4,
            "max_hops": 2,
            "max_nodes": 8,
            "timeout_ms": 120
          }
        }"#,
    )
    .expect("解析 Hermes 配置 fixture")
}

fn turn(access: &AccessContext) -> HermesTurn {
    HermesTurn {
        turn_id: "turn-42".to_owned(),
        scope: access.scope.clone(),
        occurred_at: Utc.with_ymd_and_hms(2026, 7, 31, 8, 9, 10).unwrap(),
        observations: vec![HermesObservation {
            text: "用户要求保留来源".to_owned(),
        }],
        model_output: Some(ModelResponse {
            output: ModelOutput::FinalResponse(FinalResponse {
                text: "已记录候选响应".to_owned(),
            }),
            raw_text: "已记录候选响应".to_owned(),
        }),
        tool_results: vec![ToolResultRecord {
            success: true,
            value: serde_json::json!({"status": "ok"}),
        }],
    }
}

#[tokio::test]
async fn embedded_command_bus_persists_mapped_turn_without_promoting_candidates() {
    let data_dir = tempfile::tempdir().expect("创建引擎目录");
    let engine = MemoryEngine::open(MemoryConfig::local(data_dir.path()))
        .await
        .expect("打开引擎");
    let access = access();
    let provider = HermesMemoryProvider::new(
        Arc::new(EmbeddedCommandBus::new(engine.clone())),
        access.clone(),
        config(),
    );

    let receipt = provider
        .sync_turn(turn(&access))
        .await
        .expect("写入 Hermes turn");
    assert_eq!(receipt.fragment_count, 3);
    let stats = engine
        .stats(&access, &access.scope)
        .await
        .expect("读取持久化统计");
    assert_eq!(stats.fragment_count, 3);
    let model_fragment = engine
        .fragment(&access, receipt.fragment_ids[1])
        .await
        .expect("读取模型候选");
    assert_eq!(
        model_fragment.validity,
        jc_memory_protocol::ValidityStatus::Unverified
    );
    assert_eq!(
        model_fragment.source.source_id,
        "hermes/0.19.0/hermes.observer.v1/turn/turn-42/model_output/0"
    );
}

#[tokio::test]
async fn sync_turn_preserves_hermes_provenance_and_candidate_validity() {
    let bus = RecordingBus::default();
    let provider = HermesMemoryProvider::new(Arc::new(bus.clone()), access(), config());
    let receipt = provider
        .sync_turn(turn(&access()))
        .await
        .expect("同步 turn");
    assert_eq!(receipt.turn_id, "turn-42");
    assert_eq!(receipt.fragment_count, 3);

    let requests = bus.requests.lock().await;
    let MemoryCommand::Commit(commit) = &requests[0].command else {
        panic!("期望 Commit 命令");
    };
    assert_eq!(commit.fragments.len(), 3);
    assert!(commit.fragments.iter().all(|fragment| {
        fragment.source.source_id.contains("turn-42")
            && fragment.source.source_id.contains("hermes.observer.v1")
            && fragment.validity == jc_memory_protocol::ValidityStatus::Unverified
    }));
    assert_eq!(
        commit.fragments[1].kind,
        jc_memory_protocol::FragmentKind::Claim
    );
}

#[tokio::test]
async fn prefetch_uses_configured_limits_and_returns_packet_unchanged() {
    let bus = RecordingBus::default();
    let provider = HermesMemoryProvider::new(Arc::new(bus.clone()), access(), config());
    let packet = provider
        .prefetch(PrefetchQuery {
            text: "来源".to_owned(),
            mode: RecallMode::Exact,
        })
        .await
        .expect("预取记忆");
    assert_eq!(packet.intent, RecallMode::Exact);

    let requests = bus.requests.lock().await;
    let MemoryCommand::Recall(recall) = &requests[0].command else {
        panic!("期望 Recall 命令");
    };
    assert_eq!(recall.limits.max_nodes, 8);
    assert_eq!(recall.text, "来源");
}

#[tokio::test]
async fn shutdown_waits_for_in_flight_sync_and_rejects_new_turns() {
    let bus = RecordingBus::default();
    bus.block_commit.store(true, Ordering::Release);
    let provider = HermesMemoryProvider::new(Arc::new(bus.clone()), access(), config());
    let sync = {
        let provider = provider.clone();
        tokio::spawn(async move { provider.sync_turn(turn(&access())).await })
    };
    for _ in 0..20 {
        if !bus.requests.lock().await.is_empty() {
            break;
        }
        tokio::task::yield_now().await;
    }
    let shutdown = {
        let provider = provider.clone();
        tokio::spawn(async move { provider.shutdown().await })
    };
    tokio::task::yield_now().await;
    assert!(
        !shutdown.is_finished(),
        "in-flight turn 未完成时不得结束 shutdown"
    );
    let error = provider
        .prefetch(PrefetchQuery {
            text: "关闭后".to_owned(),
            mode: RecallMode::Exact,
        })
        .await
        .expect_err("关闭中应拒绝新请求");
    assert!(error.to_string().contains("shutting down"));
    bus.release_commit.notify_waiters();
    sync.await.expect("等待同步任务").expect("同步成功");
    shutdown
        .await
        .expect("等待 shutdown")
        .expect("shutdown 成功");
}

#[test]
fn config_fixture_declares_hermes_interface_version() {
    let yaml = include_str!("fixtures/hermes-adapter.yaml");
    let config = HermesConfig::from_yaml(yaml).expect("解析 YAML 配置");
    assert_eq!(config.hermes_version, "0.19.0");
    assert_eq!(config.interface_version, "hermes.observer.v1");
}

#[test]
fn turn_fixture_only_claims_the_declared_hermes_contract() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/hermes-0.19.0-turn.json"))
            .expect("解析 Hermes turn fixture");
    assert_eq!(fixture["contract"]["hermes_version"], "0.19.0");
    assert_eq!(
        fixture["contract"]["observer_schema_version"],
        "hermes.observer.v1"
    );
    assert_eq!(fixture["contract"]["verification"], "fixture_only");
}
