# Point、Edge 与 Surface

疆诚记忆用 Point、Edge 和 Surface 表达可重建的认知结构，但三者都不是权威记录。权威来源始终是不可变的 `CognitiveFragment`、`MemoryEvent` 和 `EvidenceRef`。任何图节点、关系或聚合面都必须能沿来源标识回到原始证据。

## Point：原子认知变化

Point 是从一个或多个 Fragment 派生出的最小认知单元。当前 `PointKind` 包括 Fact、Decision、Constraint、StateChange、Action、Result、Failure、Preference 和 Correction。

每个 `CognitivePoint` 保存：

- `id`：派生 Point 的稳定标识；
- `scope`：与来源完全一致的所有权边界；
- `kind`：原子认知角色；
- `source_fragments`：一个或多个权威 Fragment 标识；
- `occurred_at`、`confidence`、`validity`：继承或保守派生的时间与可信状态。

SQLite Graph Projector 使用来源 Fragment ID 作为 Point 的确定性强类型 ID。相同 Fragment 在在线追赶、进程重启和 Rebuild 后得到相同 Point ID，不依赖进程内随机数。

Observation、Claim、Inference、Prediction、ToolCall、ToolResult、Plan、Approval 和 Rejection 不会直接转换为 Fact Point。尤其是 Prediction、Inference 和 Plan，即使来自模型，也不能绕过证据提升规则成为 Verified Fact。

## Edge：带证据的有向关系

Edge 连接同一精确 Scope 内的两个 Point。每条 `CognitiveEdge` 都包含方向、`EdgeKind`、权重、置信度、有效时间以及至少一个 `EvidenceLink`。

关系语义必须显式区分：

- `CoActivated` 只表示共同激活，不表示因果；
- `Causes`、`CausedBy` 表示因果方向，但仍需查看证据和验证标志；
- `Supports`、`Contradicts`、`Supersedes` 表达证据与版本关系；
- `PredictedBy`、`ValidatedBy`、`DisprovedBy` 保留预测与后续结果之间的身份边界；
- `ExecutedAs`、`ResultedIn` 连接决策、动作和结果。

在线权重调整会把结果裁剪到 `0.0..=1.0`，并保留产生调整的权威证据。共同出现候选默认既不是 `human_confirmed`，也不是 `tool_verified`。

持久化 Mutation Edge 使用产生关系的权威 Event ID 作为确定性强类型 Edge ID。Graph Projector 只在同一精确 Scope 内解析端点，并把 relation、来源 Fragment/Event/EvidenceRef 一起写入 SQLite；无法证明端点或 Scope 时整批回滚且不推进 Checkpoint。

## Surface：版本化认知子空间

Surface 是一组 Point 和内部关系的版本化聚合，用于表达任务、主题或动态上下文。一个 Point 可以属于多个 Surface，不需要被移动或复制。

每个 `SurfaceVersion` 保存成员权重、成员 Edge、语义中心、激活阈值、生命周期状态、有效时间和来源证据。生命周期支持 Active、Frozen、Inactive、Split 和 Merged；AddMember、RemoveMember、Merge、Split、Freeze 与 Deactivate 都创建新版本，不覆盖旧版本。

Recall 只激活查询时间点有效且状态允许的 Surface。激活值来自已访问成员 Point 的加权贡献，并受版本阈值约束。

## 权威性与重建

```text
CognitiveFragment + MemoryEvent + EvidenceRef
                    |
                    v
             Point / Edge / Surface
                    |
                    v
          Recall / Explain / MemoryPacket
```

箭头表示派生，不表示反向覆盖。派生结构损坏或算法升级时，应从权威记录和版本化元数据重建，而不是修改原始 Fragment/Event 来迎合新图。

## 当前实现边界

- `memory-graph`、`memory-surface` 已实现有界仓储、版本和生命周期规则；
- Core 的 Mutation 先追加权威 Fragment/Event/EvidenceRef，Graph Projector 再按 Event sequence 生成持久化 Point 和 Edge；
- Recall 在 deadline 内按精确 Scope 追平 Graph Checkpoint，并按候选 Point、允许的 Edge kind 和 `max_nodes` 有界读取；
- Core 重新打开或执行 Rebuild 后会从权威 Event 恢复相同的 Point/Edge 标识和 Mutation 关系；
- Surface 表和模块已经存在，但自动生成 Surface 以及把全部 Surface 接入 Core Memory Packet 仍不属于本闭环的已验证范围；
- 外部 Embedding、向量数据库和模型生成 Surface 不属于当前已验证能力。
