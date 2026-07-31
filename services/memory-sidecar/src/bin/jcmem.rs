//! `jcmem` 本地 Sidecar 控制命令。

use jc_memory_sidecar::{DaemonCommand, SidecarConfig, TokenStore, send_command};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = std::env::args().skip(1);
    let command = match arguments.next().as_deref() {
        Some("status") => DaemonCommand::Status,
        Some("stop") => DaemonCommand::Stop,
        Some("doctor") => DaemonCommand::Doctor,
        Some(other) => return Err(format!("不支持的 Sidecar 命令: {other}").into()),
        None => return Err("用法: jcmem <status|stop|doctor>".into()),
    };
    let data_dir = std::env::var_os("JC_MEMORY_SIDECAR_DATA_DIR").map_or_else(
        || std::env::temp_dir().join("jc-memory"),
        std::path::PathBuf::from,
    );
    let config = SidecarConfig::local(data_dir);
    let token = TokenStore::load_or_create(&config.token_file).await?;
    let response = send_command(&config, &token, command).await?;
    println!("{}", serde_json::to_string(&response)?);
    Ok(())
}
