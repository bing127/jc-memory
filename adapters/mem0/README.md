# Mem0 Adapter

该 crate 将 Mem0 Add、Search 和历史记录迁移映射到疆诚记忆 Core。它不调用未声明的远程 API，也不会把 Mem0 的旧评分直接写入权威置信度。

## Add 与历史迁移

- 每条 Mem0 记录映射为 `Fact + Unverified` Fragment；
- `legacy_confidence` 只保存在 metadata，Fragment `confidence` 固定为 `0.0`；
- Mem0 ID、用户/Agent ID、原 metadata、创建时间、更新时间和内容 SHA-256 均被保留；
- 同一历史版本使用确定性 Fragment ID，重复迁移幂等；
- 相同 Mem0 ID 的不同更新时间或内容生成独立 Fragment，旧历史不会被覆盖。

## Search 与导出

`search` 使用配置中的硬预算构造 Core Recall，并原样返回 `MemoryPacket`。每次 Add/历史迁移都会返回 `jc-memory.mem0-id-map.v1` ID mapping manifest；`export_manifest` 和 `parse_manifest` 提供稳定 JSON round-trip。

## 验证边界

当前测试只覆盖 Mem0 `v1` 版本化 Fixture 和真实 `EmbeddedCommandBus`，没有连接 Mem0 服务，不应描述为远程 API 实机验证。
