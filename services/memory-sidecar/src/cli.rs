//! `jcmem` 控制命令的参数解析与请求校验。

use std::path::PathBuf;

use jc_memory_protocol::{MemoryCommand, MemoryRequest};

/// Sidecar 支持的业务命令名称。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryCommandKind {
    /// 写入一条权威 Fragment。
    Observe,
    /// 批量追加运行时产生的权威 Fragment。
    Commit,
    /// 执行有界召回。
    Recall,
    /// 追加纠正记录。
    Correct,
    /// 追加遗忘记录。
    Forget,
    /// 返回召回解释轨迹。
    Explain,
    /// 读取 Scope 统计。
    Stats,
}

impl MemoryCommandKind {
    /// 返回命令对应的稳定文本。
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Observe => "observe",
            Self::Commit => "commit",
            Self::Recall => "recall",
            Self::Correct => "correct",
            Self::Forget => "forget",
            Self::Explain => "explain",
            Self::Stats => "stats",
        }
    }

    fn matches(self, command: &MemoryCommand) -> bool {
        matches!(
            (self, command),
            (Self::Observe, MemoryCommand::Observe(_))
                | (Self::Commit, MemoryCommand::Commit(_))
                | (Self::Recall, MemoryCommand::Recall(_))
                | (Self::Correct, MemoryCommand::Correct(_))
                | (Self::Forget, MemoryCommand::Forget(_))
                | (Self::Explain, MemoryCommand::Explain(_))
                | (Self::Stats, MemoryCommand::Stats(_))
        )
    }
}

/// 业务请求的输入来源。
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RequestInput {
    /// 从文件读取 JSON。
    File(PathBuf),
    /// 直接使用命令行中的 JSON 字符串。
    Json(String),
}

/// `jcmem` 已解析的控制命令。
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CliCommand {
    /// 查询 Sidecar 状态。
    Status,
    /// 停止 Sidecar。
    Stop,
    /// 诊断 Sidecar 文件与端点。
    Doctor,
    /// 转发一个指定业务命令。
    Execute {
        /// 命令名称。
        kind: MemoryCommandKind,
        /// 请求 JSON 来源。
        input: RequestInput,
    },
}

/// CLI 参数错误。
#[derive(Debug, thiserror::Error, Eq, PartialEq)]
pub enum CliError {
    /// 缺少顶层命令。
    #[error(
        "缺少命令；用法: jcmem <status|stop|doctor|observe|commit|recall|correct|forget|explain|stats>"
    )]
    MissingCommand,
    /// 命令名称不受支持。
    #[error("不支持的 Sidecar 命令: {0}")]
    UnknownCommand(String),
    /// 业务命令缺少请求输入。
    #[error("命令 {0} 必须提供 --request-file 或 --request-json")]
    MissingRequest(String),
    /// 请求输入参数重复或不受支持。
    #[error("命令参数错误: {0}")]
    InvalidArguments(String),
    /// 请求 JSON 无法解析。
    #[error("请求 JSON 无法解析: {0}")]
    InvalidRequest(String),
    /// 请求命令与 CLI 子命令不一致。
    #[error("请求命令与 CLI 子命令不一致；期望 {expected}")]
    CommandMismatch {
        /// CLI 子命令期望的命令名称。
        expected: &'static str,
    },
}

/// 解析 `jcmem` 的命令行参数。
///
/// # Errors
///
/// 命令缺失、参数不完整、选项未知或控制命令携带额外参数时返回错误。
pub fn parse_args<I, S>(arguments: I) -> Result<CliCommand, CliError>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut arguments = arguments.into_iter().map(Into::into);
    let Some(name) = arguments.next() else {
        return Err(CliError::MissingCommand);
    };
    let command = match name.as_str() {
        "status" => return finish_no_args(&mut arguments, CliCommand::Status),
        "stop" => return finish_no_args(&mut arguments, CliCommand::Stop),
        "doctor" => return finish_no_args(&mut arguments, CliCommand::Doctor),
        "observe" => MemoryCommandKind::Observe,
        "commit" => MemoryCommandKind::Commit,
        "recall" => MemoryCommandKind::Recall,
        "correct" => MemoryCommandKind::Correct,
        "forget" => MemoryCommandKind::Forget,
        "explain" => MemoryCommandKind::Explain,
        "stats" => MemoryCommandKind::Stats,
        other => return Err(CliError::UnknownCommand(other.to_owned())),
    };

    let mut input = None;
    while let Some(option) = arguments.next() {
        let value = arguments
            .next()
            .ok_or_else(|| CliError::InvalidArguments(format!("参数 {option} 缺少值")))?;
        let candidate = match option.as_str() {
            "--request-file" => RequestInput::File(PathBuf::from(value)),
            "--request-json" => RequestInput::Json(value),
            other => return Err(CliError::InvalidArguments(format!("不支持的参数 {other}"))),
        };
        if input.replace(candidate).is_some() {
            return Err(CliError::InvalidArguments("请求输入只能指定一次".into()));
        }
    }
    let input = input.ok_or_else(|| CliError::MissingRequest(command.as_str().into()))?;
    Ok(CliCommand::Execute {
        kind: command,
        input,
    })
}

fn finish_no_args<I>(arguments: &mut I, command: CliCommand) -> Result<CliCommand, CliError>
where
    I: Iterator<Item = String>,
{
    if let Some(extra) = arguments.next() {
        return Err(CliError::InvalidArguments(format!(
            "控制命令不接受额外参数: {extra}"
        )));
    }
    Ok(command)
}

/// 解析并校验业务请求 JSON，确保它与所选 CLI 子命令一致。
///
/// # Errors
///
/// JSON 无法反序列化、Envelope 校验失败或请求命令与子命令不一致时返回错误。
pub fn decode_request(kind: MemoryCommandKind, input: &str) -> Result<MemoryRequest, CliError> {
    let request: MemoryRequest =
        serde_json::from_str(input).map_err(|error| CliError::InvalidRequest(error.to_string()))?;
    request
        .validate()
        .map_err(|error| CliError::InvalidRequest(error.to_string()))?;
    if !kind.matches(&request.command) {
        return Err(CliError::CommandMismatch {
            expected: kind.as_str(),
        });
    }
    Ok(request)
}

#[cfg(test)]
mod tests {
    use super::{
        CliCommand, CliError, MemoryCommandKind, RequestInput, decode_request, parse_args,
    };
    use serde_json::json;

    #[test]
    fn parses_status_without_request_payload() {
        assert_eq!(parse_args(["status"]), Ok(CliCommand::Status));
        assert_eq!(
            parse_args(["status", "--request-json", "{}"]),
            Err(CliError::InvalidArguments(
                "控制命令不接受额外参数: --request-json".into()
            ))
        );
    }

    #[test]
    fn parses_each_memory_subcommand_with_json_or_file_input() {
        assert_eq!(
            parse_args(["observe", "--request-json", "{}"]),
            Ok(CliCommand::Execute {
                kind: MemoryCommandKind::Observe,
                input: RequestInput::Json("{}".into()),
            })
        );
        assert_eq!(
            parse_args(["stats", "--request-file", "request.json"]),
            Ok(CliCommand::Execute {
                kind: MemoryCommandKind::Stats,
                input: RequestInput::File("request.json".into()),
            })
        );
    }

    #[test]
    fn rejects_missing_input_unknown_option_and_duplicate_input() {
        assert_eq!(
            parse_args(["recall"]),
            Err(CliError::MissingRequest("recall".into()))
        );
        assert_eq!(
            parse_args(["recall", "--unknown", "value"]),
            Err(CliError::InvalidArguments("不支持的参数 --unknown".into()))
        );
        assert_eq!(
            parse_args(["recall", "--request-json", "{}", "--request-file", "x"]),
            Err(CliError::InvalidArguments("请求输入只能指定一次".into()))
        );
    }

    #[test]
    fn validates_request_envelope_before_command_dispatch() {
        let error = decode_request(MemoryCommandKind::Stats, "{}").unwrap_err();
        assert!(matches!(error, CliError::InvalidRequest(_)));
    }

    #[test]
    fn rejects_a_valid_request_when_the_subcommand_kind_does_not_match() {
        let request = json!({
            "api_version": "v1",
            "request_id": "cli-stats",
            "access_scope": {
                "namespace_id": "00000000-0000-7000-8000-000000000001"
            },
            "command": {"command": "stats", "payload": null}
        });
        let encoded = serde_json::to_string(&request).unwrap();
        assert_eq!(
            decode_request(MemoryCommandKind::Observe, &encoded),
            Err(CliError::CommandMismatch {
                expected: "observe"
            })
        );
        assert!(decode_request(MemoryCommandKind::Stats, &encoded).is_ok());
    }
}
