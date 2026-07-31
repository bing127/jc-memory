//! 疆诚记忆本地 Sidecar 的认证与生命周期边界。

mod cli;
mod config;
mod lifecycle;
mod lock;
mod token;
mod transport;

pub use cli::{CliCommand, CliError, MemoryCommandKind, RequestInput, decode_request, parse_args};
pub use config::SidecarConfig;
pub use lifecycle::{
    DaemonCommand, DaemonResponse, SidecarDaemon, SidecarError, send_command, token_hex,
};
pub use lock::{InstanceLock, LockError};
pub use token::{TokenError, TokenStore};
pub use transport::{SidecarListener, SidecarStream, TransportError};
