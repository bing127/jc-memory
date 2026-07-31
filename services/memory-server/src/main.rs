//! 疆诚记忆 Server 启动入口。

use jc_memory_core::{EmbeddedCommandBus, MemoryConfig, MemoryEngine};
use jc_memory_protocol::{MemoryScope, Operation};
use jc_memory_server::{ApiKey, ServerConfig, ServerState, serve};

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
            [Operation::Observe, Operation::Recall, Operation::Mutate],
        )],
        config.max_in_flight,
    );
    let listener = tokio::net::TcpListener::bind(config.bind).await?;
    tracing::info!(address = %config.bind, "疆诚记忆 Server 已启动");
    serve(listener, state, async {
        tokio::signal::ctrl_c().await.ok();
    })
    .await?;
    Ok(())
}
