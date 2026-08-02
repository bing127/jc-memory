# Memory Packet 协议

Memory Packet 是 Recall 的模型无关只读响应。它传递结构化记忆、证据来源、冲突和预算信息，不包含特定模型的 system prompt、角色标记或供应商专用字段。

## 顶层字段

协议固定包含 18 个顶层字段：

| 字段 | 含义 |
| --- | --- |
| `query_id` | 本次 Recall 标识 |
| `intent` | `RecallMode` |
| `current_states` | 当前有效状态版本 |
| `historical_states` | 与查询相关的历史状态版本 |
| `activated_surfaces` | 被激活的 Surface 版本 |
| `episodes` | 关联权威 Event/Fragment 的情景记录 |
| `points` | 被激活的派生 Point |
| `paths` | Point/Edge 标识组成的召回路径 |
| `procedures` | 带样本数和证据的程序经验 |
| `constraints` | 带证据的约束 |
| `preferences` | 带证据的偏好 |
| `past_failures` | 与查询相关的已观察失败 |
| `evidence` | 去重后的权威证据标识 |
| `conflicts` | 未被静默消解的冲突 |
| `missing_evidence` | 明确的证据缺口 |
| `coverage` | 种子、节点、深度、证据和截断统计 |
| `confidence` | Packet 聚合置信度 |
| `recommended_context` | 从其他字段确定性渲染的 JSON 字符串 |

字段名采用 snake_case。协议新增字段需要经过版本与兼容性决策，不能根据某个模型临时改名。

## 证据不变量

- `EpisodeView` 必须包含 `event_id` 和 `fragment_id`；
- `PointView` 必须包含 `source_fragments`；
- State、Surface、Procedure、Knowledge 和 Conflict View 必须携带来源 Evidence；
- `EvidenceView` 至少锚定一个权威 Fragment，并可附带 Event 或 EvidenceRef；
- 摘要、路径、置信度和 `recommended_context` 都不能覆盖原始记录；
- Prediction、Inference 和 Plan 不因进入 Packet 而获得 Fact 身份。

## `recommended_context`

`recommended_context` 由 Packet 的其余 17 个字段序列化得到，并在渲染前删除自身，因而不会递归。相同 Packet 数据得到相同 JSON 文本。调用方可以直接消费结构化字段，也可以把该字符串作为普通数据上下文；协议不规定模型角色、提示词模板或供应商消息格式。

## Core 当前填充范围

当前 `MemoryEngine::recall` 从 SQLite 和派生图填充 State、`episodes`、`points`、`paths`、Surface、Procedure、Constraint、Preference、PastFailure、`evidence`、`missing_evidence`、`coverage`、`confidence` 与 `recommended_context`。只有真正激活的 Point 对应 Episode/Evidence 才会返回；PastFailure 也只从这些已激活 Episode 中筛选。

State 与 Point 先共享 `max_nodes`，剩余预算再按 Surface、Procedure、Constraint/Preference 的固定顺序扣减。一个 Surface 的成本是 Surface 本身加完整成员数，不会返回部分成员。所有读取位于同一 Recall deadline 内；预算不足或候选被排除时，`coverage.truncated` 会显式反映截断。

Conflict 仍由当前 State 投影路径填充。空数组只表示本次查询在精确 Scope、有效时间、相关性和预算限制下没有返回该类数据，不应解释为全局不存在。当前连通范围是 Embedded Core + SQLite；远程 API、Web 控制台、Runtime 自动采集和外部平台实机未由本阶段验证。

## 示例骨架

```json
{
  "query_id": "<uuid-v7>",
  "intent": "semantic",
  "current_states": [],
  "historical_states": [],
  "activated_surfaces": [],
  "episodes": [],
  "points": [],
  "paths": [],
  "procedures": [],
  "constraints": [],
  "preferences": [],
  "past_failures": [],
  "evidence": [],
  "conflicts": [],
  "missing_evidence": [],
  "coverage": {
    "seed_count": 0,
    "visited_nodes": 0,
    "max_depth": 0,
    "evidence_count": 0,
    "missing_evidence_count": 0,
    "truncated": false
  },
  "confidence": 0.0,
  "recommended_context": "{}"
}
```

示例中的标识仅说明格式，不是固定 Fixture；实际 `recommended_context` 会包含除自身之外的完整 Packet 数据。
