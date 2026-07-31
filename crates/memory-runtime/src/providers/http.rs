//! 配置化 JSON Pointer 的通用 HTTP Provider。

use std::{collections::BTreeMap, time::Duration};

use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::{ModelDescriptor, ModelError, ModelProvider, ModelRequest, ModelResponse};

use super::{parse_http_url, parse_text_output, valid_json_pointer, with_control_until};

/// 通用 HTTP Provider 配置。
pub struct GenericHttpConfig {
    /// HTTP POST 地址。
    pub endpoint: String,
    /// Provider 描述标识。
    pub provider: String,
    /// 模型描述标识。
    pub model: String,
    /// 固定请求 Header。
    pub headers: BTreeMap<String, String>,
    /// 每次请求克隆的 JSON 模板。
    pub request_template: Value,
    /// 写入 [`ModelRequest`] 的 RFC 6901 JSON Pointer。
    pub request_pointer: String,
    /// 从响应提取候选输出的 RFC 6901 JSON Pointer。
    pub response_pointer: String,
    /// 单次完整调用的超时。
    pub timeout: Duration,
}

/// 使用 JSON Pointer 映射请求与响应的通用 HTTP Provider。
pub struct GenericHttpProvider {
    client: reqwest::Client,
    endpoint: reqwest::Url,
    descriptor: ModelDescriptor,
    headers: HeaderMap,
    request_template: Value,
    request_pointer: String,
    response_pointer: String,
    timeout: Duration,
}

impl GenericHttpProvider {
    /// 校验配置并创建 Provider。
    ///
    /// # Errors
    ///
    /// 地址、标识、超时或 Header 无效时返回错误。
    pub fn new(config: GenericHttpConfig) -> Result<Self, ModelError> {
        validate_common(
            &config.endpoint,
            &config.provider,
            &config.model,
            config.timeout,
        )?;
        let endpoint = parse_http_url(&config.endpoint)?;
        if !valid_json_pointer(&config.request_pointer)
            || config
                .request_template
                .pointer(&config.request_pointer)
                .is_none()
        {
            return Err(ModelError::InvalidResponse(
                "request JSON Pointer is invalid or missing from template".into(),
            ));
        }
        if !valid_json_pointer(&config.response_pointer) {
            return Err(ModelError::InvalidResponse(
                "response JSON Pointer is invalid".into(),
            ));
        }
        let mut headers = HeaderMap::new();
        for (name, value) in config.headers {
            let name = HeaderName::try_from(name).map_err(|error| {
                ModelError::InvalidResponse(format!("invalid HTTP header name: {error}"))
            })?;
            let value = HeaderValue::try_from(value).map_err(|error| {
                ModelError::InvalidResponse(format!("invalid HTTP header value: {error}"))
            })?;
            headers.insert(name, value);
        }
        Ok(Self {
            client: reqwest::Client::new(),
            endpoint,
            descriptor: ModelDescriptor {
                provider: config.provider,
                model: config.model,
                supports_streaming: false,
            },
            headers,
            request_template: config.request_template,
            request_pointer: config.request_pointer,
            response_pointer: config.response_pointer,
            timeout: config.timeout,
        })
    }
}

#[async_trait]
impl ModelProvider for GenericHttpProvider {
    async fn invoke(
        &self,
        request: ModelRequest,
        cancellation: CancellationToken,
    ) -> Result<ModelResponse, ModelError> {
        let mut body = self.request_template.clone();
        let request_value = serde_json::to_value(request).map_err(|error| {
            ModelError::InvalidResponse(format!("cannot encode model request: {error}"))
        })?;
        let target = body.pointer_mut(&self.request_pointer).ok_or_else(|| {
            ModelError::InvalidResponse("request JSON Pointer does not exist".into())
        })?;
        *target = request_value;
        let deadline = tokio::time::Instant::now() + self.timeout;
        let response = with_control_until(
            self.client
                .post(self.endpoint.clone())
                .headers(self.headers.clone())
                .json(&body)
                .send(),
            &cancellation,
            deadline,
        )
        .await?;
        let status = response.status();
        if !status.is_success() {
            return Err(ModelError::HttpStatus(status.as_u16()));
        }
        let value: Value = with_control_until(response.json(), &cancellation, deadline).await?;
        let candidate = value.pointer(&self.response_pointer).ok_or_else(|| {
            ModelError::InvalidResponse("response JSON Pointer does not exist".into())
        })?;
        let raw_text = match candidate {
            Value::String(text) => text.clone(),
            value => serde_json::to_string(value).map_err(|error| {
                ModelError::InvalidResponse(format!("cannot encode mapped response: {error}"))
            })?,
        };
        parse_text_output(raw_text)
    }

    fn descriptor(&self) -> ModelDescriptor {
        self.descriptor.clone()
    }
}

fn validate_common(
    endpoint: &str,
    provider: &str,
    model: &str,
    timeout: Duration,
) -> Result<(), ModelError> {
    if endpoint.trim().is_empty() || provider.trim().is_empty() || model.trim().is_empty() {
        return Err(ModelError::InvalidResponse(
            "endpoint, provider and model must be non-empty".into(),
        ));
    }
    if timeout.is_zero() {
        return Err(ModelError::InvalidResponse(
            "provider timeout must be greater than zero".into(),
        ));
    }
    Ok(())
}
