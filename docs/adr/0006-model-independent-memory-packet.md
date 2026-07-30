# ADR 0006：Memory Packet 保持模型无关

- 状态：Accepted
- 日期：2026-07-30

## Context（背景）

疆诚记忆需要被 Embedded API、未来服务和多种模型共同使用。若 Recall 响应直接采用某个供应商的消息格式或提示词模板，协议会与模型耦合，证据字段也容易被自然语言上下文掩盖。

## Decision（决策）

Recall 返回固定结构的 `MemoryPacket`。Packet 明确区分状态、Surface、Episode、Point、路径、程序经验、语义知识、冲突、证据缺口、Coverage 和置信度，并在所有派生 View 中保留来源标识。

`recommended_context` 只是从其他字段确定性序列化的 JSON 数据，不包含 system prompt、角色标记或供应商专用 token。模型适配器可以在 Packet 之外决定如何组织消息，但不得反向修改 Packet 的事实身份或证据链。

## Consequences（影响）

- Embedded、远程服务和未来 SDK 可以共享同一 DTO；
- 模型替换不改变权威证据或协议字段；
- 调用方可以选择结构化消费或使用确定性上下文；
- 协议字段已定义不等于 Core 已填充全部领域数据，未接入字段保持空数组并由文档说明边界。

## Rejected Alternatives（否决方案）

- **直接返回供应商 Chat Message**：把内核绑定到模型角色和提示词格式。
- **只返回一段摘要文本**：无法机器检查证据、冲突、预算和来源。
- **由模型生成 `recommended_context`**：结果不确定且可能改变事实身份。
- **省略空字段**：会增加跨语言兼容和版本判断成本。
