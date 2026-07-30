# ADR 0004：认知图是带证据的可重建派生结构

- 状态：Accepted
- 日期：2026-07-30

## Context（背景）

M2 需要用 Point、Edge 和 Surface 表达原子认知变化、关系和上下文聚合。但图结构会随分离阈值、关系学习和 Surface 算法演进；若让图覆盖 Fragment/Event，算法升级将破坏证据链。

## Decision（决策）

Point、Edge 和 Surface 都是派生结构。每个 Point 保留 `source_fragments`，每条 Edge 和每个 Surface 版本保留 `EvidenceLink`。所有结构绑定精确 `MemoryScope` 和有效时间。

共同出现只产生 `CoActivated`，不自动产生 `Causes`。Prediction、Inference 和 Plan 不能转换为 Fact Point，也不能直接提升为语义 Fact。

所有变更采用追加式权威记录。Core Mutation 先写入新的 Fragment/Event，再更新派生关系；不得原地修改目标 Fragment/Event。

## Consequences（影响）

- Recall 和 Explain 可以回到权威 Fragment/Event；
- 派生结构可以因算法升级被丢弃和重建；
- 图查询必须同时执行 Scope、有效时间和证据过滤；
- 当前进程内派生图在重启后不会自动恢复，后续持久化与重建任务必须读取权威元数据，而不能把进程快照升级为权威来源。

## Rejected Alternatives（否决方案）

- **把图数据库作为唯一真相源**：关系方便查询，但无法保留原始认知事件的完整权威性。
- **相似即合并**：会把跨项目、跨时间或不同来源的事件错误折叠。
- **共同出现即因果**：把相关性冒充因果，无法满足验证边界。
- **Mutation 直接更新旧 Point/Fragment**：会丢失历史和审计链。
