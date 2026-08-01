//! 疆诚记忆 Server 启动入口。

use jc_memory_core::{EmbeddedCommandBus, MemoryConfig, MemoryEngine};
use jc_memory_protocol::{MemoryScope, Operation};
use std::{sync::Arc, time::Duration};

use jc_memory_runtime::{
    ApprovalStore, OpenAiAuth, OpenAiCompatibleConfig, OpenAiCompatibleProvider, RuntimeReadStore,
    RuntimeRunner, RuntimeService, ToolLoop, ToolRegistry,
};
use jc_memory_server::{
    ApiKey, CommandBusRuntimeBackend, ServerConfig, ServerState, serve, serve_grpc_with_tls,
};
use jc_memory_storage::{SqliteConfig, SqliteStore};
use tokio_util::sync::CancellationToken;
use tonic::transport::{Identity, ServerTlsConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();
    let config = ServerConfig::from_env()?;
    let engine = MemoryEngine::open(MemoryConfig::local(&config.data_dir)).await?;
    let bus = EmbeddedCommandBus::new(engine);
    let runtime_store =
        SqliteStore::open(&SqliteConfig::new(config.data_dir.join("memory.sqlite3"))).await?;
    let runtime_service = config
        .runtime_provider
        .clone()
        .map(|provider| {
            let auth = provider
                .api_key
                .map_or(OpenAiAuth::None, OpenAiAuth::Bearer);
            let provider = OpenAiCompatibleProvider::new(OpenAiCompatibleConfig {
                endpoint: provider.endpoint,
                model: provider.model,
                auth,
                timeout: Duration::from_millis(config.request_timeout_ms),
                max_retries: 1,
                retry_backoff: Duration::from_millis(100),
            })?;
            Ok::<_, jc_memory_runtime::ModelError>(RuntimeService::new(
                RuntimeRunner::from_store(&runtime_store),
                Arc::new(provider),
                Arc::new(CommandBusRuntimeBackend::new(bus.clone())),
                ToolLoop::new(ToolRegistry::default(), ApprovalStore::default()),
            ))
        })
        .transpose()?;
    let scope = MemoryScope::new(config.namespace_id);
    let mut state = ServerState::with_request_limits_and_timeout(
        bus,
        [ApiKey::for_scope(
            config.api_key,
            "server-api-key",
            scope,
            [
                Operation::Observe,
                Operation::Recall,
                Operation::Mutate,
                Operation::Explain,
            ],
        )],
        config.max_in_flight,
        config.max_requests_per_second,
        config.request_timeout_ms,
    )
    .with_runtime_read_store(RuntimeReadStore::from_store(&runtime_store));
    if let Some(runtime_service) = runtime_service {
        state = state.with_runtime_service(runtime_service);
    }
    let listener = tokio::net::TcpListener::bind(config.bind).await?;
    let grpc_listener = tokio::net::TcpListener::bind(config.grpc_bind).await?;
    let grpc_tls = match config.tls {
        Some(files) => {
            let certificate = tokio::fs::read(files.cert_file).await?;
            let private_key = tokio::fs::read(files.key_file).await?;
            Some(ServerTlsConfig::new().identity(Identity::from_pem(certificate, private_key)))
        }
        None => None,
    };
    let shutdown = CancellationToken::new();
    let http_task = tokio::spawn(serve(
        listener,
        state.clone(),
        shutdown.clone().cancelled_owned(),
    ));
    let grpc_task = tokio::spawn(serve_grpc_with_tls(
        grpc_listener,
        state,
        shutdown.clone().cancelled_owned(),
        grpc_tls,
    ));
    tracing::info!(
        http_address = %config.bind,
        grpc_address = %config.grpc_bind,
        "疆诚记忆 Server 已启动"
    );
    tokio::signal::ctrl_c().await?;
    shutdown.cancel();
    http_task.await??;
    grpc_task.await??;
    Ok(())
}
