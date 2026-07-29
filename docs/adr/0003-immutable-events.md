# ADR 0003：原始事件不可变，派生数据可重建

- 状态：Accepted
- 日期：2026-07-29

## Context（背景）

疆诚记忆需要区分“实际观察到的记录”和后续摘要、推断、预测或计划。若允许模型输出覆盖原始 Fragment 或 Event，系统将失去证据链，也无法解释某个事实从何而来、何时被纠正。

同时，FTS、图关系、Surface 和其他加速结构可能因算法或模型升级而变化。把这些派生结构当作权威记录，会使重建和迁移变得危险。

## Decision（决策）

原始 `CognitiveFragment`、`MemoryEvent` 和 `EvidenceRef` 是权威记录。Event 保存完整规范化 Fragment payload 和内容 SHA-256；相同标识符只能幂等地指向相同内容，内容冲突必须失败。

不提供原地更新或覆盖 Event 的路径。Correction 和 Supersession 通过新的 Fragment/Event 表达，并保留对旧记录的引用；旧 Event 继续存在。LLM 产生的 Prediction、Inference 和 Plan 只能保留其非事实身份，不能直接进入 Verified Fact。

FTS、图关系、Surface、Embedding、召回激活和其他加速结果均为派生数据。派生数据不得覆盖权威记录，并应能从 Fragment、Event、EvidenceRef 和版本化模型信息重建。

## Consequences（影响）

- 审计、解释和纠错可以沿事件与证据链回到原始输入。
- 相同 ID 的不同内容会被拒绝，调用方必须显式创建新记录表达变化。
- 存储量会随历史增长，需要后续里程碑提供保留策略、归档和快照，但这些机制不能静默删除权威证据。
- 派生索引损坏不会授权修改原始记录；恢复流程优先重建派生状态。
- “当前事实”需要通过有效性、Correction/Supersession 和查询策略计算，不能靠覆盖旧行实现。

## Rejected Alternatives（否决方案）

- **原地更新 Event payload**：实现简单，但会破坏哈希、顺序和审计证据。
- **只保存摘要或 Embedding**：节省空间，却无法恢复原文或证明生成结果。
- **把模型推断直接写成 Verified Fact**：混淆预测与证据，违反产品的事实边界。
- **把 FTS、图或 Surface 作为唯一存储**：这些结构会随算法变化，无法承担权威记录职责。
