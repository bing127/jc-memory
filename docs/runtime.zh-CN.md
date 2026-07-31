# Agent Runtime

`jc-memory-runtime` 提供可恢复状态机、模型输出解析、策略审批、工具执行、提交记录、暂停恢复和完成判断等 Rust 组件。当前这些组件可独立调用并有契约测试，但仓库尚未提供一个把所有组件自动编排到底的生产级 Agent 循环服务。

## 状态与持久化

Runtime 状态包括 `Observing`、`Recalling`、`Reasoning`、`AwaitingApproval`、`Executing`、`Committing`、`WaitingUser`、`Paused`、`Completed`、`Failed` 和 `Cancelled`。状态转换使用穷尽匹配，非法跳转返回 `TransitionError::Invalid`。

`RuntimeRunner` 将 Run、Scope、当前状态、递增步骤和版本写入 SQLite；每次转换同时追加 Transition。恢复时会校验 Scope、步骤连续性、状态边和 Snapshot 版本，检测到不连续或损坏记录时拒绝恢复。

`RuntimeCheckpointStore` 保存控制状态、重试次数、`next_attempt_at` 和非幂等工具待确认标记。`RuntimeRecovery` 恢复时不会自动重放非幂等工具，调用方必须作出明确决策。

## 模型、策略和工具

- `ModelProvider` 定义模型调用接口；当前包含 OpenAI Compatible 和配置化 HTTP Provider。
- 模型结果必须经 `parse_model_output` 解析为 `Hypothesis`、`Prediction`、`Plan`、`DecisionCandidate`、`ToolRequest` 或 `FinalResponse`。
- `PolicyEngine` 根据权限、风险和审批要求返回 `Allow`、`Deny` 或 `RequireApproval`。
- `ToolRegistry` 校验版本、权限、输入 Schema、风险与幂等性；`ToolLoop` 记录 Action、ToolResult、Outcome 和 Prediction 关系。
- 工具实际失败只能形成 `DisprovedBy` 等验证关系，不能把 Prediction、Inference 或 Plan 直接写成 Verified Fact。

## 控制与完成判断

`RunControl` 支持 `pause`、`resume`、`cancel`、`wait_for_user` 和 `provide_user_input`。`RetryPolicy` 按错误类别和最大次数给出重试、等待或终止决策。`CompletionJudge` 同时检查目标状态、待审批、待工具、缺失证据和最大步骤数。

## 当前边界

Runtime 状态机、持久化、Provider、Capability、Policy、Approval、Tool、MCP Client 和 Completion 均已有 Rust 实现与测试。当前仍缺将 Observe、Recall、Model、Policy、Approval、Tool、Commit 和恢复串成同一真实运行的完整 E2E，因此不能把这些独立组件描述为完整 Agent 产品闭环。
