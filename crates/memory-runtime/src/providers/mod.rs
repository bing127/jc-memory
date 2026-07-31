//! 内置 HTTP 模型 Provider。

mod http;
mod openai_compatible;

pub use http::{GenericHttpConfig, GenericHttpProvider};
pub use openai_compatible::{OpenAiAuth, OpenAiCompatibleConfig, OpenAiCompatibleProvider};

use std::future::Future;

use tokio_util::sync::CancellationToken;

use crate::ModelError;

async fn with_control_until<F, T>(
    future: F,
    cancellation: &CancellationToken,
    deadline: tokio::time::Instant,
) -> Result<T, ModelError>
where
    F: Future<Output = Result<T, reqwest::Error>>,
{
    tokio::select! {
        biased;
        () = cancellation.cancelled() => Err(ModelError::Cancelled),
        result = tokio::time::timeout_at(deadline, future) => match result {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(error)) => Err(ModelError::Transport(error.to_string())),
            Err(_) => Err(ModelError::Timeout),
        },
    }
}

fn parse_text_output(raw_text: String) -> Result<crate::ModelResponse, ModelError> {
    let value = serde_json::from_str(&raw_text).map_err(|error| {
        ModelError::InvalidResponse(format!("model content is not JSON: {error}"))
    })?;
    let output = crate::parse_model_output(value)?;
    Ok(crate::ModelResponse { output, raw_text })
}

fn parse_http_url(endpoint: &str) -> Result<reqwest::Url, ModelError> {
    let url = reqwest::Url::parse(endpoint)
        .map_err(|error| ModelError::InvalidResponse(format!("invalid provider URL: {error}")))?;
    if matches!(url.scheme(), "http" | "https") {
        Ok(url)
    } else {
        Err(ModelError::InvalidResponse(
            "provider URL must use HTTP or HTTPS".into(),
        ))
    }
}

fn valid_json_pointer(pointer: &str) -> bool {
    if pointer.is_empty() {
        return true;
    }
    if !pointer.starts_with('/') {
        return false;
    }
    let bytes = pointer.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'~' {
            index += 1;
            if index == bytes.len() || !matches!(bytes[index], b'0' | b'1') {
                return false;
            }
        }
        index += 1;
    }
    true
}
