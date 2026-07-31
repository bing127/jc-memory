//! MCP Server stdio 入口。

use jc_memory_core::{EmbeddedCommandBus, MemoryConfig, MemoryEngine};
use jc_memory_mcp::{McpServer, serve_stdio};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();
    let data_dir = std::env::var_os("JC_MEMORY_DATA_DIR").map_or_else(
        || std::env::temp_dir().join("jc-memory"),
        std::path::PathBuf::from,
    );
    let engine = MemoryEngine::open(MemoryConfig::local(data_dir)).await?;
    let bus = EmbeddedCommandBus::new(engine);
    let server = match std::env::var("JC_MEMORY_MCP_TOKEN") {
        Ok(token) => McpServer::with_auth_token(bus, token)?,
        Err(std::env::VarError::NotPresent) => McpServer::new(bus),
        Err(error) => return Err(error.into()),
    };
    serve_stdio(server).await?;
    Ok(())
}
