//! `jcmemd` 本地 Sidecar 守护进程入口。

use std::sync::Arc;

use jc_memory_core::{EmbeddedCommandBus, MemoryConfig, MemoryEngine};
use jc_memory_sidecar::{SidecarConfig, SidecarDaemon};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();
    let data_dir = std::env::var_os("JC_MEMORY_SIDECAR_DATA_DIR").map_or_else(
        || std::env::temp_dir().join("jc-memory"),
        std::path::PathBuf::from,
    );
    let config = SidecarConfig::local(&data_dir);
    let engine = MemoryEngine::open(MemoryConfig::local(&data_dir)).await?;
    let bus = Arc::new(EmbeddedCommandBus::new(engine));
    let daemon = SidecarDaemon::bind_with_bus(config, bus).await?;
    tracing::info!("疆诚记忆 Sidecar 已启动");
    daemon.run().await?;
    Ok(())
}
