# 疆诚记忆 M3 巩固、神经 Fallback 与安全治理实施计划

> **面向执行代理：** 必须使用 `superpowers:subagent-driven-development`（推荐）或 `superpowers:executing-plans` 逐 Task 执行本计划；所有步骤使用复选框跟踪。

**目标：** 实现保守、增量、可恢复的记忆巩固，确定性 Neural Fallback，以及无法被上层绕过的权限隔离、审计、Quarantine、Snapshot、Restore、Rebuild 和治理删除。

**架构：** `memory-security` 在 Repository 查询前和 Packet 返回前执行双重授权；`memory-neural` 只提供可版本化的派生评分；`memory-consolidation` 用持久化 Job/Checkpoint 增加结构。所有派生数据可重建，权威 Event 和 Evidence 不被巩固覆盖。

**技术栈：** Rust 2024、Tokio、SQLx、AES-256-GCM、async-trait、Serde、tracing、确定性哈希/稀疏模型、tokio-util CancellationToken。

---

## 文件边界

- `crates/memory-security/src/{access,acl,guard,audit,quarantine,retention,export,delete,key}.rs`
- `crates/memory-neural/src/{traits,fallback,manifest,circuit_breaker}.rs`
- `crates/memory-consolidation/src/{job,runner,checkpoint,replay,dedup,edges,surfaces,semantic,stale,rebuild}.rs`
- `crates/memory-storage/src/{snapshot,export,recovery}.rs`
- `crates/memory-core/src/{security,maintenance}.rs`
- `crates/memory-core/tests/{tenant_isolation,poisoning,consolidation_recovery,snapshot_restore,hard_delete}.rs`

## Task 1：定义 AccessContext、Grant 与安全协议

**文件：** 创建 `memory-protocol/src/security.rs`、`memory-security/src/{access,acl}.rs`，修改 Workspace。

- [x] **步骤 1：写默认拒绝失败测试**

```rust
#[test]
fn missing_grant_is_denied() {
    let context = AccessContext::new(principal("alice"), scope("tenant-a"));
    assert_eq!(AclEngine::default().authorize(&context, Operation::Recall, &resource("tenant-a")), Decision::Deny(DenyReason::MissingGrant));
}
```

- [x] **步骤 2：运行测试**，预期 AccessContext 缺失。
- [x] **步骤 3：实现：**

```rust
pub struct AccessContext { pub principal_id: String, pub scope: MemoryScope, pub grants: BTreeSet<Grant>, pub request_id: String }
pub enum Operation { Observe, Recall, Explain, Mutate, Consolidate, Export, Delete, Admin }
pub struct Grant { pub operation: Operation, pub namespace_id: NamespaceId, pub tenant_id: Option<TenantId>, pub project_id: Option<ProjectId> }
pub enum Decision { Allow, Deny(DenyReason) }
```

Grant 只能匹配同 Namespace，并对 Tenant/Project 使用“Grant 为 None 表示未授权该层级”而不是通配；跨 Tenant 必须显式 Admin Grant。

- [x] **步骤 4：运行 `cargo test -p jc-memory-security`**，覆盖 Scope 矩阵。
- [x] **步骤 5：提交 `feat(security): add explicit access contexts and ACL`**。

## Task 2：把双重权限过滤下沉到 Recall

**文件：** `memory-security/src/guard.rs`、`memory-storage/src/repository.rs`、`memory-activation/src/query.rs`、`memory-core/src/security.rs`、`tests/tenant_isolation.rs`。

- [x] **步骤 1：写跨 Tenant 种子与结果泄漏失败测试**

```rust
#[tokio::test]
async fn recall_never_loads_or_returns_other_tenant() {
    let h = two_tenant_harness().await;
    let packet = h.engine.recall(h.context_a(), query("same secret project")).await.unwrap();
    assert!(packet.evidence.iter().all(|item| item.scope.tenant_id == Some(h.tenant_a)));
    assert_eq!(h.spy.loaded_scope_keys(), vec![h.scope_key_a]);
}
```

- [x] **步骤 2：运行测试**，预期 Recall 仍未要求 AccessContext 或 Spy 观察到跨域候选。
- [x] **步骤 3：所有 Core API 改为显式接收 `&AccessContext`；Repository Query 必须从授权后的 ScopeKeySet 构造 SQL，禁止先取全局候选再过滤。**
- [x] **步骤 4：PacketBuilder 在序列化前调用 `OutputGuard::filter_and_assert`，发现越权引用时返回 SecurityInvariantViolation 并写审计，不返回部分 Packet。**
- [x] **步骤 5：运行隔离测试并提交 `feat(security): enforce pre-query and pre-return filtering`**。

## Task 3：实现 Quarantine、注入和污染检测

**文件：** `memory-security/src/{quarantine,injection,poisoning,sensitive}.rs`、`tests/poisoning.rs`。

- [x] **步骤 1：写恶意 Prompt、重复错误和可疑 ToolResult 失败测试**，断言它们进入 Quarantine 且默认 Recall 不可见。
- [x] **步骤 2：运行聚焦测试**，预期 QuarantinePipeline 缺失。
- [x] **步骤 3：实现：**

```rust
pub struct RiskAssessment { pub labels: BTreeSet<RiskLabel>, pub score: f32, pub evidence: Vec<EvidenceRef>, pub detector_versions: Vec<String> }
pub enum RiskLabel { PromptInjection, CrossScopeReference, RepeatedDisputedClaim, SuspiciousToolResult, SensitiveData }
pub enum Admission { Accept, AcceptUnverified, Quarantine { reason: QuarantineReason } }
```

规则只能改变 Admission/Validity，不能删除原始内容；检测器输出必须记录版本和触发证据。

- [x] **步骤 4：实现授权 Review 的 Release/Reject，均追加审计和状态事件；运行测试。**
- [x] **步骤 5：提交 `feat(security): quarantine untrusted and poisoned memory`**。

## Task 4：实现 Neural Trait、Manifest 与确定性 Fallback

**文件：** `memory-neural/src/{traits,manifest,fallback,circuit_breaker}.rs`、`tests/fallback.rs`。

- [x] **步骤 1：写相同输入稳定输出、批处理顺序、超时降级失败测试**。
- [x] **步骤 2：运行测试**，预期 FragmentEncoder 等 Trait 缺失。
- [x] **步骤 3：实现八个 Trait：**

```rust
#[async_trait]
pub trait FragmentEncoder: Send + Sync { async fn encode(&self, batch: &[CognitiveFragment]) -> Result<Vec<Embedding>, NeuralError>; fn manifest(&self) -> &ModelManifest; }
pub trait EventClassifier: Send + Sync { async fn classify(&self, batch: &[CognitiveFragment]) -> Result<Vec<Classification>, NeuralError>; fn manifest(&self) -> &ModelManifest; }
pub trait ImportancePredictor: Send + Sync { async fn predict_importance(&self, batch: &[CognitiveFragment]) -> Result<Vec<ImportanceScore>, NeuralError>; fn manifest(&self) -> &ModelManifest; }
pub trait EdgePredictor: Send + Sync { async fn predict_edges(&self, batch: &[EdgeCandidate]) -> Result<Vec<EdgeCandidate>, NeuralError>; fn manifest(&self) -> &ModelManifest; }
pub trait QueryRouter: Send + Sync { async fn route(&self, query: &RecallQuery) -> Result<RoutingDecision, NeuralError>; fn manifest(&self) -> &ModelManifest; }
pub trait Reranker: Send + Sync { async fn rerank(&self, query: &RecallQuery, candidates: &[IndexedPoint]) -> Result<Vec<RankedCandidate>, NeuralError>; fn manifest(&self) -> &ModelManifest; }
pub trait SurfaceAssigner: Send + Sync { async fn assign(&self, points: &[CognitivePoint], surfaces: &[CognitiveSurface]) -> Result<Vec<SurfaceScore>, NeuralError>; fn manifest(&self) -> &ModelManifest; }
pub trait ContradictionDetector: Send + Sync { async fn detect(&self, batch: &[CognitiveFragment]) -> Result<Vec<ContradictionScore>, NeuralError>; fn manifest(&self) -> &ModelManifest; }
```

- [x] **步骤 4：实现词法哈希 Embedding、规则路由/分类、稳定重排、Manifest SHA-256、超时和三态 CircuitBreaker；派生记录必须保存 model_version。**
- [x] **步骤 5：运行 `cargo test -p jc-memory-neural` 并提交 `feat(neural): add deterministic offline neural interfaces`**。

## Task 5：实现持久化 Consolidation Job Runner

**文件：** `memory-consolidation/src/{job,runner,checkpoint}.rs`、`tests/job_recovery.rs`。

- [x] **步骤 1：写 Pause/Resume/Retry/Checkpoint 失败测试**

```rust
#[tokio::test]
async fn resume_continues_after_last_committed_cursor() {
    let h = harness_with_fail_after(50).await;
    let job = h.submit(JobKind::Replay).await;
    assert!(h.run(job).await.is_err());
    let checkpoint = h.checkpoint(job).await;
    h.resume(job).await.unwrap();
    assert_eq!(h.processed_before(checkpoint).await, 0);
}
```

- [x] **步骤 2：运行测试**，预期 JobRunner 缺失。
- [x] **步骤 3：实现 `JobSpec { kind, scope, cursor, limits }`、Pending/Running/Paused/Retrying/Completed/Failed/Cancelled 状态、Lease、attempt 和 JSON Checkpoint；每批事务提交后才推进 cursor。**
- [x] **步骤 4：实现 CancellationToken、CPU/IO/批量/时间预算和指数退避；崩溃后的过期 Lease 可被同 Scope Worker 接管。**
- [x] **步骤 5：运行测试并提交 `feat(consolidation): add resumable checkpointed jobs`**。

## Task 6：实现全部巩固 Job

**文件：** `memory-consolidation/src/{replay,dedup,edges,surfaces,semantic,stale,cold,rebuild}.rs`、`tests/conservative_learning.rs`。

- [x] **步骤 1：写“多次小改成功、大改失败”经验学习测试**，断言输出带样本数、置信区间和 Evidence，且文案不是绝对规则。
- [x] **步骤 2：运行测试**，预期 Job Handler 缺失。
- [x] **步骤 3：逐个实现 Replay、Deduplication、EdgeReinforcement、EdgeWeakening、SurfaceDiscovery、SurfaceMerge、SurfaceSplit、SemanticPromotion、ConflictDetection、StaleStateDetection、ColdTierMovement、Rebuild；每个 Handler 实现 `ConsolidationTask::process_batch`。**
- [x] **步骤 4：Dedup 只创建 SameEntity/CoActivated；SemanticPromotion 至少需要独立来源阈值；Surface 变更创建新版本；Rebuild 只写派生表。**
- [x] **步骤 5：运行 Consolidation 全测并提交 `feat(consolidation): add conservative memory learning jobs`**。

## Task 7：实现 Snapshot、Restore 与可重建派生层

**文件：** `memory-storage/src/{snapshot,export,recovery}.rs`、`memory-core/src/maintenance.rs`、`tests/snapshot_restore.rs`。

- [x] **步骤 1：写 Snapshot/Restore 权威哈希一致和派生索引删除重建测试。**
- [x] **步骤 2：运行测试**，预期 Snapshot API 缺失。
- [x] **步骤 3：Snapshot Manifest 记录 schema_version、SQLite checksum、Blob 列表与 checksum、Scope、创建时间；使用 SQLite online backup/一致读事务和临时目录原子完成。**
- [x] **步骤 4：Restore 先在隔离目录完整校验，再原子切换数据目录；Rebuild 顺序为 FTS -> Embedding -> Edge Candidate -> Surface Member -> Summary/Cache，并保存 Job Checkpoint。**
- [x] **步骤 5：运行测试并提交 `feat(storage): add verified snapshots restore and rebuild`**。

## Task 8：实现 Key Provider、审计、Retention、Export 与 Hard Delete

**文件：** `memory-security/src/{key,audit,retention,export,delete}.rs`、`tests/hard_delete.rs`。

- [x] **步骤 1：写无授权删除拒绝、删除后正文/Blob/FTS/图均消失、审计不含正文的失败测试。**
- [x] **步骤 2：运行测试**，预期 GovernanceService 缺失。
- [x] **步骤 3：实现 `KeyProvider::get_key(key_id)`、轮换元数据和 Zeroizing；审计记录 principal、operation、target digest、request_id、result，不记录正文/密钥。**
- [x] **步骤 4：Retention 先生成可审计计划再执行；Export 输出 Manifest + canonical JSONL + Blob；Hard Delete 在独占治理 Job 中清理权威、BlobRef 和派生数据，留下 content-free receipt 并排队重新巩固。**
- [x] **步骤 5：运行测试并提交 `feat(security): add governed export retention and deletion`**。

## Task 9：M3 验收、文档与门禁

**文件：** 五个 Core 验收测试；`docs/security/{access-control,quarantine,retention,threat-model}.md`、`docs/architecture/consolidation.md`、ADR 0007-0008、README/CHANGELOG。

- [x] **步骤 1：运行跨 Tenant、Prompt Injection、Memory Poisoning、经验学习和 Snapshot/Restore 验收场景。**
- [x] **步骤 2：运行每种 Job 的中断/恢复和资源上限测试。**
- [x] **步骤 3：编写安全模型、权限矩阵、威胁模型、巩固和派生重建文档。**
- [x] **步骤 4：运行全仓 Rust 门禁和 `git diff --check`。**
- [x] **步骤 5：提交 `docs: complete M3 consolidation and security milestone`**。

## M3 完成定义

- Recall 检索前和返回前均执行授权；
- 隔离与污染测试无泄漏；
- Neural Fallback 完全离线且确定；
- 全部巩固 Job 可暂停、恢复、重试并受资源限制；
- Snapshot/Restore 哈希一致，派生层可重建；
- Hard Delete 清理全部引用并留下无正文审计凭证；
- 全仓门禁通过，无占位实现。
