# Agent Runtime

`jc-memory-runtime` 提供可恢复状态机、模型输出解析、策略审批、工具执行、提交记录、暂停恢复和完成判断等 Rust 组件，并通过 `RuntimeService` 将这些阶段编排为可注入后端的单次运行服务。

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

## RuntimeService 编排

`RuntimeService::run` 固定执行 Observe、Recall、Model、Policy/Approval、Execute、Commit 和 Completed。模型只产生严格解析后的最终响应或工具请求；高风险工具先进入 `AwaitingApproval`，不会执行副作用。每个阶段都写入 Runtime Transition，保存 Scope、步骤、截止时间、Trace 和输入/输出证据引用。

`RuntimeBackend` 负责把 Observe、Recall 和 Commit 映射到具体 Core 或远程服务的权威记录。服务本身不会把 `Prediction`、`Inference`、`Plan` 或 `ToolResult` 自动升级为 Fact。

## 当前边界

Runtime 状态机、持久化、Provider、Capability、Policy、Approval、Tool、MCP Client、Completion 和 `RuntimeService` 均已有 Rust 实现与测试；Embedded Fixture 已验证完整编排。把 `RuntimeBackend` 接入具体远程部署进程，以及 Windows、容器和集群环境的运行验证，仍需要对应平台证据。
