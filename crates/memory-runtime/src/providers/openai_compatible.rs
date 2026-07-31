//! `OpenAI` Chat Completions 兼容 Provider。

use std::time::Duration;

use async_trait::async_trait;
use futures::StreamExt as _;
use reqwest::header::{HeaderName, HeaderValue};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::{ModelDescriptor, ModelError, ModelProvider, ModelRequest, ModelResponse, ModelRole};

use super::{parse_http_url, parse_text_output, with_control_until};

/// `OpenAI` Compatible 鉴权方式。
pub enum OpenAiAuth {
    /// 不添加认证 Header。
    None,
    /// 使用标准 Bearer Token。
    Bearer(String),
    /// 使用自定义 Header 名称和值。
    Header {
        /// Header 名称。
        name: String,
        /// Header 值。
        value: String,
    },
}

/// `OpenAI` Compatible Provider 配置。
pub struct OpenAiCompatibleConfig {
    /// Chat Completions HTTP 地址。
    pub endpoint: String,
    /// 请求中的模型标识。
    pub model: String,
    /// 鉴权配置。
    pub auth: OpenAiAuth,
    /// 每次 HTTP 尝试的完整超时。
    pub timeout: Duration,
    /// 遇到 429 或 5xx 后允许的额外尝试次数。
    pub max_retries: u32,
    /// 两次尝试间的固定等待时间。
    pub retry_backoff: Duration,
}

enum PreparedAuth {
    None,
    Bearer(HeaderValue),
    Header(HeaderName, HeaderValue),
}

/// 支持非流式与 SSE 的 `OpenAI` Chat Completions Provider。
pub struct OpenAiCompatibleProvider {
    client: reqwest::Client,
    endpoint: reqwest::Url,
    model: String,
    auth: PreparedAuth,
    timeout: Duration,
    max_retries: u32,
    retry_backoff: Duration,
}

impl OpenAiCompatibleProvider {
    /// 校验配置并创建 Provider。
    ///
    /// # Errors
    ///
    /// 地址、模型、超时或自定义 Header 无效时返回错误。
    pub fn new(config: OpenAiCompatibleConfig) -> Result<Self, ModelError> {
        if config.endpoint.trim().is_empty() || config.model.trim().is_empty() {
            return Err(ModelError::InvalidResponse(
                "endpoint and model must be non-empty".into(),
            ));
        }
        if config.timeout.is_zero() {
            return Err(ModelError::InvalidResponse(
                "provider timeout must be greater than zero".into(),
            ));
        }
        let endpoint = parse_http_url(&config.endpoint)?;
        let auth = match config.auth {
            OpenAiAuth::None => PreparedAuth::None,
            OpenAiAuth::Bearer(token) => {
                let value = HeaderValue::try_from(format!("Bearer {token}")).map_err(|error| {
                    ModelError::InvalidResponse(format!("invalid Bearer token: {error}"))
                })?;
                PreparedAuth::Bearer(value)
            }
            OpenAiAuth::Header { name, value } => {
                let name = HeaderName::try_from(name).map_err(|error| {
                    ModelError::InvalidResponse(format!("invalid HTTP header name: {error}"))
                })?;
                let value = HeaderValue::try_from(value).map_err(|error| {
                    ModelError::InvalidResponse(format!("invalid HTTP header value: {error}"))
                })?;
                PreparedAuth::Header(name, value)
            }
        };
        Ok(Self {
            client: reqwest::Client::new(),
            endpoint,
            model: config.model,
            auth,
            timeout: config.timeout,
            max_retries: config.max_retries,
            retry_backoff: config.retry_backoff,
        })
    }

    async fn invoke_once(
        &self,
        request: &ModelRequest,
        cancellation: &CancellationToken,
    ) -> Result<(reqwest::Response, tokio::time::Instant), ModelError> {
        let deadline = tokio::time::Instant::now() + self.timeout;
        let messages: Vec<_> = request
            .messages
            .iter()
            .map(|message| {
                json!({
                    "role": role_name(message.role),
                    "content": message.content,
                })
            })
            .collect();
        let body = json!({
            "model": self.model,
            "messages": messages,
            "stream": request.stream,
        });
        let builder = self.client.post(self.endpoint.clone()).json(&body);
        let builder = match &self.auth {
            PreparedAuth::None => builder,
            PreparedAuth::Bearer(value) => builder.header(reqwest::header::AUTHORIZATION, value),
            PreparedAuth::Header(name, value) => builder.header(name, value),
        };
        let response = with_control_until(builder.send(), cancellation, deadline).await?;
        Ok((response, deadline))
    }

    async fn wait_before_retry(&self, cancellation: &CancellationToken) -> Result<(), ModelError> {
        if self.retry_backoff.is_zero() {
            return if cancellation.is_cancelled() {
                Err(ModelError::Cancelled)
            } else {
                Ok(())
            };
        }
        tokio::select! {
            biased;
            () = cancellation.cancelled() => Err(ModelError::Cancelled),
            () = tokio::time::sleep(self.retry_backoff) => Ok(()),
        }
    }
}

#[async_trait]
impl ModelProvider for OpenAiCompatibleProvider {
    async fn invoke(
        &self,
        request: ModelRequest,
        cancellation: CancellationToken,
    ) -> Result<ModelResponse, ModelError> {
        let mut retries = 0;
        loop {
            let (response, deadline) = self.invoke_once(&request, &cancellation).await?;
            let status = response.status();
            if status.as_u16() == 429 || status.is_server_error() {
                if retries < self.max_retries {
                    retries += 1;
                    drop(response);
                    self.wait_before_retry(&cancellation).await?;
                    continue;
                }
                return Err(ModelError::HttpStatus(status.as_u16()));
            }
            if !status.is_success() {
                return Err(ModelError::HttpStatus(status.as_u16()));
            }
            return if request.stream {
                parse_sse_response(response, &cancellation, deadline).await
            } else {
                parse_json_response(response, &cancellation, deadline).await
            };
        }
    }

    fn descriptor(&self) -> ModelDescriptor {
        ModelDescriptor {
            provider: "openai_compatible".into(),
            model: self.model.clone(),
            supports_streaming: true,
        }
    }
}

async fn parse_json_response(
    response: reqwest::Response,
    cancellation: &CancellationToken,
    deadline: tokio::time::Instant,
) -> Result<ModelResponse, ModelError> {
    let value: Value = with_control_until(response.json(), cancellation, deadline).await?;
    let raw_text = value
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .ok_or_else(|| ModelError::InvalidResponse("missing message content".into()))?
        .to_owned();
    parse_text_output(raw_text)
}

async fn parse_sse_response(
    response: reqwest::Response,
    cancellation: &CancellationToken,
    deadline: tokio::time::Instant,
) -> Result<ModelResponse, ModelError> {
    let mut stream = response.bytes_stream();
    let mut buffer = Vec::new();
    let mut data_lines = Vec::new();
    let mut raw_text = String::new();
    loop {
        let next = tokio::select! {
            biased;
            () = cancellation.cancelled() => return Err(ModelError::Cancelled),
            result = tokio::time::timeout_at(deadline, stream.next()) => {
                result.map_err(|_| ModelError::Timeout)?
            }
        };
        let Some(chunk) = next else {
            break;
        };
        let chunk = chunk.map_err(|error| ModelError::Transport(error.to_string()))?;
        buffer.extend_from_slice(&chunk);
        while let Some(position) = buffer.iter().position(|byte| *byte == b'\n') {
            let mut line: Vec<u8> = buffer.drain(..=position).collect();
            line.pop();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            if process_sse_line(&line, &mut data_lines, &mut raw_text)? {
                return parse_text_output(raw_text);
            }
        }
    }
    if !buffer.is_empty() {
        process_sse_line(&buffer, &mut data_lines, &mut raw_text)?;
    }
    dispatch_sse_event(&mut data_lines, &mut raw_text)?;
    parse_text_output(raw_text)
}

fn process_sse_line(
    line: &[u8],
    data_lines: &mut Vec<String>,
    raw_text: &mut String,
) -> Result<bool, ModelError> {
    if line.is_empty() {
        return dispatch_sse_event(data_lines, raw_text);
    }
    if line.first() == Some(&b':') {
        return Ok(false);
    }
    let line = std::str::from_utf8(line)
        .map_err(|error| ModelError::InvalidResponse(format!("SSE line is not UTF-8: {error}")))?;
    let (field, value) = line.split_once(':').unwrap_or((line, ""));
    if field == "data" {
        data_lines.push(value.strip_prefix(' ').unwrap_or(value).to_owned());
    }
    Ok(false)
}

fn dispatch_sse_event(
    data_lines: &mut Vec<String>,
    raw_text: &mut String,
) -> Result<bool, ModelError> {
    if data_lines.is_empty() {
        return Ok(false);
    }
    let data = data_lines.join("\n");
    data_lines.clear();
    if data == "[DONE]" {
        return Ok(true);
    }
    let value: Value = serde_json::from_str(&data)
        .map_err(|error| ModelError::InvalidResponse(format!("invalid SSE data JSON: {error}")))?;
    if let Some(content) = value.pointer("/choices/0/delta/content") {
        let content = content
            .as_str()
            .ok_or_else(|| ModelError::InvalidResponse("SSE delta content is not text".into()))?;
        raw_text.push_str(content);
    }
    Ok(false)
}

const fn role_name(role: ModelRole) -> &'static str {
    match role {
        ModelRole::System => "system",
        ModelRole::User => "user",
        ModelRole::Assistant => "assistant",
    }
}
