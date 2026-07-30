# ADR 0007：巩固必须保守、增量且可恢复

- 状态：Accepted
- 日期：2026-07-30

## Context（背景）

长期记忆需要从不断增长的 Event 流构造 Point、Edge、Surface、语义经验和冷热层索引。若巩固任务直接修改原始记录、无界扫描或在崩溃后从头执行，会破坏证据链并造成不可预测的资源消耗。

## Decision（决策）

所有巩固 Handler 实现统一的 `ConsolidationTask` 批处理契约，并由持久化 `JobRunner` 驱动。Job 使用精确 Scope、Lease、Checkpoint、失败次数、指数退避、取消，以及批量、CPU 代理时间、IO、墙钟和重试上限。

Handler 只能通过不暴露 Transaction 的 `BatchContext` 读取权威 Event 和写入白名单派生表。派生引用必须属于 Checkpoint Scope；Fragment、Event 和 EvidenceRef 不可由巩固修改。批次派生写入与 cursor 推进在同一事务提交，失败时一起回滚。

语义提升要求至少两个独立 Verified Event 和来源，并拒绝 Prediction、Inference、Plan。程序经验保留样本分母、置信区间和 Evidence，文案不得表达绝对规则。共同出现最多生成 `CoActivated`，不能自动推导因果。

## Consequences（影响）

- Job 可以暂停、恢复、重试和接管过期 Lease，不重复提交已完成批次；
- 资源超限会显式失败，不以无界扫描换取完整度；
- Handler 的幂等标识与追加式版本允许重放；
- CPU 预算目前以批次 elapsed 为保守代理，不是系统级配额；
- `Operation::Consolidate` 的 Core 授权入口尚未接通，直接使用 crate 的调用方必须在外层授权。

## Rejected Alternatives（否决方案）

- **定期全库无界重算**：实现简单，但无法控制延迟、IO 和故障恢复成本。
- **Handler 直接获得 SQL Transaction**：灵活但允许绕过权威表和 Scope 边界。
- **成功一次就形成规则**：会把偶然 Outcome 写成事实，缺少样本分母和独立来源。
- **覆盖旧 Surface 或状态**：丢失历史版本和解释路径。
