# Capability、Model 与 Tool

`jc-memory-runtime` 使用版本化 Capability 描述 Prompt、Skill、Tool、MCP Tool、Workflow 和 Policy。Capability 是执行能力与权限边界，不是权威事实来源。

## Capability 契约

`CapabilityDescriptor` 包含稳定 ID、语义版本、类型、来源、权限集合、输入/输出 JSON Schema、风险级别、是否需要审批和可用 Surface。注册时同时保存 `CapabilityExecutor`；解析时按 ID、版本要求、权限和风险约束选择匹配实现。

调用方不能仅凭名称取得执行器。`CapabilityRegistry::resolve` 必须接收 `CapabilityRequest` 和 `AccessContext`，权限不足或版本不匹配时返回结构化错误。

## 执行统计

`CapabilityStatsStore` 只记录运行数、成功数、失败数、失败模式和 Evidence。排序使用最小样本数与贝叶斯平滑，样本不足时不会把偶然成功解释为稳定能力。

统计是程序记忆和调度证据，不会生成 Fact，也不能覆盖原始 Fragment、Event 或 EvidenceRef。

## Tool 边界

`ToolDescriptor` 声明输入/输出 Schema、权限、风险、审批要求和 `Idempotent` 或 `NonIdempotent`。`ToolRegistry` 在执行前校验请求；高风险或声明需审批的工具必须经过 Approval。

非幂等工具在进程恢复后不会自动重放。工具返回值先保存为 ToolResult 和 Outcome，再由验证关系连接到 Prediction；失败结果可否定 Prediction，但不会改写原始模型输出。

## Model Provider 边界

OpenAI Compatible Provider 支持非流式与 SSE、Bearer 或自定义 Header、超时和有界 429/5xx 重试。Generic HTTP Provider 使用配置化 JSON Pointer 提取响应。API Key 只应通过运行环境注入，不应写入配置样例、日志或错误正文。

当前没有统一的远程 Capability 管理 API，也没有完成完整 Agent 循环 E2E。生产系统在接入自定义 Capability、Model 或 Tool 时仍需自行提供配置生命周期、Secret 管理和端到端验收。
