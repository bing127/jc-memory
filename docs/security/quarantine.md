# Quarantine 与污染防护

M3 在 `MemoryEngine::observe` 的权威持久化之前执行确定性风险评估。规则只决定准入状态，不删除、摘要或改写调用方提供的正文与 `EvidenceRef`。

## 1. 检测与准入

`QuarantinePipeline` 聚合以下本地规则，风险分数取所有命中规则的最大值：

| `RiskLabel` | 当前规则 | 检测器版本 |
| --- | --- | --- |
| `PromptInjection` | 改写上层指令，或请求显示隐藏提示/记忆的组合模式 | `injection-rules/1` |
| `CrossScopeReference` | `EvidenceRef` 指向精确 Scope 之外的 Fragment 或 Event | `injection-rules/1` |
| `RepeatedDisputedClaim` | 同 Scope 内相同争议 Claim/Fact 已重复出现至少两次 | `poisoning-rules/1` |
| `SuspiciousToolResult` | Tool 来源自行声明 `verified=true` 或授权已授予 | `poisoning-rules/1` |
| `SensitiveData` | 命中私钥、API Key、密码、Token 等低成本标记 | `sensitive-rules/1` |

分数达到 `0.8` 时返回 `Admission::Quarantine`，原 Fragment 以 `ValidityStatus::Quarantined` 保存，并追加一条带风险评估的治理 Fragment/Event。未达到隔离阈值时，模型来源以及 `Prediction`、`Inference`、`Plan` 仍被强制降为 `Unverified`；规则输出不能把它们提升为 Verified Fact。

## 2. Recall 可见性

默认 Recall 会跳过处于 Quarantine 且没有有效 Release 记录的 Fragment。Reject 后仍不可见；Release 后只在审查生效时间之后以 `Unverified` 身份参与 Recall。原始 Fragment 的正文、Evidence 和持久化 `Quarantined` 有效性保持不变，释放状态来自追加的审查记录，而不是覆盖原记录。

跨 Scope Evidence 在 Observe 阶段按权威仓储核对。风险评估生成的 Evidence 只指回触发检测的原始 Fragment，并记录规则说明和检测器版本，不替换原始 Evidence。

## 3. 人工审查

`review_quarantine` 要求精确 Scope 的 `Admin` Grant、非空理由以及仍处于 Quarantine 的目标。Release 和 Reject 都追加两条权威记录：

1. `Approval` 或 `Rejection` 审计 Fragment/Event；
2. 表示 `released` 或 `rejected` 的 `StateTransition` Fragment/Event。

第二条记录通过 Evidence 关联目标 Fragment 和第一条审计 Event。`quarantine` 表只保存可查询的派生处置状态，不能替代上述权威记录。

## 4. 安全边界

当前检测器是版本化、完全离线、可重复的规则集合，不是通用 Prompt Injection 分类器、内容杀毒器或数据泄漏防护服务。未命中规则不等于内容可信；命中也可能需要人工复核。上层执行器仍必须把 Memory Packet 当作不可信数据，不能把召回内容拼接成更高优先级指令。

规则来源已迁移到版本化 `config/security/quarantine.zh-CN.yaml`。`QuarantinePolicy` 在启动时使用严格 YAML Schema，拒绝未知字段、空词项、重复词项、超长词项、非有限或越界分数，以及零重复阈值；每次风险评估记录策略版本和原始 YAML SHA-256 摘要。外部策略读取或校验失败时直接阻止 Engine 启动，不静默使用另一套规则。

`crates/memory-core/tests/poisoning.rs` 和 `crates/memory-security/tests/quarantine_policy.rs` 覆盖恶意 Prompt、正常安全讨论、重复争议声明、可疑 ToolResult、授权 Release/Reject、自定义规则、阈值变化、原文与 Evidence 保留，以及默认 Recall 隔离。
