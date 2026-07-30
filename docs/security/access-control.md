# 访问控制与权限矩阵

本文描述 M3 已实现的本地访问控制边界。它适用于 Rust 嵌入式 `MemoryEngine`、`GovernanceService` 以及 `AclEngine` 的直接调用，不代表 HTTP、gRPC、MCP、远程身份认证或组织级角色系统已经实现。

## 1. 默认拒绝

每次受保护调用都携带 `AccessContext`：

- `principal_id` 标识调用主体；
- `scope` 是主体当前所在的完整 `MemoryScope`；
- `grants` 是显式授权集合；
- `request_id` 用于关联拒绝日志和治理审计。

`Grant` 精确匹配 `operation`、`namespace_id`、`tenant_id` 和 `project_id`。`tenant_id = None` 与 `project_id = None` 只匹配资源对应字段也为 `None` 的情况，不表示通配。没有匹配 Grant 时返回 `MissingGrant`，不会降级为匿名读取。

同一 Namespace 内，普通操作还要求调用者与资源的 `user_id`、`agent_id` 一致。跨 Project 需要目标 Project 的精确操作 Grant；跨 Tenant 只接受目标 Scope 的精确 `Admin` Grant；跨 Namespace 始终拒绝。当前 ACL 没有继承、前缀匹配或通配角色。

## 2. 权限矩阵

| 调用入口 | 所需 `Operation` | 授权资源 | 返回前复核 |
| --- | --- | --- | --- |
| `MemoryEngine::observe` | `Observe` | 待写 Fragment 的精确 Scope | 不适用 |
| `MemoryEngine::fragment`、`stats` | `Recall` | 调用或查询的精确 Scope | 由 Scope 绑定查询约束 |
| `MemoryEngine::recall` | `Recall` | `RecallQuery.scope` | `OutputGuard` 复核 Packet 全部 Fragment 来源 |
| `MemoryEngine::explain` | `Explain` | `RecallQuery.scope` | 与 Recall 使用同一 Packet 复核路径 |
| `correct`、`reinforce`、`weaken`、`supersede`、`contradict`、`forget` | `Mutate` | 调用上下文的精确 Scope | 目标通过 Scope 绑定仓储读取 |
| `MemoryEngine::health` | `Admin` | 调用上下文的精确 Scope | 不适用 |
| `review_quarantine` | `Admin` | 调用上下文的精确 Scope | 目标必须处于同 Scope 的 Quarantine |
| `create_snapshot` | `Export` | 调用上下文的精确 Scope | Snapshot 再校验数据库仅含该 Scope |
| `restore_snapshot`、`rebuild_derived` | `Admin` | 调用上下文的精确 Scope | Restore 校验 Manifest 与数据库 Scope |
| `GovernanceService::export_scope` | `Export` | 调用上下文的精确 Scope | SQL 与 Blob 清单均从 `scope_key` 构造 |
| `plan_retention`、`execute_retention`、`hard_delete` | `Delete` | 调用上下文的精确 Scope | 执行计划在读取或改变状态前重新授权；删除目标必须在该 Scope 内 |

`Operation::Consolidate` 已是稳定协议字段，但 M3 没有把 `ConsolidationJobStore` 和 `JobRunner` 暴露为受 `MemoryEngine` 保护的提交入口。直接使用 `jc-memory-consolidation` 的嵌入式调用方必须在外层完成身份认证和 `Consolidate` 授权；不能把协议枚举的存在描述为 Core 已接通权限门面。

## 3. Recall 双重授权

Recall 在读取候选前先调用 `AclEngine`。授权通过后，FTS、近期候选、按 ID 读取和 Quarantine 状态查询都使用由 `RecallQuery.scope` 计算的精确 `scope_key`，不会先加载全局候选再做内存过滤。

Packet 构造完成后，`OutputGuard::filter_and_assert` 再执行一次 ACL，并遍历状态、Surface、Episode、Point、Procedure、知识、冲突和顶层 Evidence 引用。任何来源 Fragment 未登记或 Scope 不一致都会使整个调用返回 `SecurityInvariantViolation`；不会返回经过删减的部分 Packet。

## 4. 审计边界

Core 的预授权和返回前拒绝通过结构化 `tracing` 记录 `principal_id`、`request_id`、操作、散列后的 Scope 标识或 FragmentId、拒绝原因和结果。治理操作另外写入 `audit_logs`，保存主体、操作、目标摘要、请求标识、结果和时间。

治理审计与 Hard Delete 凭证不保存目标正文或密钥材料。调用方仍需保护日志输出、SQLite 文件和数据目录权限；M3 未实现远程日志传输、日志签名或外部审计平台集成。

## 5. 已验证边界

`crates/memory-security/tests/acl.rs` 覆盖默认拒绝、精确 Scope、跨 Project、跨 Tenant、跨 Namespace 和 User 边界。`crates/memory-core/tests/tenant_isolation.rs` 证明跨 Tenant 请求在查询前被拒绝，并通过破坏另一 Tenant 行的 Fixture 验证本 Tenant Recall 没有读取或返回该行。
