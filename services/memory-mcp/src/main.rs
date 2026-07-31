//! MCP Server stdio 入口。

use jc_memory_core::{EmbeddedCommandBus, MemoryConfig, MemoryEngine};
use jc_memory_mcp::{McpBackend, McpServer, SidecarCommandBus, serve_stdio};
use jc_memory_sidecar::{SidecarConfig, TokenStore};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();
    let backend = if let Some(data_dir) = std::env::var_os("JC_MEMORY_MCP_SIDECAR_DATA_DIR") {
        let config = SidecarConfig::local(data_dir);
        let token = TokenStore::load_or_create(&config.token_file).await?;
        McpBackend::Sidecar(SidecarCommandBus::new(config, token))
    } else {
        let data_dir = std::env::var_os("JC_MEMORY_DATA_DIR").map_or_else(
            || std::env::temp_dir().join("jc-memory"),
            std::path::PathBuf::from,
        );
        let engine = MemoryEngine::open(MemoryConfig::local(data_dir)).await?;
        McpBackend::Embedded(EmbeddedCommandBus::new(engine))
    };
    let server = match std::env::var("JC_MEMORY_MCP_TOKEN") {
        Ok(token) => McpServer::with_auth_token(backend, token)?,
        Err(std::env::VarError::NotPresent) => McpServer::new(backend),
        Err(error) => return Err(error.into()),
    };
    serve_stdio(server).await?;
    Ok(())
}
