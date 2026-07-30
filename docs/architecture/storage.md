# M1 存储架构

本文以 M1 本地可信存储为基础，并同步记录截至 M3 的 Schema 与恢复边界。Schema 中的表或字段不代表对应上层业务调用链已经接通；当前嵌入式门面已公开 Observe、Fragment、Health、Recall、Explain、Stats、Mutation、Quarantine 审查和 Snapshot/Restore/Rebuild。

## 1. 权威边界

以下记录是权威来源：

- `CognitiveFragment`：原始内容、Scope、来源、时间、Validity、安全标签、EvidenceRef 和元数据；
- `MemoryEvent`：完整 Fragment payload、认知阶段、记录时间、内容哈希和精确 Scope 内的 sequence；
- `EvidenceRef`：Fragment 与另一个 Fragment 或 Event 之间的明确证据引用；
- `.jcb` Blob 文件：由明文 SHA-256 标识的原始二进制内容。

`fragment_fts` 是可重建的派生索引。后续的图、Surface、Embedding、摘要和召回状态同样不得覆盖上述权威记录。`scope_sequences` 与 `blob_refs` 是维护事务顺序和 Blob 可达性的存储元数据。

## 2. SQLite 连接与事务

`SqliteStore` 通过 SQLx 打开数据库并设置：

- `foreign_keys = ON`；
- 文件数据库使用 WAL；
- `synchronous = NORMAL`；
- 默认最多 4 个连接；`:memory:` 强制 1 个连接；
- 默认 Busy Timeout 为 5 秒，嵌入式 `MemoryConfig` 可覆盖。

Observe 只对 SQLite Busy/Locked 进行最多 3 次尝试，并为每次重试加入由 EventId 派生的确定性短 jitter。身份冲突、校验失败、约束失败和损坏数据不重试。

## 3. 表、主键与关键外键

### 3.1 M1 已使用的表

| 表 | 主键/唯一约束 | 关键外键与职责 |
| --- | --- | --- |
| `fragment_contents` | `id`；`(sha256, media_type)` 唯一 | 保存 inline text、inline JSON 或 Blob 描述；内容表示不兼容时拒绝复用 |
| `fragments` | `id` | `content_id -> fragment_contents.id RESTRICT`；保存权威 Fragment 字段 |
| `events` | `id`；`(scope_key, sequence)` 唯一 | `fragment_id -> fragments.id RESTRICT`；保存权威 Event envelope 与完整 payload |
| `scope_sequences` | `scope_key` | 为规范化精确 Scope 原子分配下一 sequence |
| `evidence_refs` | `id` | `owner_fragment_id -> fragments.id CASCADE`；被引用 Fragment/Event 使用 `RESTRICT`；两种目标必须且只能存在一个 |
| `blob_refs` | `(blob_id, owner_type, owner_id)` | 记录权威行对 Blob 的正引用计数，供审计和 GC 判断可达性 |
| `fragment_fts` | FTS5 虚表，无关系主键 | 保存 `fragment_id`、`scope_key` 和可搜索 body；完全派生，可从 inline Fragment 内容重建 |

### 3.2 后续里程碑的 Schema 预留

这些表由初始 Migration 创建，但 M1 没有声称已经实现相应认知、召回、Runtime 或服务能力。

| 领域 | 表 | 主键与主要关系 |
| --- | --- | --- |
| 模型版本 | `model_versions`、`memory_models` | 各自以 `id` 为主键，并对提供方/名称/版本或 kind/version 建唯一约束 |
| 点线面 | `points`、`edges`、`surfaces`、`surface_members` | Point 引用来源 Fragment；Edge 引用首尾 Point、来源 Fragment 和可选 ModelVersion；Surface 引用可选 ModelVersion；成员表以 `(surface_id, point_id, version)` 为主键 |
| 实体 | `entities`、`entity_aliases` | Entity 引用来源 Fragment；Alias 以 `(entity_id, alias)` 为主键并引用 Entity 和来源 Fragment |
| 状态 | `states`、`state_versions` | State 指向当前版本；版本引用 State、来源 Fragment 和可选前序版本 |
| 召回 | `recall_sessions`、`recall_activations` | Session 以 `id` 为主键；Activation 以 `(recall_session_id, point_id, depth)` 为主键并引用 Session、Point 和可选 Edge |
| Runtime | `transitions`、`hypotheses`、`capabilities`、`approvals`、`decisions`、`actions`、`outcomes`、`capability_runs` | Transition 是流程根；其余表分别引用 Transition、Hypothesis、Approval、Capability、Action、来源 Fragment 等关系 |
| 治理与恢复 | `audit_logs`、`snapshots`、`quarantine`、`consolidation_jobs` | 各自以 `id` 为主键；Quarantine 引用 Fragment；其余保存 Scope 下的审计、快照与任务元数据 |

## 4. Scope 与索引

`MemoryScope` 至少包含 `namespace_id`，并可包含 `tenant_id`、`user_id`、`agent_id` 和 `project_id`。存储层对规范化 JSON 递归排序后计算 SHA-256，得到 64 字符 `scope_key`。不同 Tenant 即使属于同一 Namespace，也会得到不同 sequence 空间。

M1 活跃索引包括 Fragment 的 Scope/occurred time、Scope/observed time、kind/validity，Event 的 Scope/sequence 与 fragment_id，以及 Evidence 的 owner/referenced target、Blob 引用 owner/digest。Schema 还为后续能力建立以下以 Scope 为首列的索引：

- `idx_points_scope_validity`、`idx_edges_scope_model`、`idx_surfaces_scope_validity`；
- `idx_entities_scope_name`；
- `idx_recall_sessions_scope_started`、`idx_recall_sessions_query`；
- `idx_transitions_trace`、`idx_capabilities_scope_kind_version`、`idx_approvals_scope_status`；
- `idx_audit_logs_scope_created`、`idx_snapshots_scope_created`、`idx_quarantine_scope_status`、`idx_consolidation_jobs_scope_status`。

这些索引提供物理过滤路径，但不替代后续服务层的 AccessContext 与权限判断。

## 5. Observe 事务顺序

1. 在事务外校验 Fragment；禁止空内容、非法时间/置信度、无 Evidence 的 Verified 记录，以及 Verified Prediction。
2. 构造 EventDraft：复制 Scope 和完整 payload，计算规范化内容 SHA-256，并确定认知阶段。
3. 计算 `scope_key`，以 `BEGIN IMMEDIATE` 开始写事务。
4. 按 EventId 和 FragmentId 检查幂等；相同 ID 的不同内容返回 Conflict。
5. 对 `scope_sequences` 执行原子 UPSERT 并取得本次 sequence。
6. 写入或校验 `fragment_contents`，随后写入 `fragments`。
7. Blob 内容存在时写入 `blob_refs`。
8. 写入 `events` 和全部 `evidence_refs`。
9. Text/JSON 内容写入 `fragment_fts`；Blob 内容不进入 FTS。
10. 测试 Failpoint 位于全部行写完、提交之前；任何错误显式回滚。
11. 提交成功后返回 EventId、FragmentId、sequence、content hash 和幂等状态。

因此 Fragment、Event、EvidenceRef、BlobRef、sequence 和 FTS 行不会出现数据库内半写。Blob 文件发布先于数据库事务时，数据库回滚可能留下孤儿文件，由审计报告并按显式 GC 策略处理。

## 6. Blob `JCB1` 格式

BlobId 永远是原始明文的 lowercase SHA-256，与压缩和加密选项无关。文件路径使用两级分片：`blobs/<hash[0..2]>/<hash[2..4]>/<hash>.jcb`。

`JCB1` envelope 按以下顺序编码：

| 字段 | 长度 | 说明 |
| --- | ---: | --- |
| Magic | 4 bytes | ASCII `JCB1` |
| Flags | 1 byte | bit 0 为 Zstd，bit 1 为 AES-256-GCM |
| Nonce length | 1 byte | 未加密为 0；AES-GCM 为 12 |
| Original length | 8 bytes | 明文长度，big-endian `u64` |
| Nonce | 0/12 bytes | 每次加密写入使用安全随机 nonce |
| Payload | 可变 | 先可选 Zstd，再可选 AES-256-GCM |
| Checksum | 32 bytes | 明文 SHA-256 |

固定头和 nonce 作为 AES-GCM AAD。读取时依次验证 header、flags、大小限制、认证解密、解压、明文长度、footer checksum 和路径期望的 BlobId。密钥由调用方提供并在内存中使用 `Zeroizing` 包装，不写入 Blob 文件或 Debug 输出。

发布使用同目录临时文件、文件同步和 `persist_noclobber`；Unix 平台还同步父目录。并发发布相同内容时只会留下一个已验证文件。

## 7. 审计、恢复与 GC

| 场景 | 当前行为 | 权威数据结果 |
| --- | --- | --- |
| Observe 在 commit 前失败 | 显式 rollback | Fragment、Event、EvidenceRef、BlobRef、sequence 和 FTS 均不可见 |
| Blob 已发布但数据库回滚 | 审计列入 `orphan_blobs` | Blob 保留，默认不删除 |
| 引用的 Blob 缺失或损坏 | `audit` 报告 missing/corrupt；读取校验失败 | 不伪造或返回损坏内容 |
| FTS 内容不一致 | `audit` 报告 mismatch | 权威 Fragment/Event 不变 |
| FTS rebuild 在 swap 前中断 | 同一事务回滚临时重建 | 旧 FTS 继续可用 |
| SQLite Busy/Locked | Busy Timeout 后有限重试 | 不转化为冲突或静默丢写 |
| Migration 失败 | SQLx Migration 返回失败并关闭池 | 不把失败版本记录为成功 |

GC 的默认策略是 dry run。实际删除必须显式使用 `GcPolicy::delete_after`，并同时满足：Blob 没有正引用、文件年龄超过 grace period、当前进程取得 `blobs/.gc.lock` 排他锁。删除前文件先移动到 `blobs/.trash/`；审计枚举忽略该目录。GC 不删除 Fragment、Event 或 EvidenceRef。

## 8. Migration 策略

- Migration 以递增编号嵌入二进制，Engine 打开数据库时自动运行；当前 Schema version 为 3。
- `0001_initial.sql` 建立权威存储、认知/Runtime 预留表、FTS 与基础治理表；`0002_consolidation_derived.sql` 增加可版本化 Surface、巩固产物和冷层索引；`0003_governance.sql` 增加目标摘要、请求结果、治理凭证和可恢复 Retention 计划。
- `_sqlx_migrations` 是已应用版本的判定来源；重复打开同一路径必须幂等。
- 已发布 Migration 不原地修改；后续变更添加新版本，并为升级和失败回滚编写测试。
- 破坏性迁移必须先定义备份、校验和回滚路径，不能依赖 FTS 或其他派生表作为恢复来源。
- 权威字段迁移后必须重新验证内容哈希、外键和完整性；派生索引可以在提交后重建。

## 9. 本地数据目录

```text
<data_dir>/
├── memory.sqlite3
├── memory.sqlite3-wal       # 数据库打开且 WAL 活跃时可能存在
├── memory.sqlite3-shm       # 数据库打开且 WAL 活跃时可能存在
├── .governance.lock         # Hard Delete 排他治理时按需创建
├── .governance-trash/       # Hard Delete Blob 暂存区
├── .maintenance/            # 阶段化 Rebuild Checkpoint
└── blobs/
    ├── .gc.lock
    ├── .trash/
    └── ab/cd/<64-hex>.jcb
```

治理锁、暂存目录、Rebuild Checkpoint、`.gc.lock` 和 `.trash/` 均按需创建。调用方负责保护整个数据目录的文件系统权限、备份和磁盘容量。

## 10. M1 验证方法与边界

提交前使用以下命令验证：

```bash
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps
git diff --check
```

当前验证主机为 macOS 26.6（Apple Silicon，`aarch64-apple-darwin`），Rust `1.97.1`。系统 `sqlite3` 为 3.51.0，独立 FTS5 探针成功；仓库 Migration/FTS 测试进一步验证了本项目调用路径。该记录不是 Linux、Windows、macOS x64 或移动平台的实机验证。

最终测试数量和命令耗时在 Task 11 提交前以同一主机的最新门禁输出为准，并记录在 `CHANGELOG.md` 的 M1 验收条目中。
