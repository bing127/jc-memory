//! 疆诚记忆 Server 启动入口。

use jc_memory_core::{EmbeddedCommandBus, MemoryConfig, MemoryEngine};
use jc_memory_protocol::{MemoryScope, Operation};
use jc_memory_server::{ApiKey, ServerConfig, ServerState, serve, serve_grpc_with_tls};
use tokio_util::sync::CancellationToken;
use tonic::transport::{Identity, ServerTlsConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();
    let config = ServerConfig::from_env()?;
    let engine = MemoryEngine::open(MemoryConfig::local(&config.data_dir)).await?;
    let bus = EmbeddedCommandBus::new(engine);
    let scope = MemoryScope::new(config.namespace_id);
    let state = ServerState::with_max_in_flight(
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
    );
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
