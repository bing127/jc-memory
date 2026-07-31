//! 疆诚记忆 HTTP 与共享协议服务边界。

mod config;

pub use config::{ConfigError, ServerConfig, TlsFiles};

use std::{
    collections::BTreeMap,
    future::Future,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode, header::AUTHORIZATION},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use jc_memory_core::{CommandBus, CommandError};
use jc_memory_protocol::{
    AccessContext, Grant, MemoryRequest, MemoryScope, Operation, ResponseEnvelope,
    generated::jiangcheng::memory::v1::{
        RequestEnvelope as ProtoRequestEnvelope, ResponseEnvelope as ProtoResponseEnvelope,
        memory_service_server::{MemoryService, MemoryServiceServer},
    },
};
use serde::Serialize;
use serde_json::json;
use tokio::net::TcpListener;

/// 服务端保存的 API Key 与授权上下文模板。
#[derive(Clone)]
pub struct ApiKey {
    value: String,
    context: AccessContext,
}

impl ApiKey {
    /// 创建一个 API Key；Grant 只存在服务端上下文中。
    #[must_use]
    pub fn new(value: impl Into<String>, context: AccessContext) -> Self {
        Self {
            value: value.into(),
            context,
        }
    }

    /// 为精确 Scope 和显式操作集合创建服务端 API Key。
    #[must_use]
    pub fn for_scope(
        value: impl Into<String>,
        principal_id: impl Into<String>,
        scope: MemoryScope,
        operations: impl IntoIterator<Item = Operation>,
    ) -> Self {
        let grants = operations
            .into_iter()
            .map(|operation| Grant::for_scope(operation, &scope))
            .collect();
        let mut context = AccessContext::new(principal_id, scope);
        context.grants = grants;
        Self::new(value, context)
    }
}

/// HTTP 与 gRPC 适配器共享的服务状态。
pub struct ServerState<B> {
    bus: Arc<B>,
    keys: Arc<BTreeMap<String, AccessContext>>,
    in_flight: Arc<AtomicUsize>,
    max_in_flight: usize,
    rate_limiter: Option<Arc<ApiKeyRateLimiter>>,
    request_timeout: Option<Duration>,
}

impl<B> Clone for ServerState<B> {
    fn clone(&self) -> Self {
        Self {
            bus: Arc::clone(&self.bus),
            keys: Arc::clone(&self.keys),
            in_flight: Arc::clone(&self.in_flight),
            max_in_flight: self.max_in_flight,
            rate_limiter: self.rate_limiter.clone(),
            request_timeout: self.request_timeout,
        }
    }
}

impl<B> ServerState<B> {
    /// 创建服务状态。
    #[must_use]
    pub fn new(bus: B, keys: impl IntoIterator<Item = ApiKey>) -> Self {
        Self::with_max_in_flight(bus, keys, 64)
    }

    /// 创建带有界并发限制的服务状态。
    #[must_use]
    pub fn with_max_in_flight(
        bus: B,
        keys: impl IntoIterator<Item = ApiKey>,
        max_in_flight: usize,
    ) -> Self {
        Self::build(bus, keys, max_in_flight, None, None)
    }

    /// 创建带有界并发和每 API Key 每秒速率限制的服务状态。
    #[must_use]
    pub fn with_request_limits(
        bus: B,
        keys: impl IntoIterator<Item = ApiKey>,
        max_in_flight: usize,
        max_requests_per_second: u64,
    ) -> Self {
        Self::build(
            bus,
            keys,
            max_in_flight,
            Some(max_requests_per_second),
            None,
        )
    }

    /// 创建带并发、速率和请求超时限制的服务状态。
    #[must_use]
    pub fn with_request_limits_and_timeout(
        bus: B,
        keys: impl IntoIterator<Item = ApiKey>,
        max_in_flight: usize,
        max_requests_per_second: u64,
        request_timeout_ms: u64,
    ) -> Self {
        Self::build(
            bus,
            keys,
            max_in_flight,
            Some(max_requests_per_second),
            Some(Duration::from_millis(request_timeout_ms)),
        )
    }

    fn build(
        bus: B,
        keys: impl IntoIterator<Item = ApiKey>,
        max_in_flight: usize,
        max_requests_per_second: Option<u64>,
        request_timeout: Option<Duration>,
    ) -> Self {
        let keys = keys
            .into_iter()
            .map(|key| (key.value, key.context))
            .collect();
        Self {
            bus: Arc::new(bus),
            keys: Arc::new(keys),
            in_flight: Arc::new(AtomicUsize::new(0)),
            max_in_flight,
            rate_limiter: max_requests_per_second
                .map(|limit| Arc::new(ApiKeyRateLimiter::new(limit))),
            request_timeout,
        }
    }

    fn access(&self, key: &str, request_id: &str) -> Result<AccessContext, ServerError> {
        let mut context = self
            .keys
            .get(key)
            .cloned()
            .ok_or(ServerError::Unauthorized)?;
        request_id.clone_into(&mut context.request_id);
        Ok(context)
    }

    fn acquire_rate_limit(&self, key: &str) -> Result<(), ServerError> {
        if let Some(limiter) = &self.rate_limiter {
            limiter.acquire(key)?;
        }
        Ok(())
    }
}

struct ApiKeyRateLimiter {
    max_requests: u64,
    windows: Mutex<BTreeMap<String, RateWindow>>,
}

impl ApiKeyRateLimiter {
    fn new(max_requests: u64) -> Self {
        Self {
            max_requests,
            windows: Mutex::new(BTreeMap::new()),
        }
    }

    fn acquire(&self, key: &str) -> Result<(), ServerError> {
        let now = Instant::now();
        let mut windows = self
            .windows
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let window = windows.entry(key.to_owned()).or_insert(RateWindow {
            started_at: now,
            accepted: 0,
        });
        if now.duration_since(window.started_at) >= Duration::from_secs(1) {
            window.started_at = now;
            window.accepted = 0;
        }
        if window.accepted >= self.max_requests {
            return Err(ServerError::RateLimited);
        }
        window.accepted += 1;
        Ok(())
    }
}

struct RateWindow {
    started_at: Instant,
    accepted: u64,
}

/// 构造 HTTP 路由。
pub fn router<B>(state: ServerState<B>) -> Router
where
    B: CommandBus + Send + Sync + 'static,
{
    Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .route("/metrics", get(metrics))
        .route("/v1/memory/command", post(command::<B>))
        .route("/v1/memory/observe", post(observe::<B>))
        .route("/v1/memory/recall", post(recall::<B>))
        .route("/v1/memory/stats", post(stats::<B>))
        .route("/v1/memory/correct", post(correct::<B>))
        .route("/v1/memory/forget", post(forget::<B>))
        .route("/v1/memory/explain", post(explain::<B>))
        .with_state(state)
}

/// 在 TCP Listener 上运行 HTTP 服务，并等待外部优雅关闭信号。
///
/// # Errors
///
/// Listener 接受循环失败时返回传输错误；shutdown Future 完成后正常返回。
pub async fn serve<B, F>(
    listener: TcpListener,
    state: ServerState<B>,
    shutdown: F,
) -> Result<(), ServerError>
where
    B: CommandBus + Send + Sync + 'static,
    F: Future<Output = ()> + Send + 'static,
{
    axum::serve(listener, router(state))
        .with_graceful_shutdown(shutdown)
        .await
        .map_err(|error| ServerError::Transport(error.to_string()))
}

/// 共享 `CommandBus` 的真实 Tonic gRPC 服务实现。
#[derive(Clone)]
pub struct TonicMemoryService<B> {
    adapter: GrpcAdapter<B>,
}

impl<B> TonicMemoryService<B>
where
    B: CommandBus + Send + Sync + 'static,
{
    /// 从服务状态创建 Tonic 服务。
    #[must_use]
    pub fn new(state: ServerState<B>) -> Self {
        Self {
            adapter: GrpcAdapter::new(state),
        }
    }
}

#[tonic::async_trait]
impl<B> MemoryService for TonicMemoryService<B>
where
    B: CommandBus + Send + Sync + 'static,
{
    async fn execute(
        &self,
        request: tonic::Request<ProtoRequestEnvelope>,
    ) -> Result<tonic::Response<ProtoResponseEnvelope>, tonic::Status> {
        let key = extract_tonic_api_key(request.metadata())
            .ok_or_else(|| tonic::Status::unauthenticated("unauthorized"))?;
        let domain_request = jc_memory_protocol::MemoryRequest::try_from(request.into_inner())
            .map_err(|error| tonic::Status::invalid_argument(error.to_string()))?;
        let response = self
            .adapter
            .execute(&key, domain_request)
            .await
            .map_err(|error| grpc_status(&error))?;
        let wire_response = ProtoResponseEnvelope::try_from(response)
            .map_err(|error| tonic::Status::internal(error.to_string()))?;
        Ok(tonic::Response::new(wire_response))
    }
}

/// 在 TCP Listener 上运行真实 Tonic gRPC 服务。
///
/// # Errors
///
/// Listener 接受循环失败或优雅关闭前出现传输错误时返回错误。
pub async fn serve_grpc<B, F>(
    listener: TcpListener,
    state: ServerState<B>,
    shutdown: F,
) -> Result<(), ServerError>
where
    B: CommandBus + Send + Sync + 'static,
    F: Future<Output = ()> + Send + 'static,
{
    serve_grpc_with_tls(listener, state, shutdown, None).await
}

/// 在 TCP Listener 上运行可选 TLS 的 Tonic gRPC 服务。
///
/// # Errors
///
/// TLS 配置无效、Listener 接受循环失败或优雅关闭前出现传输错误时返回错误。
pub async fn serve_grpc_with_tls<B, F>(
    listener: TcpListener,
    state: ServerState<B>,
    shutdown: F,
    tls: Option<tonic::transport::ServerTlsConfig>,
) -> Result<(), ServerError>
where
    B: CommandBus + Send + Sync + 'static,
    F: Future<Output = ()> + Send + 'static,
{
    let mut server = tonic::transport::Server::builder();
    if let Some(tls) = tls {
        server = server
            .tls_config(tls)
            .map_err(|error| ServerError::Transport(error.to_string()))?;
    }
    server
        .add_service(MemoryServiceServer::new(TonicMemoryService::new(state)))
        .serve_with_incoming_shutdown(
            tokio_stream::wrappers::TcpListenerStream::new(listener),
            shutdown,
        )
        .await
        .map_err(|error| ServerError::Transport(error.to_string()))
}

async fn healthz() -> impl IntoResponse {
    Json(json!({"status":"ok"}))
}

async fn readyz() -> impl IntoResponse {
    Json(json!({"status":"ready"}))
}

async fn metrics() -> impl IntoResponse {
    (
        [("content-type", "text/plain; version=0.0.4")],
        "jc_memory_server_ready 1\n",
    )
}

async fn observe<B>(
    State(state): State<ServerState<B>>,
    headers: HeaderMap,
    Json(request): Json<MemoryRequest>,
) -> Result<Json<ResponseEnvelope>, ServerError>
where
    B: CommandBus + Send + Sync + 'static,
{
    command_inner(state, headers, request).await
}

async fn command<B>(
    State(state): State<ServerState<B>>,
    headers: HeaderMap,
    Json(request): Json<MemoryRequest>,
) -> Result<Json<ResponseEnvelope>, ServerError>
where
    B: CommandBus + Send + Sync + 'static,
{
    command_inner(state, headers, request).await
}

async fn recall<B>(
    State(state): State<ServerState<B>>,
    headers: HeaderMap,
    Json(request): Json<MemoryRequest>,
) -> Result<Json<ResponseEnvelope>, ServerError>
where
    B: CommandBus + Send + Sync + 'static,
{
    command_inner(state, headers, request).await
}

async fn stats<B>(
    State(state): State<ServerState<B>>,
    headers: HeaderMap,
    Json(request): Json<MemoryRequest>,
) -> Result<Json<ResponseEnvelope>, ServerError>
where
    B: CommandBus + Send + Sync + 'static,
{
    command_inner(state, headers, request).await
}

async fn correct<B>(
    State(state): State<ServerState<B>>,
    headers: HeaderMap,
    Json(request): Json<MemoryRequest>,
) -> Result<Json<ResponseEnvelope>, ServerError>
where
    B: CommandBus + Send + Sync + 'static,
{
    command_inner(state, headers, request).await
}

async fn forget<B>(
    State(state): State<ServerState<B>>,
    headers: HeaderMap,
    Json(request): Json<MemoryRequest>,
) -> Result<Json<ResponseEnvelope>, ServerError>
where
    B: CommandBus + Send + Sync + 'static,
{
    command_inner(state, headers, request).await
}

async fn explain<B>(
    State(state): State<ServerState<B>>,
    headers: HeaderMap,
    Json(request): Json<MemoryRequest>,
) -> Result<Json<ResponseEnvelope>, ServerError>
where
    B: CommandBus + Send + Sync + 'static,
{
    command_inner(state, headers, request).await
}

async fn command_inner<B>(
    state: ServerState<B>,
    headers: HeaderMap,
    request: MemoryRequest,
) -> Result<Json<ResponseEnvelope>, ServerError>
where
    B: CommandBus + Send + Sync + 'static,
{
    let key = extract_api_key(&headers).ok_or(ServerError::Unauthorized)?;
    let access = state.access(&key, &request.request_id)?;
    state.acquire_rate_limit(&key)?;
    let _permit = InFlightPermit::acquire(&state)?;
    let response = execute_with_timeout(&state, &access, request).await?;
    Ok(Json(response))
}

struct InFlightPermit {
    counter: Arc<AtomicUsize>,
}

impl InFlightPermit {
    fn acquire<B>(state: &ServerState<B>) -> Result<Self, ServerError> {
        let mut current = state.in_flight.load(Ordering::Acquire);
        loop {
            if current >= state.max_in_flight {
                return Err(ServerError::RateLimited);
            }
            match state.in_flight.compare_exchange_weak(
                current,
                current + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    return Ok(Self {
                        counter: Arc::clone(&state.in_flight),
                    });
                }
                Err(observed) => current = observed,
            }
        }
    }
}

impl Drop for InFlightPermit {
    fn drop(&mut self) {
        self.counter.fetch_sub(1, Ordering::AcqRel);
    }
}

fn extract_api_key(headers: &HeaderMap) -> Option<String> {
    if let Some(value) = headers
        .get("x-api-key")
        .and_then(|value| value.to_str().ok())
    {
        return Some(value.to_owned());
    }
    let value = headers.get(AUTHORIZATION)?.to_str().ok()?;
    value.strip_prefix("Bearer ").map(str::to_owned)
}

fn extract_tonic_api_key(metadata: &tonic::metadata::MetadataMap) -> Option<String> {
    if let Some(value) = metadata
        .get("x-api-key")
        .and_then(|value| value.to_str().ok())
    {
        return Some(value.to_owned());
    }
    let value = metadata.get("authorization")?.to_str().ok()?;
    value.strip_prefix("Bearer ").map(str::to_owned)
}

/// gRPC/SDK 适配器的共享语义入口。
#[derive(Clone)]
pub struct GrpcAdapter<B> {
    state: ServerState<B>,
}

impl<B> GrpcAdapter<B>
where
    B: CommandBus + Send + Sync + 'static,
{
    /// 创建适配器。
    #[must_use]
    pub fn new(state: ServerState<B>) -> Self {
        Self { state }
    }

    /// 使用 API Key 执行同一个 Core `CommandBus` 请求。
    ///
    /// # Errors
    ///
    /// API Key 无效或 Core 命令失败时返回错误。
    pub async fn execute(
        &self,
        key: &str,
        request: MemoryRequest,
    ) -> Result<ResponseEnvelope, ServerError> {
        let access = self.state.access(key, &request.request_id)?;
        self.state.acquire_rate_limit(key)?;
        let _permit = InFlightPermit::acquire(&self.state)?;
        execute_with_timeout(&self.state, &access, request).await
    }
}

async fn execute_with_timeout<B>(
    state: &ServerState<B>,
    access: &AccessContext,
    request: MemoryRequest,
) -> Result<ResponseEnvelope, ServerError>
where
    B: CommandBus + Send + Sync + 'static,
{
    let execute = state.bus.execute(access, request);
    let result = match state.request_timeout {
        Some(timeout) => tokio::time::timeout(timeout, execute)
            .await
            .map_err(|_| ServerError::timeout(&access.request_id))?,
        None => execute.await,
    };
    result.map_err(|error| ServerError::command(&access.request_id, error))
}

/// 服务边界错误。
#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    /// API Key 缺失或无效。
    #[error("unauthorized")]
    Unauthorized,
    /// 当前请求资源已耗尽。
    #[error("request limit exceeded")]
    RateLimited,
    /// 请求执行超过 deadline。
    #[error("request deadline exceeded for request {request_id}")]
    Timeout {
        /// 请求标识。
        request_id: String,
    },
    /// HTTP Listener 或连接循环失败。
    #[error("server transport failure: {0}")]
    Transport(String),
    /// Core 命令错误。
    #[error("command failed for request {request_id}: {source}")]
    Command {
        /// 请求标识。
        request_id: String,
        /// Core 原始错误。
        #[source]
        source: CommandError,
    },
}

impl ServerError {
    fn command(request_id: &str, source: CommandError) -> Self {
        Self::Command {
            request_id: request_id.to_owned(),
            source,
        }
    }

    fn timeout(request_id: &str) -> Self {
        Self::Timeout {
            request_id: request_id.to_owned(),
        }
    }
}

fn grpc_status(error: &ServerError) -> tonic::Status {
    let (code, request_id) = match &error {
        ServerError::Unauthorized => (tonic::Code::Unauthenticated, None),
        ServerError::RateLimited => (tonic::Code::ResourceExhausted, None),
        ServerError::Timeout { request_id } => {
            (tonic::Code::DeadlineExceeded, Some(request_id.as_str()))
        }
        ServerError::Transport(_) => (tonic::Code::Internal, None),
        ServerError::Command { source, request_id } => {
            let code = match source.code() {
                jc_memory_protocol::CommandErrorCode::InvalidArgument => {
                    tonic::Code::InvalidArgument
                }
                jc_memory_protocol::CommandErrorCode::PermissionDenied => {
                    tonic::Code::PermissionDenied
                }
                jc_memory_protocol::CommandErrorCode::NotFound => tonic::Code::NotFound,
                jc_memory_protocol::CommandErrorCode::Conflict => tonic::Code::AlreadyExists,
                jc_memory_protocol::CommandErrorCode::ResourceExhausted => {
                    tonic::Code::ResourceExhausted
                }
                jc_memory_protocol::CommandErrorCode::Internal => tonic::Code::Internal,
            };
            (code, Some(request_id.as_str()))
        }
    };
    let mut status = tonic::Status::new(code, error.to_string());
    if let Some(request_id) = request_id
        && let Ok(value) = request_id.parse()
    {
        status.metadata_mut().insert("x-request-id", value);
    }
    status
}

#[derive(Serialize)]
struct ErrorBody {
    code: String,
    message: String,
    request_id: Option<String>,
}

impl IntoResponse for ServerError {
    fn into_response(self) -> Response {
        let status = match &self {
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            Self::Timeout { .. } => StatusCode::GATEWAY_TIMEOUT,
            Self::Transport(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::Command { source, .. } => match source.code() {
                jc_memory_protocol::CommandErrorCode::InvalidArgument => StatusCode::BAD_REQUEST,
                jc_memory_protocol::CommandErrorCode::PermissionDenied => StatusCode::FORBIDDEN,
                jc_memory_protocol::CommandErrorCode::NotFound => StatusCode::NOT_FOUND,
                jc_memory_protocol::CommandErrorCode::Conflict => StatusCode::CONFLICT,
                jc_memory_protocol::CommandErrorCode::ResourceExhausted => {
                    StatusCode::TOO_MANY_REQUESTS
                }
                jc_memory_protocol::CommandErrorCode::Internal => StatusCode::INTERNAL_SERVER_ERROR,
            },
        };
        let code = match &self {
            Self::Unauthorized => "unauthorized".into(),
            Self::RateLimited => "resource_exhausted".into(),
            Self::Timeout { .. } => "deadline_exceeded".into(),
            Self::Transport(_) => "internal".into(),
            Self::Command { source, .. } => serde_json::to_value(source.code())
                .unwrap_or(json!("internal"))
                .as_str()
                .unwrap_or("internal")
                .into(),
        };
        (
            status,
            Json(ErrorBody {
                code,
                message: self.to_string(),
                request_id: match &self {
                    Self::Command { request_id, .. } | Self::Timeout { request_id } => {
                        Some(request_id.clone())
                    }
                    Self::Unauthorized | Self::RateLimited | Self::Transport(_) => None,
                },
            }),
        )
            .into_response()
    }
}
