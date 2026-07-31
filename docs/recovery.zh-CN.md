# 故障恢复

疆诚记忆将权威 Fragment、Event 和 EvidenceRef 与可重建派生结构分离。恢复流程不得用 Summary、Embedding、图关系或 Surface 覆盖权威记录。

## SQLite 与事务恢复

SQLite 使用 WAL、外键、Busy Timeout 和版本化 Migration。Observe、Commit 和治理写入通过事务保证原子性；Commit 批次中任一晚期写入失败时整批回滚。

连接池在所有引用释放后关闭。当前 Sidecar 会先停止接收新连接并等待已认证在途请求结束，再让 CommandBus 和 MemoryEngine 随进程退出释放；尚未提供独立的显式数据库关闭 API。

## Snapshot 与 Restore

`MemoryEngine::create_snapshot` 对授权 Scope 创建一致性 SQLite 备份，复制引用 Blob，写入 Manifest 和校验和，并通过暂存目录原子发布。

`MemoryEngine::restore_snapshot` 在切换前校验 Scope、Schema、SQLite 完整性、Blob 和校验和。调用 Restore 前必须关闭目标目录上的所有 MemoryEngine 与 SQLite 连接；切换失败时保留或恢复原目录。

## 派生重建

`rebuild_derived` 按固定顺序执行 FTS、Embedding、Edge Candidate、Surface Member 和 Summary/Cache。每完成一阶段就持久化 Checkpoint；重启后从最后完成阶段继续。当前 FTS 路径真实重建，其余模型相关阶段主要验证本地契约，不能描述为生产模型重算。

## Runtime 恢复

Runtime Snapshot 和 Transition 保存在 SQLite。恢复会校验 Scope、版本、步骤连续性和状态边；损坏历史会被拒绝。Retry Checkpoint 保留尝试次数与下次执行时间，非幂等工具不会自动重放。

## Sidecar 边界

Sidecar 锁文件记录 PID 与启动标识。OS 锁可取得但旧元数据仍存在时，会标记为陈旧元数据恢复；如果其他进程仍持有 OS 锁，则拒绝第二实例。`stop` 等待已认证在途请求后退出。

Sidecar 当前没有独立 checkpoint 文件，也没有在启动时自动执行 Snapshot Restore 或 Runtime Run 恢复。Windows 实机传输、Sidecar checkpoint 和显式数据库关闭完成前，不应把 Sidecar 描述为完整崩溃恢复服务。
