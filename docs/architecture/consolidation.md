# M3 巩固与派生重建架构

M3 的巩固层从权威 Event 流增量生成可丢弃、可重建的 Point、Edge、Surface、FTS 和统计产物。它增加检索结构，不改变事实身份；`Fragment`、`Event` 和 `EvidenceRef` 始终是权威来源。

## 1. 持久化 Job 模型

`JobSpec` 固定 `JobKind`、精确 `MemoryScope`、初始 cursor 和 `ResourceLimits`。`consolidation_jobs` 持久化状态、Checkpoint、失败次数和限制。状态机包含 `Pending`、`Running`、`Paused`、`Retrying`、`Completed`、`Failed` 与 `Cancelled`。

Worker 通过限时 Lease 独占 Job。Lease 过期后，另一个 Worker 可以在同一持久化 Scope 上竞争接管；当前批次一旦取得 `BEGIN IMMEDIATE`，其他写方会被 SQLite 串行化，不能在事务中途替换 `worker_id`。Pause 会释放 Lease，Resume 从最近提交的 cursor 回到 Pending；失败按指数退避进入 Retrying，达到 `max_attempts` 后进入 Failed。Retry 只能恢复尚未耗尽次数的 Retrying/Failed Job。

每个批次使用 `BEGIN IMMEDIATE`。Handler 成功、结果不变量和资源预算都通过后，派生写入与 Checkpoint 在同一事务提交；失败、取消或超时会回滚整个批次，cursor 不推进。Checkpoint 保存 cursor、已提交批次数、处理条目数、累计 CPU 代理时间、IO 次数、Lease 和下次重试时间。

`MemoryEngine` 持有同一 SQLite 上的 `ConsolidationJobStore`，并公开 `submit_consolidation`、`consolidation_job`、`run_consolidation`、`pause_consolidation`、`resume_consolidation`、`retry_consolidation` 和 `cancel_consolidation`。提交按 `JobSpec.scope` 执行 `Operation::Consolidate` 授权；后续查询和生命周期操作使用 Job 持久化的 Scope 再授权，调用方不能自报 Scope。`run_consolidation` 根据持久化 `JobKind` 静态分派到 12 个已有 Handler。该门面当前是 Embedded Core API，不代表远程 CommandBus、HTTP/gRPC 或 Web 已公开对应命令。

## 2. 资源限制

| 限制 | 执行位置 | 超限结果 |
| --- | --- | --- |
| `max_batch_size` | `BatchContext::consume_items` 与 Checkpoint 提交 | 回滚批次，记录失败 |
| `max_cpu_ms` | Runner 以批次 elapsed 毫秒作为保守代理并累计 | 超时或累计超限时回滚 |
| `max_io_ops` | 每个受限读写在 `BatchContext` 内计数 | 超限前拒绝下一次 IO 并回滚 |
| `max_runtime_ms` | 加载持久化限制后的 claim、写锁等待与批次处理 | 超时返回 `BudgetExceeded("wall_clock")` |
| `max_attempts` | 持久化失败计数 | 达到上限后进入终态 Failed |

写锁等待在 claim 前超限时，Job 保持 Pending 且不增加失败次数；claim 后获取下一批写事务超限时，Job 保持 Running，直到 Lease 过期后由原 Worker 或其他 Worker 重新 claim。Runner 不会在无法取得写锁时伪造已持久化失败。

这些限制是嵌入式 Runner 的合作式预算，不是操作系统级 CPU、内存、磁盘或进程配额。M3 没有全局调度器和跨 Job 公平性策略。

## 3. Handler 与保守写入

| `JobKind` | Handler 行为 |
| --- | --- |
| `Replay` | 从可表达为认知节点的权威 Event 幂等生成基础 Point |
| `Deduplication` | 仅从相同内容生成 `same_entity`，或从同 trace/session 生成 `co_activated`；不发明因果 |
| `EdgeReinforcement` / `EdgeWeakening` | 只有显式 `edge_id` 的成功/失败 Event 才以 `+0.05`/`-0.05` 有界调整权重并留存产物 |
| `SurfaceDiscovery` | 从已有 Point 创建单证据初始动态 Surface |
| `SurfaceMerge` / `SurfaceSplit` | 追加源 Surface 终态版本，再创建新后继 Surface；不覆盖旧版本 |
| `SemanticPromotion` | 只有至少两个独立 Event 和来源、且全部为 Verified 非预测类记录时提升语义 Point；程序经验保存样本数、Wilson 区间和全部 Evidence |
| `ConflictDetection` | 从显式矛盾元数据写入保留 Evidence 的冲突产物 |
| `StaleStateDetection` | 从显式 supersession 元数据标记派生陈旧状态，不覆盖状态历史 |
| `ColdTierMovement` | 只给 Superseded/Invalidated Fragment 写派生冷层索引，不移动权威正文 |
| `Rebuild` | 首批清空 Scope 内派生表，随后从权威 Event 增量重建 Point 与 inline FTS |

所有 Handler 实现同一 `ConsolidationTask::process_batch` 契约。`BatchContext` 不公开底层 Transaction，只允许静态白名单表操作，并在写 Point、Edge、Surface、冷层和 FTS 前核对所有引用属于 Checkpoint 的精确 Scope。尝试修改 `fragments`、`events` 或 `evidence_refs` 会被拒绝。

## 4. 经验学习边界

程序经验按 `procedure_key` 累积独立 Event 和来源。只有去重后至少两个 Event、至少两个来源时才生成经验；比较小改和大改方向时，两组各自还必须至少包含两个独立 Event 和来源。

输出包含 `sample_count`、95% Wilson `confidence_interval`、限定于当前样本的中文陈述和原始 `ExperienceSample` Evidence。相同成功率或证据不足时明确写“不能形成规模方向性结论”；即使观察到差异，也只写“更可能成功”，不生成绝对规则或 Verified Fact。

## 5. Snapshot、Restore 与重建

Snapshot 通过 SQLite `VACUUM INTO` 生成一致镜像，校验数据库完整性和 Scope 后复制实际引用 Blob。Manifest 记录版本、Schema version、精确 Scope、SQLite 与 Blob 的 SHA-256/长度及创建时间，目录通过 staging 和 rename 原子发布。

Restore 在目标数据目录同级 staging 中复制 Manifest 文件，重新校验相对路径、摘要、Schema、SQLite、Scope 和 Blob 清单，再把旧目录改名为可恢复 backup 并切换新目录。调用前必须关闭目标目录上的活动 Engine/SQLite 连接。

当前有两个不同层级的重建入口：

- `jc-memory-consolidation::Rebuild` 先删除目标 Scope 的 Artifact、Surface/Member、Edge、Point、冷层索引与 inline FTS，再从 Event 流重建规范 UUID Point 和 inline FTS，并受通用 Job Checkpoint 和资源预算约束；
- `MemoryEngine::rebuild_derived` 固定按 FTS、Embedding、Edge Candidate、Surface Member、Summary/Cache 顺序持久化阶段 Checkpoint。M3 的 FTS 阶段真实重建索引；其余阶段只校验本地表和 Scope 契约，因为外部 Embedding 与 Summary backend 尚未接入。

`Rebuild` 不隐式编排 Surface 和 Semantic 流水线。需要恢复这些派生项时，调用方必须在 Rebuild 成功后显式提交 `SurfaceDiscovery`、`SemanticPromotion` 等 Job。旧 64 字符 Point/Surface ID 在 Recall 中失败关闭；精确 Scope Rebuild 后恢复为协议 UUID，且 Fragment、Event、EvidenceRef 权威哈希不变。

因此当前可以证明权威哈希在 Snapshot/Restore 后一致、FTS/Point 可由权威记录重建、阶段中断可续跑，但不声称已经生成外部向量、模型摘要或完整 Core 启动时自动恢复的进程内图。

## 6. 经验召回接线

`MemoryEngine::recall` 在同一 Recall deadline 内先完成 State 与 Point 激活，再用剩余 `max_nodes` 读取派生项。读取顺序固定为 Surface、Procedure、Constraint/Preference；Surface 按交集权重、置信度和 ID 排序，整个 Surface 与成员共享预算，容量不足时不返回半个版本并标记 `coverage.truncated`。

Surface 只返回有效时间内的最新 Active/Frozen 版本，且必须与激活 Point 相交。Procedural 模式支持完整 `procedure_key` 精确命中；Constraint 和 Preference 必须保留独立 Verified Evidence。`past_failures` 只从已激活的权威 Failure Episode 筛选。所有派生 Evidence 在 Packet 返回前重新注册精确 Scope，并继续经过 OutputGuard。

## 7. 验证映射

- `conservative_learning.rs`：12 种 Handler 契约、保守经验、派生写入、版本化 Surface、跨 Scope 引用拒绝；
- `job_recovery.rs`：Pause/Resume/Retry、持久化 Checkpoint、Lease 接管、取消、并发 claim、事务回滚，以及批量/CPU/IO/墙钟预算；
- `snapshot_restore.rs`：权威哈希、Manifest round-trip、损坏拒绝、Scope 拒绝、原子发布、固定阶段和 Checkpoint 续跑。
- `experience_recall.rs`：完整 Packet、统一预算、同键跨 Tenant 隔离、Core 重启、Snapshot/Restore、旧 ID 失败关闭和显式 Rebuild。
