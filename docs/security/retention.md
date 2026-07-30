# Retention、Export 与 Hard Delete

M3 的治理能力面向本地 SQLite 与文件系统。所有操作绑定精确 Scope，并把可恢复失败与成功结果写成不含正文的审计记录。

## 1. Retention 先计划后执行

`plan_retention` 要求 `Delete` Grant，按 `observed_at` 截止时间选择候选，并以稳定顺序保存 FragmentId 列表、计划 SHA-256 和进度。该步骤不删除数据。

`execute_retention` 在读取或改变计划状态前重新要求调用者精确 Scope 的 `Delete` Grant，再接受数据库中仍为 `planned` 或可恢复 `failed`、摘要与候选完全匹配的计划。每个候选通过独立 Hard Delete 执行；成功后推进 `next_candidate_index`，失败时保存失败目标摘要并停在未完成候选。候选在计划后被替换、计划被篡改、状态非法或重复执行都会被拒绝。

Retention 当前按观察时间和 Fragment 粒度运行，没有法规模板、Legal Hold、租户默认策略或后台调度器。调用方负责何时创建和执行计划。

## 2. Export

`export_scope` 要求 `Export` Grant，并在临时目录生成：

- `manifest.json`：Manifest 版本、精确 Scope、创建时间、权威 JSONL 摘要和 Blob 清单；
- `authority.jsonl`：按稳定顺序导出的 `fragment_contents`、`fragments`、`events` 与 `evidence_refs`；
- 当前 Scope 实际引用的内容寻址 Blob。

发布使用同文件系统 rename。失败时删除 staging 目录并写入失败审计。当前 Export 是本地目录格式，不是远程传输、加密归档、签名包或跨版本导入 API。

## 3. Hard Delete

`hard_delete` 要求 `Delete` Grant，并取得数据目录级 `.governance.lock` 排他锁。目标必须属于调用者精确 Scope。删除在 `BEGIN EXCLUSIVE` 事务和 Blob 暂存流程中协调：

1. 清除该 Scope 的 Recall、Surface、Edge、Point、巩固产物、冷层索引和 FTS 派生行；
2. 清除目标相关的 `EvidenceRef`、Quarantine、Entity、Hypothesis、Outcome 和 State 引用；
3. 删除目标 Event、Fragment、BlobRef，以及不再被 Fragment 使用的内容行；
4. 把失去全部引用的 Blob 移入 `.governance-trash` 暂存；
5. 在同一事务写入待完成的无正文凭证、失败态审计，并排队 `rebuild` Job；
6. 提交数据库后删除暂存 Blob，再把凭证和审计原子更新为成功。

数据库提交前失败会回滚事务并恢复已暂存 Blob。数据库已提交但文件清理失败时保留 `failed` 凭证，不伪造成功记录，后续可据此恢复处理。成功凭证只包含目标标识摘要、`request_id` 和是否已排队重新巩固，不包含正文。

Hard Delete 会清空目标 Scope 的派生层并排队重建，以避免残留引用；这意味着同 Scope 的 Recall 派生结果在重建完成前可能暂时不完整。M3 没有实现分布式副本擦除、操作系统安全擦除、备份过期编排或云对象存储生命周期。

## 4. 密钥边界

`KeyProvider` 返回 `Zeroizing<[u8; 32]>`，并把轮换版本、来源和时间与密钥材料分离。M3 提供的是进程内 `InMemoryKeyProvider` 及接口契约，没有接入系统 Keychain、KMS 或 HSM。调用方不得把进程内测试实现当作生产密钥托管。

`crates/memory-security/tests/hard_delete.rs` 覆盖授权拒绝、无正文审计、Export、Retention 计划恢复、Blob 清理失败、Hard Delete 全引用清理和重建排队。
