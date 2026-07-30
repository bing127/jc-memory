# 模式分离与有界模式补全

疆诚记忆把写入时的“不要误合并”和读取时的“从模糊线索恢复证据”分成两条独立路径。前者是 Pattern Separation，后者是 Bounded Pattern Completion。

## 写入侧：模式分离

`PatternSeparator` 使用六个可检查维度比较新 Fragment 与既有 Point：内容、实体、时间、项目、来源和状态。只有规范内容、精确 Scope、实体、来源、五分钟时间桶和状态签名都一致时，才判定为 `ExactDuplicate`。

高相似但不完全相同的输入只能得到 `LinkSimilar`，并保留为不同 Point；低于阈值则得到 `CreateDistinct`。相似关系最多形成低置信共同激活候选，不能自动生成因果或 Verified Fact。

## 读取侧：快慢记忆协作

当前 Recall 使用两类来源：

1. SQLite FTS5 在精确 Scope 内执行字面全文检索；
2. 非 `Exact` 模式读取带 `LIMIT` 的近期文本候选，再由确定性词项覆盖率、字符稀疏特征和显式实体进行评分。

这里的“稀疏特征”是进程内确定性字符特征，不是外部 Embedding，也不依赖远程模型。查询和候选共享词项或局部字符模式时，可以从不同措辞恢复原始 Fragment；它不等同于通用自然语言语义模型。

候选读取必须同时满足：

- 精确 `MemoryScope`；
- 可选 `at_time` 历史截止时间；
- 稳定时间与 ID 排序；
- 存储层硬 `LIMIT`；
- Invalidated 和 Quarantined 记录不进入正常激活。

## 局部传播

每个 `RecallQuery` 都携带 `RecallLimits`：

| 限制 | 含义 |
| --- | --- |
| `max_seeds` | 初始种子 Point 上限 |
| `max_neighbors` | 每个 Point 可扩展的邻居上限 |
| `max_hops` | 从种子向外传播的最大深度 |
| `max_nodes` | 本次 Recall 可访问并返回的 Point 总上限 |
| `timeout_ms` | 墙钟超时上限 |

传播使用全局优先队列，优先保留激活值更高的候选；同分时按时间和 PointId 稳定排序。每种 `RecallMode` 都有显式 Edge 白名单，避免 Semantic 查询意外穿越任意因果或执行关系。`Exact` 不传播 Edge，`DeepReconstruction` 才允许完整关系集合，但仍受同一组硬预算约束。

## Surface 与证据缺口

局部传播完成后，系统根据已访问成员计算 Surface 激活，只使用查询时间点有效的版本。对于 Unverified 或 Disputed Point，补全阶段只沿验证、反证等指定关系寻找证据，并继续消耗原有跳数、邻居数和节点预算。

`RecallTrace` 和 `Coverage` 返回实际种子数、访问节点数、最大深度及 `truncated`。达到任何硬限制时，结果明确标记截断，不把不完整结果伪装成全量答案。

## 当前边界

- Core 已接通 SQLite FTS5、近期有界候选、确定性评分和 Memory Packet；
- Mutation 产生的派生 Edge 当前在同一进程内可参与传播；
- 状态仓储、语义记忆和 Surface 模块已有独立测试，但尚未全部汇入 Core 的持久化 Packet 组装；
- 当前没有向量数据库、外部 Embedding、跨进程派生图自动重建或百万级性能结论。
