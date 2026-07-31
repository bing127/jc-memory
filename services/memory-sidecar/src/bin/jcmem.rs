//! `jcmem` 本地 Sidecar 控制命令。

use std::{
    path::PathBuf,
    process::{Command, Stdio},
};

use jc_memory_sidecar::{
    CliCommand, DaemonCommand, DaemonResponse, RequestInput, SidecarConfig, TokenStore,
    decode_request, parse_args, send_command,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let command = match parse_args(std::env::args().skip(1))? {
        CliCommand::Start => {
            let data_dir = sidecar_data_dir();
            let config = SidecarConfig::local(data_dir);
            let token = TokenStore::load_or_create(&config.token_file).await?;
            let response = start_daemon(&config, &token).await?;
            println!("{}", serde_json::to_string(&response)?);
            return Ok(());
        }
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
    let data_dir = sidecar_data_dir();
    let config = SidecarConfig::local(data_dir);
    let token = TokenStore::load_or_create(&config.token_file).await?;
    let response = send_command(&config, &token, command).await?;
    println!("{}", serde_json::to_string(&response)?);
    Ok(())
}

fn sidecar_data_dir() -> PathBuf {
    std::env::var_os("JC_MEMORY_SIDECAR_DATA_DIR")
        .map_or_else(|| std::env::temp_dir().join("jc-memory"), PathBuf::from)
}

async fn start_daemon(
    config: &SidecarConfig,
    token: &TokenStore,
) -> Result<DaemonResponse, Box<dyn std::error::Error>> {
    if let Ok(response) = send_command(config, token, DaemonCommand::Status).await {
        return Ok(DaemonResponse {
            ok: response.ok,
            status: "already_running".into(),
            detail: response.detail,
            response: None,
        });
    }

    let binary = std::env::var_os("JC_MEMORY_SIDECAR_DAEMON_BIN").map_or_else(
        || {
            std::env::current_exe()
                .ok()
                .and_then(|path| path.parent().map(|parent| parent.join("jcmemd")))
                .unwrap_or_else(|| PathBuf::from("jcmemd"))
        },
        PathBuf::from,
    );
    let mut child = Command::new(&binary)
        .env("JC_MEMORY_SIDECAR_DATA_DIR", &config.data_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("启动 Sidecar 守护进程失败 ({})：{error}", binary.display()))?;

    for _ in 0..100 {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        if let Ok(response) = send_command(config, token, DaemonCommand::Status).await {
            return Ok(DaemonResponse {
                ok: response.ok,
                status: "started".into(),
                detail: Some(format!("pid={}", child.id())),
                response: None,
            });
        }
        if let Some(status) = child.try_wait()? {
            return Err(format!("Sidecar 守护进程提前退出：{status}").into());
        }
    }
    Err("等待 Sidecar 守护进程就绪超时".into())
}
