# ADR 0002：以 SQLite 作为本地权威存储

- 状态：Accepted
- 日期：2026-07-29

## Context（背景）

M1 需要在单机嵌入式场景中可靠保存 Fragment、不可变 Event、EvidenceRef、Blob 引用和按精确 Scope 分配的 sequence，同时支持事务回滚、并发写入、全文检索、Migration 和损坏检查。

自行实现 B-Tree、WAL、锁管理、崩溃恢复和查询执行器会把项目重心从记忆语义转移到数据库内核，并引入难以证明的持久化风险。单独引入搜索服务又会破坏本地部署的简单性。

## Decision（决策）

M1 使用 SQLite 作为本地权威数据库，并通过 SQLx 管理连接和版本化 Migration。每个连接启用 Foreign Keys，文件数据库使用 WAL，`synchronous` 设置为 `NORMAL`，锁等待使用可配置 Busy Timeout。

Observe 使用 `BEGIN IMMEDIATE`，在同一事务内分配 Scope sequence 并写入 Fragment、Event、EvidenceRef、Blob 引用和 FTS 行。仅 Busy/Locked 可以进行有上限、带 jitter 的重试。全文检索使用 SQLite FTS5，但 FTS 是派生数据，必须能够从权威 Fragment 内容重建。

## Consequences（影响）

- 复用成熟的 B-Tree、WAL、事务、约束、Foreign Keys、Migration 和 FTS5，减少自研存储风险。
- 一个本地数据目录即可部署，不需要外部数据库或搜索服务。
- SQLite 仍是单写者模型；高并发写入通过短事务、Busy Timeout 和有界重试治理，而不是宣称获得分布式写扩展能力。
- FTS5 和所需编译选项必须在目标平台验证；当前 macOS 结果不能替代 Linux 或 Windows 验证。
- 未来远程服务可以复用协议和语义，但不能假设 SQLite 文件可由多个网络节点直接共享。

## Rejected Alternatives（否决方案）

- **自研 B-Tree 与 WAL**：开发和验证成本远高于 M1 价值，崩溃一致性风险不可接受。
- **PostgreSQL 优先**：适合服务端并发，但破坏本地嵌入式和离线开箱能力。
- **纯文件或 JSON 日志**：追加简单，但约束、索引、原子多行写入和恢复查询需要重新实现。
- **外部全文检索引擎**：增加部署组件，并制造权威数据与索引之间的双写一致性问题。
