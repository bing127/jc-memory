//! `jcmem` 本地 Sidecar 控制命令。

use jc_memory_sidecar::{
    CliCommand, DaemonCommand, RequestInput, SidecarConfig, TokenStore, decode_request, parse_args,
    send_command,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let command = match parse_args(std::env::args().skip(1))? {
        CliCommand::Status => DaemonCommand::Status,
        CliCommand::Stop => DaemonCommand::Stop,
        CliCommand::Doctor => DaemonCommand::Doctor,
        CliCommand::Execute { kind, input } => {
            let raw = match input {
                RequestInput::File(path) => tokio::fs::read_to_string(path).await?,
                RequestInput::Json(value) => value,
            };
            DaemonCommand::Execute {
                request: Box::new(decode_request(kind, &raw)?),
            }
        }
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
