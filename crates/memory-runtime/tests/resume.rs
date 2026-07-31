//! Runtime 控制、重试、MCP 和完成判断契约测试。

use std::time::Duration;

use async_trait::async_trait;
use jc_memory_runtime::{
    CompletionInput, CompletionJudge, CompletionStatus, ControlError, McpClient, McpError,
    McpTransport, RetryClass, RetryDecision, RetryPolicy, RunControl, ToolIdempotency,
};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

#[test]
fn waiting_user_can_resume_with_input_and_pause_can_cancel() {
    let mut control = RunControl::new();
    control.wait_for_user().unwrap();
    control.provide_user_input("补充上下文").unwrap();
    assert_eq!(control.take_user_input().as_deref(), Some("补充上下文"));
    control.pause().unwrap();
    control.resume().unwrap();
    control.cancel().unwrap();
    assert!(matches!(control.pause(), Err(ControlError::Terminal)));
}

#[test]
fn retry_policy_caps_attempts_and_blocks_non_idempotent_replay() {
    let policy = RetryPolicy::new(2, Duration::from_millis(10), Duration::from_secs(1));
    assert!(matches!(
        policy.decide(RetryClass::Transient, 1, ToolIdempotency::Idempotent),
        RetryDecision::Retry { .. }
    ));
    assert!(matches!(
        policy.decide(RetryClass::Transient, 2, ToolIdempotency::Idempotent),
        RetryDecision::Stop
    ));
    assert_eq!(
        policy.decide(RetryClass::Transient, 1, ToolIdempotency::NonIdempotent),
        RetryDecision::Stop
    );
}

struct TimeoutTransport;

#[async_trait]
impl McpTransport for TimeoutTransport {
    async fn request(&self, _method: &str, _params: Value) -> Result<Value, McpError> {
        tokio::time::sleep(Duration::from_millis(50)).await;
        Ok(json!({"result":{}}))
    }
}

#[tokio::test]
async fn mcp_client_returns_timeout_and_cancellation_distinctly() {
    let client = McpClient::new(TimeoutTransport, Duration::from_millis(5));
    assert!(matches!(client.initialize().await, Err(McpError::Timeout)));
    let token = CancellationToken::new();
    token.cancel();
    assert!(matches!(
        client.tools_list(token).await,
        Err(McpError::Cancelled)
    ));
}

#[test]
fn completion_judge_reports_missing_requirements_before_completed() {
    let judge = CompletionJudge::default();
    let result = judge.judge(CompletionInput {
        goal_met: true,
        pending_approval: false,
        pending_tool: false,
        missing_evidence: true,
        steps: 1,
    });
    assert_eq!(result, CompletionStatus::Incomplete("missing_evidence"));
}
