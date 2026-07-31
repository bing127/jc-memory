//! `ModelProvider` 与模型输出解析契约测试。

use std::{
    collections::{BTreeMap, VecDeque},
    io::{Read as _, Write as _},
    net::TcpListener,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use async_trait::async_trait;
use jc_memory_runtime::{
    DecisionCandidate, FinalResponse, GenericHttpConfig, GenericHttpProvider, Hypothesis,
    ModelDescriptor, ModelError, ModelMessage, ModelOutput, ModelProvider, ModelRequest,
    ModelResponse, OpenAiAuth, OpenAiCompatibleConfig, OpenAiCompatibleProvider, Plan, Prediction,
    ToolRequest, parse_model_output,
};
use serde_json::json;
use tokio_util::sync::CancellationToken;

struct DeterministicProvider;

#[async_trait]
impl ModelProvider for DeterministicProvider {
    async fn invoke(
        &self,
        request: ModelRequest,
        cancellation: CancellationToken,
    ) -> Result<ModelResponse, ModelError> {
        if cancellation.is_cancelled() {
            return Err(ModelError::Cancelled);
        }
        let text = request
            .messages
            .last()
            .map(|message| message.content.clone())
            .unwrap_or_default();
        Ok(ModelResponse {
            output: ModelOutput::FinalResponse(FinalResponse { text: text.clone() }),
            raw_text: text,
        })
    }

    fn descriptor(&self) -> ModelDescriptor {
        ModelDescriptor {
            provider: "deterministic".into(),
            model: "fixture-v1".into(),
            supports_streaming: false,
        }
    }
}

#[tokio::test]
async fn custom_provider_contract_is_deterministic_and_cancellable() {
    let provider = DeterministicProvider;
    let request = ModelRequest {
        messages: vec![ModelMessage::user("确定性回答")],
        stream: false,
    };
    let response = provider
        .invoke(request.clone(), CancellationToken::new())
        .await
        .expect("确定性 Provider 应返回结果");
    assert_eq!(
        response.output,
        ModelOutput::FinalResponse(FinalResponse {
            text: "确定性回答".into()
        })
    );
    assert_eq!(provider.descriptor().model, "fixture-v1");

    let cancellation = CancellationToken::new();
    cancellation.cancel();
    assert_eq!(
        provider.invoke(request, cancellation).await,
        Err(ModelError::Cancelled)
    );
}

#[test]
fn parser_accepts_all_six_typed_output_kinds() {
    let cases = [
        (
            json!({"kind":"hypothesis","payload":{"statement":"可能缺少索引","rationale":"查询变慢"}}),
            ModelOutput::Hypothesis(Hypothesis {
                statement: "可能缺少索引".into(),
                rationale: Some("查询变慢".into()),
            }),
        ),
        (
            json!({"kind":"prediction","payload":{"statement":"测试会通过","confidence":0.7}}),
            ModelOutput::Prediction(Prediction {
                statement: "测试会通过".into(),
                confidence: 0.7,
            }),
        ),
        (
            json!({"kind":"plan","payload":{"goal":"修复问题","steps":["复现","修复","验证"]}}),
            ModelOutput::Plan(Plan {
                goal: "修复问题".into(),
                steps: vec!["复现".into(), "修复".into(), "验证".into()],
            }),
        ),
        (
            json!({"kind":"decision_candidate","payload":{"decision":"运行测试","rationale":"需要真实证据"}}),
            ModelOutput::DecisionCandidate(DecisionCandidate {
                decision: "运行测试".into(),
                rationale: "需要真实证据".into(),
            }),
        ),
        (
            json!({"kind":"tool_request","payload":{"capability_id":"cargo.test","arguments":{"package":"jc-memory-runtime"}}}),
            ModelOutput::ToolRequest(ToolRequest {
                capability_id: "cargo.test".into(),
                arguments: json!({"package":"jc-memory-runtime"}),
            }),
        ),
        (
            json!({"kind":"final_response","payload":{"text":"处理完成"}}),
            ModelOutput::FinalResponse(FinalResponse {
                text: "处理完成".into(),
            }),
        ),
    ];

    for (input, expected) in cases {
        assert_eq!(parse_model_output(input), Ok(expected));
    }
}

#[test]
fn parser_rejects_unknown_kind_and_schema_mismatch() {
    for invalid in [
        json!({"kind":"fact","payload":{"statement":"不得成为事实"}}),
        json!({"kind":"prediction","payload":{"statement":"缺少 confidence"}}),
        json!({"kind":"prediction","payload":{"statement":"越界","confidence":1.1}}),
        json!({"kind":"plan","payload":{"goal":"空步骤","steps":[]}}),
        json!({"kind":"tool_request","payload":{"capability_id":"tool","arguments":[]}}),
        json!({"kind":"final_response","payload":{"text":"回答","unexpected":true}}),
        json!({"kind":"final_response","payload":{"text":"回答"},"unexpected":true}),
    ] {
        assert!(parse_model_output(invalid).is_err());
    }
}

#[test]
fn provider_configuration_rejects_invalid_urls_pointers_headers_and_timeouts() {
    assert!(
        OpenAiCompatibleProvider::new(OpenAiCompatibleConfig {
            endpoint: "not a URL".into(),
            model: "fixture".into(),
            auth: OpenAiAuth::None,
            timeout: Duration::from_secs(1),
            max_retries: 0,
            retry_backoff: Duration::ZERO,
        })
        .is_err()
    );
    assert!(
        OpenAiCompatibleProvider::new(OpenAiCompatibleConfig {
            endpoint: "http://127.0.0.1".into(),
            model: "fixture".into(),
            auth: OpenAiAuth::Bearer("bad\ntoken".into()),
            timeout: Duration::from_secs(1),
            max_retries: 0,
            retry_backoff: Duration::ZERO,
        })
        .is_err()
    );
    assert!(
        OpenAiCompatibleProvider::new(OpenAiCompatibleConfig {
            endpoint: "http://127.0.0.1".into(),
            model: "fixture".into(),
            auth: OpenAiAuth::Header {
                name: "bad header".into(),
                value: "secret".into(),
            },
            timeout: Duration::from_secs(1),
            max_retries: 0,
            retry_backoff: Duration::ZERO,
        })
        .is_err()
    );
    for (request_pointer, response_pointer, timeout) in [
        ("/missing", "/result", Duration::from_secs(1)),
        ("/input", "result", Duration::from_secs(1)),
        ("/input", "/result", Duration::ZERO),
    ] {
        assert!(
            GenericHttpProvider::new(GenericHttpConfig {
                endpoint: "http://127.0.0.1".into(),
                provider: "fixture".into(),
                model: "fixture".into(),
                headers: BTreeMap::default(),
                request_template: json!({"input": null}),
                request_pointer: request_pointer.into(),
                response_pointer: response_pointer.into(),
                timeout,
            })
            .is_err()
        );
    }
}

#[tokio::test]
async fn openai_non_streaming_applies_bearer_and_parses_output() {
    let server = TestServer::start(vec![ResponseSpec::json(&openai_response(
        &json!({"kind":"final_response","payload":{"text":"非流式完成"}}),
    ))]);
    let provider = OpenAiCompatibleProvider::new(OpenAiCompatibleConfig {
        endpoint: server.endpoint(),
        model: "fixture-model".into(),
        auth: OpenAiAuth::Bearer("secret-token".into()),
        timeout: Duration::from_secs(1),
        max_retries: 0,
        retry_backoff: Duration::ZERO,
    })
    .expect("创建 OpenAI Compatible Provider");
    let response = provider
        .invoke(request(false), CancellationToken::new())
        .await
        .expect("解析非流式响应");
    assert_eq!(
        response.output,
        ModelOutput::FinalResponse(FinalResponse {
            text: "非流式完成".into()
        })
    );
    let requests = server.finish();
    assert_eq!(requests.len(), 1);
    let lower = requests[0].headers.to_ascii_lowercase();
    assert!(lower.contains("authorization: bearer secret-token"));
    assert_eq!(requests[0].body["model"], "fixture-model");
    assert_eq!(requests[0].body["stream"], false);
}

#[tokio::test]
async fn openai_custom_header_auth_is_applied() {
    let server = TestServer::start(vec![ResponseSpec::json(&openai_response(
        &json!({"kind":"final_response","payload":{"text":"Header 完成"}}),
    ))]);
    let provider = OpenAiCompatibleProvider::new(OpenAiCompatibleConfig {
        endpoint: server.endpoint(),
        model: "fixture-model".into(),
        auth: OpenAiAuth::Header {
            name: "x-api-key".into(),
            value: "header-secret".into(),
        },
        timeout: Duration::from_secs(1),
        max_retries: 0,
        retry_backoff: Duration::ZERO,
    })
    .unwrap();
    provider
        .invoke(request(false), CancellationToken::new())
        .await
        .unwrap();
    let requests = server.finish();
    assert!(
        requests[0]
            .headers
            .to_ascii_lowercase()
            .contains("x-api-key: header-secret")
    );
}

#[tokio::test]
async fn openai_streaming_reassembles_sse_across_transport_chunks() {
    let first =
        "data: {\"choices\":[{\"delta\":{\"content\":\"{\\\"kind\\\":\\\"final_res\"}}]}\n\n";
    let second = "data: {\"choices\":[{\"delta\":{\"content\":\"ponse\\\",\\\"payload\\\":{\\\"text\\\":\\\"流式完成\\\"}}\"}}]}\n\n";
    let done = "data: [DONE]\n\n";
    let body = format!("{first}{second}{done}");
    let split = body.len() / 3;
    let server = TestServer::start(vec![ResponseSpec::sse(vec![
        body.as_bytes()[..split].to_vec(),
        body.as_bytes()[split..split + 5].to_vec(),
        body.as_bytes()[split + 5..].to_vec(),
    ])]);
    let provider = openai_provider(server.endpoint(), Duration::from_secs(1), 0);
    let response = provider
        .invoke(request(true), CancellationToken::new())
        .await
        .expect("解析跨传输分片的 SSE");
    assert_eq!(
        response.output,
        ModelOutput::FinalResponse(FinalResponse {
            text: "流式完成".into()
        })
    );
    assert_eq!(server.finish().len(), 1);
}

#[tokio::test]
async fn openai_retries_429_and_5xx_with_a_hard_bound() {
    let success = ResponseSpec::json(&openai_response(
        &json!({"kind":"final_response","payload":{"text":"重试成功"}}),
    ));
    let server = TestServer::start(vec![
        ResponseSpec::status(429),
        ResponseSpec::status(503),
        success,
    ]);
    let provider = openai_provider(server.endpoint(), Duration::from_secs(1), 2);
    let response = provider
        .invoke(request(false), CancellationToken::new())
        .await
        .expect("第三次调用应成功");
    assert!(matches!(response.output, ModelOutput::FinalResponse(_)));
    assert_eq!(server.finish().len(), 3);

    let bounded = TestServer::start(vec![
        ResponseSpec::status(500),
        ResponseSpec::status(500),
        ResponseSpec::status(500),
    ]);
    let provider = openai_provider(bounded.endpoint(), Duration::from_secs(1), 2);
    assert_eq!(
        provider
            .invoke(request(false), CancellationToken::new())
            .await,
        Err(ModelError::HttpStatus(500))
    );
    assert_eq!(bounded.finish().len(), 3);
}

#[tokio::test]
async fn openai_timeout_and_cancellation_are_distinct() {
    let timed_out = TestServer::start(vec![ResponseSpec::delayed_json(
        &openai_response(&json!({"kind":"final_response","payload":{"text":"过晚"}})),
        Duration::from_millis(150),
    )]);
    let provider = openai_provider(timed_out.endpoint(), Duration::from_millis(20), 0);
    assert_eq!(
        provider
            .invoke(request(false), CancellationToken::new())
            .await,
        Err(ModelError::Timeout)
    );
    assert_eq!(timed_out.finish().len(), 1);

    let cancelled = TestServer::start(vec![ResponseSpec::delayed_json(
        &openai_response(&json!({"kind":"final_response","payload":{"text":"已取消"}})),
        Duration::from_millis(150),
    )]);
    let provider = openai_provider(cancelled.endpoint(), Duration::from_secs(1), 0);
    let cancellation = CancellationToken::new();
    let trigger = cancellation.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(20)).await;
        trigger.cancel();
    });
    assert_eq!(
        provider.invoke(request(false), cancellation).await,
        Err(ModelError::Cancelled)
    );
    assert_eq!(cancelled.finish().len(), 1);
}

#[tokio::test]
async fn openai_timeout_covers_headers_and_body_with_one_deadline() {
    let server = TestServer::start(vec![ResponseSpec::staged_json(
        &openai_response(&json!({"kind":"final_response","payload":{"text":"超过总预算"}})),
        Duration::from_millis(40),
        Duration::from_millis(40),
    )]);
    let provider = openai_provider(server.endpoint(), Duration::from_millis(60), 0);
    assert_eq!(
        provider
            .invoke(request(false), CancellationToken::new())
            .await,
        Err(ModelError::Timeout)
    );
    assert_eq!(server.finish().len(), 1);
}

#[tokio::test]
async fn generic_http_uses_json_pointer_request_and_response_mapping() {
    let server = TestServer::start(vec![ResponseSpec::json(&json!({
        "result": {
            "candidate": {"kind":"final_response","payload":{"text":"通用 HTTP 完成"}}
        }
    }))]);
    let provider = GenericHttpProvider::new(GenericHttpConfig {
        endpoint: server.endpoint(),
        provider: "generic-fixture".into(),
        model: "generic-v1".into(),
        headers: [("x-provider-key".into(), "generic-secret".into())]
            .into_iter()
            .collect(),
        request_template: json!({"input": null, "fixed": {"temperature": 0}}),
        request_pointer: "/input".into(),
        response_pointer: "/result/candidate".into(),
        timeout: Duration::from_secs(1),
    })
    .expect("创建 Generic HTTP Provider");
    let response = provider
        .invoke(request(false), CancellationToken::new())
        .await
        .expect("解析 JSON Pointer 输出");
    assert_eq!(
        response.output,
        ModelOutput::FinalResponse(FinalResponse {
            text: "通用 HTTP 完成".into()
        })
    );
    let requests = server.finish();
    assert_eq!(
        requests[0].body["input"]["messages"][0]["content"],
        "请回答"
    );
    assert_eq!(requests[0].body["fixed"]["temperature"], 0);
    assert!(
        requests[0]
            .headers
            .to_ascii_lowercase()
            .contains("x-provider-key: generic-secret")
    );
}

fn request(stream: bool) -> ModelRequest {
    ModelRequest {
        messages: vec![ModelMessage::user("请回答")],
        stream,
    }
}

fn openai_provider(
    endpoint: String,
    timeout: Duration,
    max_retries: u32,
) -> OpenAiCompatibleProvider {
    OpenAiCompatibleProvider::new(OpenAiCompatibleConfig {
        endpoint,
        model: "fixture-model".into(),
        auth: OpenAiAuth::None,
        timeout,
        max_retries,
        retry_backoff: Duration::ZERO,
    })
    .unwrap()
}

fn openai_response(output: &serde_json::Value) -> serde_json::Value {
    json!({"choices":[{"message":{"content":output.to_string()}}]})
}

struct CapturedRequest {
    headers: String,
    body: serde_json::Value,
}

struct ResponseSpec {
    status: u16,
    content_type: &'static str,
    chunks: Vec<Vec<u8>>,
    header_delay: Duration,
    chunk_delay: Duration,
}

impl ResponseSpec {
    fn json(body: &serde_json::Value) -> Self {
        Self {
            status: 200,
            content_type: "application/json",
            chunks: vec![serde_json::to_vec(body).unwrap()],
            header_delay: Duration::ZERO,
            chunk_delay: Duration::ZERO,
        }
    }

    fn delayed_json(body: &serde_json::Value, delay: Duration) -> Self {
        let mut response = Self::json(body);
        response.header_delay = delay;
        response
    }

    fn staged_json(
        body: &serde_json::Value,
        header_delay: Duration,
        chunk_delay: Duration,
    ) -> Self {
        let mut response = Self::json(body);
        response.header_delay = header_delay;
        response.chunk_delay = chunk_delay;
        response
    }

    fn sse(chunks: Vec<Vec<u8>>) -> Self {
        Self {
            status: 200,
            content_type: "text/event-stream",
            chunks,
            header_delay: Duration::ZERO,
            chunk_delay: Duration::from_millis(5),
        }
    }

    fn status(status: u16) -> Self {
        Self {
            status,
            content_type: "application/json",
            chunks: vec![b"{}".to_vec()],
            header_delay: Duration::ZERO,
            chunk_delay: Duration::ZERO,
        }
    }
}

struct TestServer {
    address: std::net::SocketAddr,
    requests: Arc<Mutex<Vec<CapturedRequest>>>,
    handle: thread::JoinHandle<()>,
}

impl TestServer {
    fn start(responses: Vec<ResponseSpec>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("绑定 Fixture TCP 端口");
        let address = listener.local_addr().unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&requests);
        let handle = thread::spawn(move || {
            let mut responses = VecDeque::from(responses);
            while let Some(response) = responses.pop_front() {
                let (mut stream, _) = listener.accept().expect("接受 Fixture 请求");
                let request = read_request(&mut stream);
                captured.lock().unwrap().push(request);
                if !response.header_delay.is_zero() {
                    thread::sleep(response.header_delay);
                }
                let length: usize = response.chunks.iter().map(Vec::len).sum();
                if write!(
                    stream,
                    "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    response.status, response.reason(), response.content_type, length
                )
                .is_err()
                {
                    continue;
                }
                for chunk in response.chunks {
                    if !response.chunk_delay.is_zero() {
                        thread::sleep(response.chunk_delay);
                    }
                    if stream.write_all(&chunk).is_err() {
                        break;
                    }
                    let _ = stream.flush();
                }
            }
        });
        Self {
            address,
            requests,
            handle,
        }
    }

    fn endpoint(&self) -> String {
        format!("http://{}/v1/chat/completions", self.address)
    }

    fn finish(self) -> Vec<CapturedRequest> {
        self.handle.join().expect("Fixture Server 线程应结束");
        Arc::try_unwrap(self.requests)
            .unwrap_or_else(|_| panic!("Fixture 请求仍被持有"))
            .into_inner()
            .unwrap()
    }
}

impl ResponseSpec {
    const fn reason(&self) -> &'static str {
        match self.status {
            200 => "OK",
            429 => "Too Many Requests",
            500 => "Internal Server Error",
            503 => "Service Unavailable",
            _ => "Fixture Status",
        }
    }
}

fn read_request(stream: &mut std::net::TcpStream) -> CapturedRequest {
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 4096];
    let header_end = loop {
        let read = stream.read(&mut buffer).expect("读取 Fixture 请求");
        assert!(read > 0, "Fixture 请求不得提前结束");
        bytes.extend_from_slice(&buffer[..read]);
        if let Some(position) = find_bytes(&bytes, b"\r\n\r\n") {
            break position + 4;
        }
    };
    let headers = String::from_utf8(bytes[..header_end].to_vec()).unwrap();
    let content_length = headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().unwrap())
        })
        .unwrap_or(0);
    while bytes.len() - header_end < content_length {
        let read = stream.read(&mut buffer).expect("读取 Fixture Body");
        assert!(read > 0, "Fixture Body 不得提前结束");
        bytes.extend_from_slice(&buffer[..read]);
    }
    CapturedRequest {
        headers,
        body: serde_json::from_slice(&bytes[header_end..header_end + content_length]).unwrap(),
    }
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}
