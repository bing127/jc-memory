# Core Consolidation 与经验召回设计

## 1. 背景

M3 已实现十二类 `ConsolidationTask`、持久化 `ConsolidationJobStore`、带 Lease 和 Checkpoint 的 `JobRunner`，并能向 SQLite 的 Point、Edge、Surface 和 `consolidation_artifacts` 写入可重建派生产物。Core 可信记忆闭环又补齐了 State、Graph Projector、Mutation 重启语义和统一 `MemoryPacket`。

当前仍有两个产品断点：

1. `MemoryEngine` 没有 Consolidation Job 门面，应用只能直接依赖底层 crate，无法在 Core 边界统一执行 `Operation::Consolidate` 授权；
2. Core Recall 仍把 `activated_surfaces`、`procedures`、`constraints`、`preferences` 和 `past_failures` 留空，M3 形成的派生产物没有进入统一 Memory Packet。

本阶段关闭这两个断点，但不扩展到 Server、Web、Runtime Tool 或后台自动调度。Fragment、Event 和 EvidenceRef 继续作为唯一权威记录；Job、Point、Edge、Surface、Artifact 和召回 View 都是可重建派生层。

## 2. 目标

- 在 `MemoryEngine` 提供授权后的 Consolidation Job 提交、查询、运行和生命周期控制；
- 复用现有十二类 Handler，不在 Core 重写巩固算法或 Job 状态机；
- 从 SQLite 按精确 Scope 和 Recall 相关集合加载 Surface、程序经验、约束、偏好和失败记录；
- 让这些派生内容进入模型无关 `MemoryPacket`，并保留权威 Evidence ID；
- 让所有新增 Recall 数据共享原有 deadline、`max_nodes`、时间过滤和双重 Scope 校验；
- 证明 Job 与经验召回在 Core 重启后仍可恢复，两个 Tenant 不发生交叉读取。

## 3. 非目标

本阶段不实现：

- REST、gRPC、Sidecar、MCP、BFF 或 Web 的 Consolidation 远程接口；
- 后台定时器、Cron、常驻 Worker 或自动 Job 编排；
- Runtime Tool 自动采集 `procedure_key`、Approval 续跑或 Tool Commit；
- 组织级多 API Key、远程 Tenant 管理或治理控制台；
- LLM、Embedding 或启发式自然语言知识抽取；
- 外部数据库、向量数据库或消息队列；
- 把 Prediction、Inference、Plan 或统计经验提升为 Verified Fact。

上述远程和 Runtime 能力进入后续“Runtime Tool、Tenant 与远程治理”子项目。

## 4. Core Job 门面

`MemoryEngine` 在打开 SQLite 时同时构造 `ConsolidationJobStore`。Core 新增独立 `consolidation.rs`，公开以下进程内 API：

```rust
pub async fn submit_consolidation(
    &self,
    context: &AccessContext,
    spec: JobSpec,
) -> Result<JobRecord, MemoryError>;

pub async fn consolidation_job(
    &self,
    context: &AccessContext,
    id: JobId,
) -> Result<JobRecord, MemoryError>;

pub async fn run_consolidation(
    &self,
    context: &AccessContext,
    id: JobId,
) -> Result<JobRecord, MemoryError>;

pub async fn pause_consolidation(
    &self,
    context: &AccessContext,
    id: JobId,
) -> Result<JobRecord, MemoryError>;

pub async fn resume_consolidation(
    &self,
    context: &AccessContext,
    id: JobId,
) -> Result<JobRecord, MemoryError>;

pub async fn retry_consolidation(
    &self,
    context: &AccessContext,
    id: JobId,
) -> Result<JobRecord, MemoryError>;

pub async fn cancel_consolidation(
    &self,
    context: &AccessContext,
    id: JobId,
) -> Result<JobRecord, MemoryError>;
```

提交时先对 `spec.scope` 执行 `Operation::Consolidate` 授权。其余操作必须先按 Job ID 加载持久化 `JobRecord`，再对记录中的 `spec.scope` 授权；API 不接受调用方另传 Scope，避免用伪造 Scope 控制其他 Tenant 的 Job。

Core 依据 `record.spec.kind` 分派到已有十二类 Handler。Worker ID 使用进程内随机标识，不包含 Principal、Namespace、Tenant、API Key 或正文。Lease 和重试时间继续由 `JobRunner` 与 SQLite 状态机管理。Core 不自动提交下一类 Job，也不隐式执行完整巩固流水线。

`cancel` 从 consolidation crate 的内部能力提升为受文档约束的公共 Store 操作，但外部产品代码仍应通过 Core 门面调用。取消只改变 Job 状态和释放 Lease，不删除已经提交的完整批次，也不回滚权威记录。

## 5. Handler 分派

Core 固定映射：

| `JobKind` | Handler |
| --- | --- |
| `Replay` | `Replay` |
| `Deduplication` | `Deduplication` |
| `EdgeReinforcement` | `EdgeReinforcement` |
| `EdgeWeakening` | `EdgeWeakening` |
| `SurfaceDiscovery` | `SurfaceDiscovery` |
| `SurfaceMerge` | `SurfaceMerge` |
| `SurfaceSplit` | `SurfaceSplit` |
| `SemanticPromotion` | `SemanticPromotion` |
| `ConflictDetection` | `ConflictDetection` |
| `StaleStateDetection` | `StaleStateDetection` |
| `ColdTierMovement` | `ColdTierMovement` |
| `Rebuild` | `Rebuild` |

分派只选择 Handler，不改变 `JobSpec`、资源预算或 Checkpoint。每次 `run_consolidation` 只运行一个既有 Job 到其完成、暂停、取消、预算错误或可重试失败状态。

## 6. 派生召回读取模型

`memory-storage` 在 `SqliteProjectionStore` 增加一个受限读取入口。输入包括精确 `MemoryScope`、激活 Point ID、激活 Fragment ID、查询有效时间和剩余节点预算；输出为强类型 `ProjectedRecallDerivations`：

```rust
pub struct ProjectedRecallDerivations {
    pub surfaces: Vec<SurfaceView>,
    pub procedures: Vec<ProcedureView>,
    pub constraints: Vec<KnowledgeView>,
    pub preferences: Vec<KnowledgeView>,
    pub past_failures: Vec<EpisodeView>,
    pub evidence: Vec<EvidenceView>,
    pub truncated: bool,
}
```

读取使用参数绑定和 `scope_key` 首列过滤。任何 Surface 成员、Point、Fragment、Event 或 Artifact Evidence 无法证明属于同一精确 Scope 时返回结构化错误，不能跳过损坏行后继续返回部分结果。

### 6.1 规范强类型 ID

旧 M3 Handler 的内部 `stable_id` 返回 64 字符 SHA-256 文本，而当前 `PointId`、`EdgeId` 和 `SurfaceId` 的协议值是 UUID。两种格式不能直接接线，否则 Storage 读取会把旧派生行判定为损坏。

本阶段统一以下规则：

- 基础 Point ID 复用来源 `FragmentId` 的 UUID 字符串，与 Graph Projector 和 Core 即时 Point 保持一致；
- Mutation Edge 可复用来源 `EventId`，其他派生 Edge 和 Surface 使用带 RFC 4122 version/variant 位的确定性 UUID；
- Procedure 和 Semantic Artifact 的 `id` 仍是 SQLite 内部主键，不暴露为协议强类型 ID，可以继续使用稳定 SHA-256；
- Artifact 中的 `point_id`、Evidence Event/Fragment ID 必须能解析为对应协议强类型；
- 旧的非 UUID Point、Edge 和 Surface 行不原地改写为新身份；通过精确 Scope 的 `Rebuild` 删除后，按权威 Event 重新生成规范派生行；
- Rebuild 前后的权威 Fragment、Event、EvidenceRef 数量与哈希必须一致。

迁移测试先构造旧 64 字符派生 ID，证明 Recall 失败关闭；随后运行 Rebuild，证明新行可以解析、Packet 可召回且权威哈希未变化。

## 7. 相关性规则

### 7.1 Surface

只读取查询有效时间内处于 `active` 或 `frozen` 状态的最新 Surface 版本。Surface 至少有一个成员 Point 出现在本次激活 Point 集合中才相关。返回完整当前版本成员，但成员总量计入剩余 `max_nodes`；成员越界时不返回半个 Surface，而是停止后续 Surface 并标记 `truncated`。

Surface Evidence 必须解析为权威 Fragment/Event 标识并通过精确 Scope 校验。`merged`、`split`、`inactive` 或时间上不可见的版本不得进入 Packet。

### 7.2 Procedure

程序经验来源于 `procedure_experience` Artifact。Artifact 必须至少保留：

- `procedure_key`；
- 独立 `sample_count`；
- `success_count`、`failure_count` 和 `success_rate`；
- Wilson 区间和保守陈述；
- 每个样本的 Event、Fragment、来源和实际结果。

只有其 Evidence Fragment 与本次激活 Fragment 集合相交，或 `RecallMode::Procedural` 且 `query.text` 精确匹配 `procedure_key` 时才返回。`ProcedureView.success_rate` 必须从持久化计数重新校验，不能信任无法对应 Evidence 的单独浮点值。

### 7.3 Constraint 与 Preference

`SemanticPromotion` 为稳定 Constraint/Preference Artifact 保存结构化原始值、Point ID、证据派生置信度、Validity 和独立 Evidence。只有满足以下全部条件才进入 Packet：

- Artifact 为当前 Scope；
- Point 在本次激活集合中；
- 至少两个独立 Event 和两个独立来源；
- 所有支持记录都不是 Prediction、Inference 或 Plan；
- Validity 不是 Quarantined、Invalidated 或 Superseded。

Constraint 填入 `constraints`，Preference 填入 `preferences`。本阶段不新增 `facts` 字段；稳定 Fact 继续通过 Point、Episode 和 Evidence 表达，避免修改跨语言协议。

### 7.4 PastFailure

`past_failures` 只从本次已激活、Kind 为 `Failure` 的权威 Episode 中筛选，不进行 Scope 全表扫描，也不复制不相关历史失败。

## 8. Recall 预算与顺序

Core Recall 顺序调整为：

1. 授权并建立统一 deadline；
2. 追平 State 和 Graph Projector；
3. FTS、Graph 和有界激活；
4. 读取相关 State；
5. 使用剩余 `max_nodes` 和剩余 deadline 读取 Consolidation 派生项；
6. 构造完整 Packet；
7. 汇总并去重 Evidence；
8. 返回前执行 Scope/Operation 校验；
9. 确定性重建 `recommended_context`。

节点预算按 State、激活 Point、Surface、Procedure、Constraint、Preference 和 PastFailure 的返回项依次扣减。Evidence 不重复扣节点预算，但继续计入 `coverage.evidence_count`。任何阶段用尽预算时停止加载后续类别并设置 `coverage.truncated = true`。数据库读取也必须位于同一个 Recall deadline 内。

排序保持确定性：Surface 按激活交集权重、置信度、Surface ID；Procedure 按精确 key 命中、样本数、key；Constraint/Preference 按激活 Point 顺序和 Point ID；失败按发生时间倒序和 Event ID。

## 9. Evidence 与权威边界

所有 Artifact 只保存权威 ID 和可重建统计，不保存替代权威正文的摘要。View 中的 `EvidenceView` 从 Artifact 记录的 Event/Fragment ID 构造，并通过权威表验证：

- Event 必须真实存在并指向对应 Fragment；
- Fragment 必须属于请求 Scope；
- Artifact 的来源游标不能超出 Scope 当前 Event sequence；
- 重复 Evidence 按 `(fragment_id, event_id, evidence_ref_id)` 去重；
- Artifact 数据不得修改 Fragment 的 Kind、Validity、正文或 EvidenceRef。

Prediction、Inference 和 Plan 可以作为可见 Episode，但不能直接支持稳定 Constraint、Preference 或 Verified Fact。程序成功率始终是带分母的统计证据，不表达绝对规则。

## 10. 错误语义

`MemoryError` 增加 Consolidation 错误包装和派生召回损坏类别。公开错误正文只包含 Job ID、字段类别或 Scope 的非敏感哈希，不包含正文、Artifact payload、Evidence note、Principal 或凭据。

以下情况返回错误而不是空数组：

- Job 不存在、状态转换非法、Lease 不可用或预算耗尽；
- Job 记录的 Scope 与 Checkpoint Scope 不一致；
- Artifact JSON、计数或强类型 ID 损坏；
- Surface 成员跨 Scope 或引用缺失；
- Artifact Evidence 的 Event/Fragment 关系不成立；
- 派生读取超过统一 Recall deadline。

未运行对应 Consolidation Job、当前查询没有相关派生项，才合法返回空数组。

## 11. 重启与恢复

Job 和派生产物全部保存在当前唯一底层数据库 SQLite 中。`MemoryEngine` 重启后按 Job ID 重新加载状态：

- `Pending` 可运行；
- `Paused` 需显式恢复；
- `Retrying` 遵守持久化重试时间，或经显式 `retry` 恢复；
- Lease 过期的 `Running` Job 可由新 Worker 接管；
- `Completed`、`Failed` 和 `Cancelled` 不会被隐式重跑。

每个完整批次提交后才推进 Checkpoint。崩溃不会产生部分可见批次；已完成批次保持幂等。Snapshot、Restore 和 `rebuild_derived` 继续覆盖这些 SQLite 派生表，不改变权威哈希。

## 12. 测试与验收

所有行为变更按 TDD 实施，至少覆盖：

1. 缺少 `Consolidate` Grant 时提交、查看和控制 Job 均被拒绝；
2. 使用 Tenant A 的上下文不能查看或控制 Tenant B 的 Job；
3. 十二类 `JobKind` 都分派到正确 Handler；
4. Pause、Resume、Retry、Cancel 和 Lease 接管保持现有状态机语义；
5. Core 重启后能读取 Job、继续 Checkpoint 并召回已提交派生项；
6. 两个独立成功/失败来源形成带样本数的 Procedure，重复 Event 不重复计数；
7. Constraint/Preference 只有达到独立 Verified Evidence 门槛后可见；
8. Prediction、Inference、Plan、Quarantined 和 Invalidated 不能产生稳定知识；
9. Surface 只有与激活 Point 相交且时间、状态有效时可见；
10. `past_failures` 只包含激活范围内的 Failure；
11. `max_nodes` 截断覆盖所有新增 Packet 类别；
12. 所有派生 Evidence 可回溯到同 Scope 的 Fragment/Event；
13. 跨 Scope 引用、损坏 JSON、计数不一致和缺失引用均失败关闭；
14. `recommended_context` 包含新增 Packet 字段且保持确定性；
15. 旧 64 字符派生 ID 经精确 Scope Rebuild 后变为规范 UUID，权威哈希不变；
16. 原有 State、Graph、Mutation、隔离和污染测试继续通过。

阶段完成前运行 Rust 聚焦测试、workspace fmt、严格 Clippy、workspace test、rustdoc 和 `git diff --check`。本阶段只证明当前主机的 Core/SQLite 行为，不把结果描述为远程协议、Web、外部平台或其他操作系统实机验证。

## 13. 提交与阶段边界

设计、实施计划和每个代码 Task 使用独立中文三段式 Conventional Commit，并逐次推送 `feat/consolidation-experience-recall`。计划中的步骤完成后立即勾选。阶段验收完成后创建并推送 `core-consolidation-experience-recall` Tag。

用户原有未跟踪目录 `sdk/java/bin/` 和 `sdk/kotlin/bin/` 不属于本阶段，不提交、不删除、不覆盖。
