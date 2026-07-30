# ADR 0008：治理采用纵深防御与可验证恢复

- 状态：Accepted
- 日期：2026-07-30

## Context（背景）

单次入口授权不足以阻止查询实现错误、派生引用污染或返回阶段混入其他 Scope。Prompt Injection 和 Memory Poisoning 也不能通过删除原文解决，否则系统会失去证据。Snapshot、Retention 与 Hard Delete 同时跨越 SQLite 和文件系统，需要明确失败边界。

## Decision（决策）

访问控制默认拒绝并精确匹配 Scope。Recall 在查询前授权，所有仓储读取以授权 Scope 构造 SQL；Packet 返回前再复核 ACL 与每个 Fragment 来源，任何不变量错误使整个调用失败。

不可信输入通过版本化确定性规则进入 Quarantine。隔离和人工 Release/Reject 均追加权威治理记录，不覆盖原始 Fragment 或 Evidence。模型 Prediction、Inference 和 Plan 保持非事实身份。

Retention 必须先持久化候选计划再执行。Hard Delete 使用排他锁、数据库事务、Blob 暂存、无正文摘要凭证和派生层重建队列；只有数据库与文件清理完成后才标记成功。Snapshot 在隔离目录校验 Manifest、Schema、Scope、SQLite 和 Blob 后才切换当前目录。

## Consequences（影响）

- 单个过滤器或派生索引错误不会直接变成跨 Scope Packet；
- Quarantine 保留攻击样本和审计链，但规则仍可能漏报或误报；
- 治理失败能区分未提交、可恢复清理失败和成功完成；
- Hard Delete 会暂时清空目标 Scope 的派生层并要求后续 Rebuild；
- 本地摘要与校验和不等于签名、KMS、远程身份、备份擦除或分布式副本治理。

## Rejected Alternatives（否决方案）

- **只在 API 入口做一次 ACL**：无法捕获 Packet 组装阶段的跨 Scope 引用。
- **命中注入规则就删除原文**：破坏权威证据并阻止后续复核。
- **Retention 查询后立即删除**：没有稳定候选、摘要和可恢复进度。
- **数据库提交后直接删除 Blob**：文件失败时无法证明治理结果或恢复到一致状态。
- **把派生索引打包成不可校验的权威备份**：会使损坏或旧算法结果覆盖原始证据。
