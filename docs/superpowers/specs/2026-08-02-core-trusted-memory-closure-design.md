# Core 可信记忆闭环设计

## 1. 背景

当前 Core 已具备不可变 Fragment/Event/EvidenceRef、SQLite FTS5、基础 Recall、Mutation、ACL、Snapshot/Restore 和派生重建，但仍有三个直接影响产品语义的断点：

1. `MemoryPacket` 定义了当前状态和历史状态，Core 构包路径却没有填充这些字段；
2. Mutation 关系只写入进程内 `derived_graph`，重启后 Supersedes、Supports 和 Contradicts 等关系丢失；
3. `jc-memory-episodic::StateRepository` 是独立的进程内实现，没有接入权威 Event、SQLite 投影和远程协议。

本设计关闭上述断点，使疆诚记忆能够可靠回答“当前是什么、以前是什么、何时改变、为什么改变”，同时继续遵守权威记录不可被派生数据覆盖的原则。

疆诚记忆自身的底层存储始终使用 SQLite。原始需求中的 PostgreSQL 到 MySQL 只是一组被记忆的业务事实，不代表存储引擎切换。产品示例统一使用部署策略从 `blue_green` 变为 `canary`，原始验收场景仍作为兼容测试保留并明确标注为记忆内容。

## 2. 目标

- 定义显式、可校验的结构化 State 契约，不从自然语言猜测事实；
- 从权威 Event 增量生成可重建的 State、Point 和 Edge SQLite 投影；
- 使用精确 Scope Checkpoint 保证投影可续跑、可重放且不跨 Tenant；
- 让 Core Recall 返回真实的 `current_states` 和 `historical_states`；
- 让 Mutation 关系在 Core 重启后仍可 Recall 和 Explain；
- 让 Snapshot/Restore/Rebuild 覆盖新增投影且不改变权威哈希；
- 保持 HTTP、gRPC、Sidecar、MCP 和旧客户端的协议兼容；
- 在 Web Memory Lab 提供显式 State 输入和历史查询入口。

## 3. 非目标

本阶段不实现以下能力：

- 使用 LLM、Embedding、正则模板或启发式规则从普通文本抽取 State；
- 外部向量数据库或远程语义模型；
- 自动执行全部 Consolidation Handler；
- 把程序经验、Surface、约束和偏好全部接入 Memory Packet；
- 远程 Tool/Approval、组织级多 API Key 和治理控制台；
- Recall SQL 性能专项优化和生产容量承诺。

这些能力分别进入后续性能、经验召回和远程 Runtime/Tenant 子项目。

## 4. 权威与派生边界

Fragment、Event 和 EvidenceRef 继续作为唯一权威记录。StateVersion、Point、Edge、Surface、索引和 Projection Checkpoint 都是可删除、可重建的派生数据。

写入顺序固定为：

1. 校验 Fragment 及显式 State metadata；
2. 事务性追加 Fragment、Event 和 EvidenceRef；
3. 返回权威写入结果；
4. 后续由 Projector 按 Event sequence 增量处理派生数据。

Correct、Supersede 或 Contradict 携带 `metadata.state` 时，第一步必须在同 Scope 内解析目标 StateVersion 并校验 State key。目标不存在、未映射到 StateVersion、跨 Scope 或 key 不一致时，写入返回 State metadata 无效且不追加 Fragment/Event/EvidenceRef。Projector 对旧数据、恢复数据和损坏数据重复执行相同校验；发现不合法 Event 时回滚该批次并保持 Checkpoint 不前进。

派生失败不得回滚已经提交的权威 Event，也不得修改原始 Fragment 的 Kind、Validity、正文或 EvidenceRef。Recall 在读取派生数据前必须追平对应 Scope 的 Checkpoint；追平失败时返回结构化错误，不能用空字段伪装成“没有状态”。

## 5. 显式 State 契约

State 使用 Fragment `metadata.state` 表达：

```json
{
  "state": {
    "key": "project.deployment_strategy",
    "value": "blue_green",
    "reason": "初始发布策略"
  }
}
```

后续版本使用相同 `key`：

```json
{
  "state": {
    "key": "project.deployment_strategy",
    "value": "canary",
    "reason": "降低全量发布风险"
  }
}
```

约束如下：

- `key` 必须是 1 到 256 字节的非空稳定标识，只允许 ASCII 字母、数字、点、下划线和连字符；
- `value` 可以是任意合法 JSON 值，但序列化后不得超过现有 Fragment JSON 内容上限；
- `reason` 必须是 1 到 2048 字符的非空文本；
- 只有 Fact、StateTransition 和 Correction 可以声明 `metadata.state`；
- StateVersion 继承 Fragment 的 Validity、occurred_at、Scope 和 Evidence；
- Prediction、Inference、Plan 和 Quarantined Fragment 不得产生当前状态投影；
- 普通 metadata 不受影响；没有 `metadata.state` 的 Fragment 不参与状态投影。

不完整或类型错误的 `metadata.state` 在权威写入前被拒绝。系统不会把普通文本自动解释为 State。

## 6. SQLite 投影模型

新增 Migration，在现有派生表基础上进行兼容扩展：

- `state_versions.reason`：可选变更原因；
- `state_versions.source_event_id`：产生该版本的权威 Event；
- `projection_checkpoints`：按 `scope_key + projector` 保存最后成功处理的 Event sequence 和更新时间；
- 为 State key、有效时间和投影追赶查询补充复合索引。

`states` 继续保存稳定 State ID 和唯一当前版本指针；`state_versions` 保存不可变版本链。旧版本关闭时只修改派生表中的 `valid_to_us` 和派生 Validity，不修改来源 Fragment。

Point、Edge 使用可解析为协议强类型 ID 的确定性 UUID。一个 Fragment 的基础 Point ID 必须在 Core 即时 Recall、Projector 重放和 Consolidation Replay 中保持一致。旧的非规范派生 ID 通过 Rebuild 清理并重新生成，不迁移为权威数据。

## 7. Projector 与 Checkpoint

第一阶段包含两个 Projector：

### 7.1 State Projector

- 按精确 Scope 和 Event sequence 读取未处理 Event；
- 先识别合法 `metadata.state` 和带 `target_fragment_id` 的 State 相关 Mutation；两者都不存在的 Event 不产生 State 投影；
- 首个 key 创建 State 和 StateVersion；
- 更晚的版本关闭唯一当前版本并建立 `previous_version_id`；
- 同一有效时间出现不同值时保留双方并标记 Disputed，不选择赢家；
- 没有唯一当前版本时将 `states.current_version_id` 置空，Recall 按有效时间和 Validity 返回全部可见争议版本；
- 完全相同的 key、value、时间和来源按幂等规则处理；
- Quarantined、普通 Invalidated Fragment 和 FutureHypothesis 不创建 StateVersion；
- State 相关 Mutation 只能解析同 Scope 的目标；目标未映射到 StateVersion 且未携带 State metadata 时不改变 State 投影，携带 State metadata 时则拒绝处理并保持 Checkpoint 不前进；
- Mutation 的 `metadata.state.key` 必须与目标 StateVersion 的 key 一致，不一致时拒绝处理并保持 Checkpoint 不前进；
- Forget 产生的 Invalidated Mutation 根据同 Scope 的 `target_fragment_id` 关闭对应 StateVersion，将其派生 Validity 标记为 Invalidated，并清除指向它的当前版本指针，不创建替代版本；
- Weaken 根据 `target_fragment_id` 将对应 StateVersion 的派生 Validity 标记为 Disputed；Contradict 除此之外还保留可追溯的 Contradicts Edge，二者都不得静默替换当前值；
- Correct 或 Supersede 只有携带合法 `metadata.state` 时才关闭目标版本并创建新版本；没有 State metadata 时只生成图关系。

### 7.2 Graph Projector

- 为可映射 Fragment 创建确定性 Point；
- 从 Mutation metadata、目标 Fragment 和 EvidenceRef 生成持久化 Edge；
- 保留 Edge kind、有效时间、来源 Fragment、人工确认和工具验证标志；
- 不从共同出现关系推导 Causes；
- 无法证明端点或 Scope 的 Edge 拒绝写入并保持 Checkpoint 不前进。

每个批次在一个 SQLite 写事务内完成“读取 Event、写派生行、推进 Checkpoint”。任何失败同时回滚派生变更和 Checkpoint。多个进程竞争同一 Scope 时由 SQLite 写事务串行化，不能依赖单进程 Mutex 保证正确性。

## 8. Recall 数据流

Recall 流程调整为：

1. 完成 ACL 和精确 Scope 校验；
2. 在请求 deadline 内追平 State 与 Graph Checkpoint；
3. 执行 FTS 和有界候选检索；
4. 从 SQLite 加载相关 Point、Edge 和 StateVersion；
5. 执行有界激活；
6. 构造 Memory Packet；
7. 返回前再次校验 Packet 中所有来源 Scope。

State 相关性规则：

- `current_state` 和 `historical_state` 模式优先把 `query.text` 解释为完整 State key；
- 未精确命中 key 时，只返回来源 Fragment 已进入本次激活集合的 State；
- 普通 Recall 只附带与激活 Fragment 相关的 State，不扫描并返回整个 Scope；
- `at_time` 为空时使用当前时间；指定时按 `[valid_from, valid_to)` 半开区间解析；
- `current_states` 返回查询时刻有效的所有非隐藏版本；
- `historical_states` 返回同一 State 在查询时刻之前的相关版本，按时间倒序且受 Recall `max_nodes` 约束；
- 同时有效的争议版本全部返回，并在 `conflicts` 中保留可追溯冲突。

`StateView` 增加向后兼容的可选字段：

- `reason: Option<String>`；
- `source_event_id: Option<EventId>`。

EvidenceView 继续携带 Fragment、Event 和 EvidenceRef ID。`recommended_context` 由完整 Packet 确定性重建，不增加模型特定提示词。

## 9. Mutation 与重启语义

Correct、Supersede、Reinforce、Weaken、Contradict 和 Forget 仍追加新的权威 Fragment/Event。Graph Projector 根据 Mutation metadata 生成持久化关系。

当 Correct 或 Supersede 同时携带合法 `metadata.state` 时，State Projector 按 State key 生成新版本并关闭被替代版本。没有 State metadata 的 Correction 只影响图关系，不会凭正文猜测 State key。

Forget/Invalidated Mutation 使用 `target_fragment_id` 关闭目标 StateVersion；Weaken 将目标版本标记为 Disputed；Contradict 将目标版本标记为 Disputed，并通过持久化 Contradicts Edge 让冲突对 Recall 和 Explain 可见。若 Contradict 还携带合法 `metadata.state`，新值作为竞争版本写入，双方保持 Disputed，系统不自动选择赢家。上述操作只改变可重建的 State/Graph 投影，绝不改写或删除来源 Fragment、Event 和 EvidenceRef。

Core 打开时不需要把全部图加载进内存。Recall 从 SQLite 按 Scope、候选 Point 和允许的 Edge kind 有界加载关系，因此进程重启不会丢失关系，也不会产生无界全图读取。

## 10. Rebuild、Snapshot 与故障恢复

`rebuild_derived` 执行时：

1. 获取精确 Scope 的维护权限和互斥 Lease；
2. 清理该 Scope 的 State、Point、Edge 和 Projection Checkpoint；
3. 按 Event sequence 重放 State 与 Graph Projector；
4. 重建 FTS；
5. 校验派生引用和最终 Checkpoint；
6. 比较重建前后的权威哈希。

阶段中断时保留最后完整提交的 Checkpoint。恢复后从该 sequence 继续，不重复创建版本或 Edge。Snapshot 包含权威记录和当时的派生投影；Restore 完成后仍可显式 Rebuild 验证投影一致性。

## 11. 协议与服务兼容

- Rust 类型新增字段使用 `Option` 和 Serde default；
- Protobuf 只追加新 field number，不复用或重排已有编号；
- HTTP、gRPC、Sidecar、MCP 继续共享同一个 CommandBus；
- 旧客户端可以忽略新增字段；
- 新客户端解析旧 Packet 时将新增字段视为 `None`；
- State 写入继续复用 Observe/Commit，不新增绕过权威 Fragment 的写接口。

## 12. Web Memory Lab

Memory Lab 增加显式 State 模式，字段包括 State key、JSON value、Reason、Fragment kind、Validity 和发生时间。表单生成 `metadata.state`，浏览器不能提交 Scope 或 Grant。

Recall 页面在 Current State 和 Historical State 模式下提供 State key 输入，结果分别展示当前版本、历史时间线、原因和 Evidence ID。普通 Recall 的现有 Memory Packet 展示保持兼容。

产品示例使用 `project.deployment_strategy: blue_green -> canary`。PostgreSQL 到 MySQL 仅保留在原始需求兼容测试中，并标注为“被记忆的业务内容”。

## 13. 错误语义

新增结构化错误类别：

- State metadata 无效；
- Projection 追赶超时；
- Projection Checkpoint 损坏；
- 派生引用跨 Scope；
- State 版本链损坏；
- Rebuild 后权威哈希变化。

错误正文只包含字段名、Scope 的非敏感哈希和 Projector 名称，不记录 State value、Fragment 正文、API Key 或 Evidence note。

## 14. 测试与验收

行为变更全部按 TDD 实施，至少覆盖：

1. State metadata 合法与非法输入；
2. `blue_green -> canary` 当前值、历史值、时间、原因和 Evidence；
3. 原始 PostgreSQL 到 MySQL 场景作为记忆内容的兼容验收；
4. `at_time` 在变更前、边界时刻和变更后解析；
5. 同时刻不同值形成可见冲突；
6. Forget 关闭目标版本且不产生替代当前值，原始 Fragment/Event/EvidenceRef 保持不变；
7. Weaken/Contradict 产生可见 Disputed 状态和可追溯冲突，不静默替换当前值；
8. Correct/Supersede 仅在合法 `metadata.state` 存在时产生新状态版本；
9. 新 State Mutation 的目标不存在、跨 Scope 或 key 不匹配时不追加权威记录；重放中的非法 Event 不产生部分投影，也不推进 Checkpoint；
10. Core 关闭重开后 State 和 Mutation Edge 保持一致；
11. Checkpoint 落后自动追赶，批次失败不推进；
12. 两个 Tenant 使用相同 State key 时不泄漏；
13. Snapshot/Restore/Rebuild 前后权威哈希不变；
14. Memory Packet Rust/JSON/Protobuf 往返和旧 Fixture 兼容；
15. Embedded、REST、gRPC、Sidecar、MCP 返回规范化一致结果；
16. Web Fixture E2E 和真实 Rust Server E2E 完成 State 写入及历史召回。

本阶段提交前运行适用的 Rust fmt、严格 Clippy、聚焦测试、workspace test、rustdoc、Web test/typecheck/build、两类 Playwright E2E 和 `git diff --check`。

## 15. 后续子项目顺序

Core 可信记忆闭环完成后，按以下顺序继续 C 方案：

1. Recall 性能与真实 E2E：基于 EXPLAIN、可重复压测和真实模式测试分层修复；
2. Consolidation 与经验召回：接入 Core Job 门面并填充 procedures、constraints、preferences 和 Surface；
3. Runtime Tool、Tenant 与远程治理：接通 Tool Commit、Approval 续跑、多 API Key 精确 Scope 和治理远程契约。

每个子项目单独形成中文设计、实施计划、提交和验证记录，不把 Fixture 描述为外部平台实机结果。
