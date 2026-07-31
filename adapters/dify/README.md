# Dify Adapter

该 crate 将 Dify 工作流调用映射为疆诚记忆的稳定 Scope，并把 HTTP/MCP 节点的结构化输出接入统一 `MemoryPacket`。适配器只负责协议和来源桥接，Recall 始终经由注入的 Core `CommandBus` 执行。

## Scope 映射

- 配置中的 `namespace_id` 作为固定 Namespace。
- `user_id`、`conversation_id`、`app_id`、`workflow_id` 和 `trace_id` 通过带来源版本和字段类型的 SHA-256 稳定映射为 typed UUID。
- `DifyExternalIds` 同时保留所有原始 Dify 字符串，便于审计和重建来源。
- Dify 的 app 映射为 Project，workflow 映射为 Agent，conversation 映射为 Session。

## 工作流节点边界

HTTP 和 MCP 成功节点必须直接输出可反序列化的 `MemoryPacket`。失败节点、缺少输出、不支持的节点类型或非 Packet JSON 都 fail closed，不会把错误文本提升为记忆事实。

## 配置与验证

`config.example.yaml` 只包含来源版本、Namespace 和 Recall 硬预算，不包含 API Key。当前测试使用 Dify `v1` Fixture 和真实 Embedded Core；没有连接 Dify 服务，因此不应描述为 Dify 实机验证。
