# Hermes Adapter

该 crate 将 Hermes turn 转换为疆诚记忆的权威 `Fragment` / `Event` 写入，并通过同一个 `CommandBus` 执行有界 Recall。适配器不实现独立记忆语义，也不会把模型候选或工具结果提升为 `Verified Fact`。

## 当前契约

- Fixture 对应 Hermes `0.19.0` 和 `hermes.observer.v1`。
- `sync_turn` 将 Observation、ModelOutput 和 ToolResult 作为独立 Fragment 一次性 Commit。
- `source_id` 与 metadata 同时保留 Hermes 版本、接口版本和 turn ID。
- 所有映射结果初始为 `Unverified`；Prediction 只保留来源自报置信度。
- `prefetch` 返回 Core 生成的原始 `MemoryPacket`，不改写内容或来源。
- `shutdown` 先拒绝新请求，再等待 in-flight 请求完成，最后调用下游 `CommandBus::shutdown`。

## 配置

YAML 示例见 `config.example.yaml`。服务可先反序列化为 `HermesConfig`，再显式提供已经配置好 ACL Grant 的 `AccessContext`：

```rust,no_run
use std::sync::Arc;

use jc_memory_adapter_hermes::{HermesConfig, HermesMemoryProvider};
use jc_memory_core::CommandBus;
use jc_memory_protocol::AccessContext;

fn provider(
    bus: Arc<dyn CommandBus>,
    access: AccessContext,
    yaml: &str,
) -> HermesMemoryProvider {
    let config = HermesConfig::from_yaml(yaml).expect("Hermes 配置应有效");
    HermesMemoryProvider::new(bus, access, config)
}
```

## 验证边界

当前测试使用版本化 Fixture 和 Rust `CommandBus` 契约，不依赖 Hermes Python 包，也不应描述为 Hermes 实机验证。升级 Hermes 或 observer schema 时必须新增 Fixture，并保留旧 Fixture 的兼容测试。
