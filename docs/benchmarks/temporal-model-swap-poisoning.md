# Cross-Year、Model Swap 与 Memory Poisoning

## Cross-Year

`benchmarks/cross-year/generator.rs` 通过统一 Harness 的固定 LCG 生成器创建确定性 Case。Manifest 固定 `seed`、`case_count` 和 SHA-256。Smoke 覆盖跨 1 天、1 月、1 年、状态多次变更、模糊线索、同名实体、相似事件和稀有事实标签。相同 Seed 必须得到相同 Digest，不同 Seed 必须得到不同 Digest。

## Model Swap

两个 `DeterministicModel` 可以使用不同回复前缀，但对同一输入必须产生相同的 Fragment、Evidence、CurrentState canonical hash。比较器只比较结构化 hash，不比较最终语言表达；因此语言模型措辞变化不会被错误地当作记忆结构变化。

## Memory Poisoning

仓库内 `PRIVATE-FIXTURE` 覆盖提示注入、跨租户访问、未验证推断、重复错误、恶意 ToolResult 和权限绕过。Runner 依据显式场景类型生成 quarantine/deny 预期，未知类型也采取隔离并拒绝的 fail-closed 结果。该 Fixture 只验证安全契约和数据流，不替代 `memory-core` 的真实数据库端到端安全测试。

当前验证范围是确定性 Smoke Fixture 和 Rust Harness 测试；没有外部数据集、外部模型、其他平台或生产负载验证。
