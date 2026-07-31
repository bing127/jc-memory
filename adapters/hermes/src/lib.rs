//! Hermes 到疆诚记忆 Core 的来源保真适配器。

pub mod config;
pub mod mapping;

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use jc_memory_core::{CommandBus, CommandError};
use jc_memory_protocol::{
    API_VERSION_V1, AccessContext, AccessScope, CommitCommand, FragmentId, MemoryCommand,
    MemoryPacket, MemoryRequest, MemoryResponse, RecallCommand, RecallMode, ResponseEnvelope,
};
use jc_memory_runtime::{ModelResponse, ToolResultRecord};
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, Notify};

pub use config::HermesConfig;
use mapping::map_turn;

/// Hermes 一次 turn 中捕获的原始观察。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct HermesObservation {
    /// 观察原文。
    pub text: String,
}

/// 传给适配器的一次 Hermes turn。
#[derive(Clone, Debug)]
pub struct HermesTurn {
    /// Hermes turn 稳定标识。
    pub turn_id: String,
    /// 该 turn 所属的精确记忆 Scope。
    pub scope: jc_memory_protocol::MemoryScope,
    /// Hermes 记录的发生时间。
    pub occurred_at: DateTime<Utc>,
    /// 原始观察集合。
    pub observations: Vec<HermesObservation>,
    /// 可选模型候选输出。
    pub model_output: Option<ModelResponse>,
    /// 已返回的工具结果集合。
    pub tool_results: Vec<ToolResultRecord>,
}

/// 一次同步操作的最小审计结果。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SyncReceipt {
    /// Hermes turn 标识。
    pub turn_id: String,
    /// 本次追加的 Fragment 数量。
    pub fragment_count: usize,
    /// Core 返回的权威 Fragment 标识。
    pub fragment_ids: Vec<FragmentId>,
}

/// Hermes 预取请求。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrefetchQuery {
    /// 用于 Recall 的原始文本。
    pub text: String,
    /// Recall 模式。
    pub mode: RecallMode,
}

/// 适配器公开的记忆 Provider 契约。
#[async_trait]
pub trait MemoryProvider: Send + Sync {
    /// 将一个 Hermes turn 追加到 Core。
    async fn sync_turn(&self, turn: HermesTurn) -> Result<SyncReceipt, HermesError>;

    /// 读取有界、模型无关的 Memory Packet。
    async fn prefetch(&self, query: PrefetchQuery) -> Result<MemoryPacket, HermesError>;

    /// 停止新请求、等待 in-flight 操作并刷新下游。
    async fn shutdown(&self) -> Result<(), HermesError>;
}

/// 共享 `CommandBus` 的 Hermes 记忆适配器。
#[derive(Clone)]
pub struct HermesMemoryProvider {
    inner: Arc<ProviderInner>,
}

struct ProviderInner {
    bus: Arc<dyn CommandBus>,
    access: AccessContext,
    config: HermesConfig,
    lifecycle: Lifecycle,
}

struct Lifecycle {
    state: Mutex<LifecycleState>,
    shutdown_lock: Mutex<()>,
    in_flight: AtomicUsize,
    notify: Notify,
}

#[derive(Default)]
struct LifecycleState {
    closing: bool,
    shutdown_complete: bool,
}

struct InFlight<'a> {
    lifecycle: &'a Lifecycle,
}

impl Drop for InFlight<'_> {
    fn drop(&mut self) {
        self.lifecycle.in_flight.fetch_sub(1, Ordering::AcqRel);
        self.lifecycle.notify.notify_waiters();
    }
}

/// 适配器错误。
#[derive(Debug, thiserror::Error)]
pub enum HermesError {
    /// 下游 `CommandBus` 失败。
    #[error("command bus failure: {0}")]
    Command(#[from] CommandError),
    /// Provider 已进入关闭流程。
    #[error("hermes memory provider is shutting down")]
    ShuttingDown,
    /// turn 没有可写入的原始 Fragment。
    #[error("hermes turn {0} produced no memory fragments")]
    EmptyTurn(String),
    /// 配置未通过适配器边界校验。
    #[error("invalid Hermes adapter configuration: {0}")]
    InvalidConfig(String),
    /// 下游返回了不匹配的响应类型。
    #[error("unexpected command response from memory core")]
    UnexpectedResponse,
}

impl HermesMemoryProvider {
    /// 创建使用既有 `CommandBus` 和授权上下文的 Provider。
    #[must_use]
    pub fn new(bus: Arc<dyn CommandBus>, access: AccessContext, config: HermesConfig) -> Self {
        Self {
            inner: Arc::new(ProviderInner {
                bus,
                access,
                config,
                lifecycle: Lifecycle {
                    state: Mutex::new(LifecycleState::default()),
                    shutdown_lock: Mutex::new(()),
                    in_flight: AtomicUsize::new(0),
                    notify: Notify::new(),
                },
            }),
        }
    }

    async fn begin(&self) -> Result<InFlight<'_>, HermesError> {
        let state = self.inner.lifecycle.state.lock().await;
        if state.closing {
            return Err(HermesError::ShuttingDown);
        }
        self.inner
            .lifecycle
            .in_flight
            .fetch_add(1, Ordering::AcqRel);
        Ok(InFlight {
            lifecycle: &self.inner.lifecycle,
        })
    }

    async fn request(&self, command: MemoryCommand) -> Result<ResponseEnvelope, HermesError> {
        let request_id = jc_memory_protocol::TraceId::new().to_string();
        let mut access = self.inner.access.clone();
        access.request_id.clone_from(&request_id);
        let request = MemoryRequest {
            api_version: API_VERSION_V1.to_owned(),
            request_id,
            access_scope: AccessScope::from(access.scope.clone()),
            command,
        };
        self.inner
            .bus
            .execute(&access, request)
            .await
            .map_err(HermesError::Command)
    }
}

#[async_trait]
impl MemoryProvider for HermesMemoryProvider {
    async fn sync_turn(&self, turn: HermesTurn) -> Result<SyncReceipt, HermesError> {
        let _in_flight = self.begin().await?;
        self.inner
            .config
            .validate()
            .map_err(HermesError::InvalidConfig)?;
        if turn.turn_id.trim().is_empty() {
            return Err(HermesError::EmptyTurn(turn.turn_id));
        }
        let fragments = map_turn(&turn, &self.inner.config);
        if fragments.is_empty() {
            return Err(HermesError::EmptyTurn(turn.turn_id));
        }
        let response = self
            .request(MemoryCommand::Commit(CommitCommand { fragments }))
            .await?;
        let MemoryResponse::Commit(result) = response.response else {
            return Err(HermesError::UnexpectedResponse);
        };
        Ok(SyncReceipt {
            turn_id: turn.turn_id,
            fragment_count: result.receipts.len(),
            fragment_ids: result
                .receipts
                .into_iter()
                .map(|receipt| receipt.fragment_id)
                .collect(),
        })
    }

    async fn prefetch(&self, query: PrefetchQuery) -> Result<MemoryPacket, HermesError> {
        let _in_flight = self.begin().await?;
        self.inner
            .config
            .validate()
            .map_err(HermesError::InvalidConfig)?;
        let response = self
            .request(MemoryCommand::Recall(RecallCommand {
                mode: query.mode,
                text: query.text,
                entities: Vec::new(),
                at_time: None,
                limits: self.inner.config.recall_limits,
            }))
            .await?;
        let MemoryResponse::Recall(packet) = response.response else {
            return Err(HermesError::UnexpectedResponse);
        };
        Ok(packet)
    }

    async fn shutdown(&self) -> Result<(), HermesError> {
        let _shutdown = self.inner.lifecycle.shutdown_lock.lock().await;
        {
            let mut state = self.inner.lifecycle.state.lock().await;
            if state.shutdown_complete {
                return Ok(());
            }
            state.closing = true;
        }
        while self.inner.lifecycle.in_flight.load(Ordering::Acquire) != 0 {
            let notified = self.inner.lifecycle.notify.notified();
            if self.inner.lifecycle.in_flight.load(Ordering::Acquire) == 0 {
                break;
            }
            notified.await;
        }
        self.inner
            .bus
            .shutdown()
            .await
            .map_err(HermesError::Command)?;
        let mut state = self.inner.lifecycle.state.lock().await;
        state.shutdown_complete = true;
        self.inner.lifecycle.notify.notify_waiters();
        Ok(())
    }
}
