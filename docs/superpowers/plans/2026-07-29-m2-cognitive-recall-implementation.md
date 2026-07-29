# 疆诚记忆 M2 认知图与召回实施计划

> **面向执行代理：** 必须使用 `superpowers:subagent-driven-development`（推荐）或 `superpowers:executing-plans` 逐 Task 执行本计划；所有步骤使用复选框跟踪。

**目标：** 在 M1 权威事件存储之上，实现 Point、Edge、Surface、状态版本、模式分离、有界模式补全、十种 Recall、Explain 和模型无关 Memory Packet。

**架构：** Event 先由情景层生成 Point 与候选关系，语义层只保存带证据的稳定知识，图和 Surface 均为可重建派生结构。Recall 先按 Scope 获取有界种子，再进行意图限定的局部传播，最终构造最小证据子图。

**技术栈：** Rust 2024、Tokio、Serde、SQLx/SQLite FTS5、确定性稀疏向量、petgraph 局部子图、proptest、tracing。

---

## 文件边界

- `crates/memory-episodic/src/{separation,state,repository}.rs`：快速情景写入、模式分离、状态版本链。
- `crates/memory-semantic/src/{knowledge,procedure,repository}.rs`：带证据的事实、偏好、约束和程序经验。
- `crates/memory-graph/src/{point,edge,repository,local_graph}.rs`：Point/Edge 和有界邻接加载。
- `crates/memory-surface/src/{model,membership,lifecycle,repository}.rs`：Surface 版本与生命周期。
- `crates/memory-activation/src/{query,seeds,spread,completion,explain}.rs`：十种 Recall 的统一流水线。
- `crates/memory-protocol/src/{recall,packet,graph,surface,state}.rs`：跨边界协议。
- `crates/memory-core/src/{commands,recall,mutation}.rs`：完整 Memory API 门面。

## Task 1：加入 M2 crate 与协议类型

**文件：** 创建上述五个 crate 和协议模块；修改根 `Cargo.toml`、`memory-core/Cargo.toml`。

- [ ] **步骤 1：写失败测试**

```rust
#[test]
fn recall_and_edge_names_are_stable() {
    assert_eq!(serde_json::to_string(&RecallMode::DeepReconstruction).unwrap(), "\"deep_reconstruction\"");
    assert_eq!(serde_json::to_string(&EdgeKind::ValidatedBy).unwrap(), "\"validated_by\"");
}
```

- [ ] **步骤 2：运行 `cargo test -p jc-memory-protocol recall_and_edge_names_are_stable`**，预期因类型缺失而失败。
- [ ] **步骤 3：实现协议类型**

```rust
pub enum RecallMode { Exact, Semantic, Temporal, Causal, Entity, Associative, Procedural, CurrentState, HistoricalState, DeepReconstruction }
pub enum EdgeKind { TemporalBefore, TemporalAfter, Causes, CausedBy, Supports, Contradicts, Supersedes, DependsOn, PartOf, SameEntity, CoActivated, PredictedBy, ValidatedBy, DisprovedBy, RecalledBy, UsedInDecision, ExecutedAs, ResultedIn }
pub enum SurfaceKind { Explicit, Dynamic, TemporaryTask }
pub enum SurfaceStatus { Active, Frozen, Inactive, Split, Merged }
```

同时增加 PointId、EdgeId、SurfaceId、StateId、StateVersionId、RecallId；全部沿用 M1 UUID v7 和 snake_case Serde 规则。

- [ ] **步骤 4：运行 `cargo test -p jc-memory-protocol`**，预期通过。
- [ ] **步骤 5：提交 `feat(protocol): define cognitive graph and recall contracts`**。

## Task 2：实现 Point 与模式分离

**文件：** `memory-graph/src/point.rs`、`memory-episodic/src/separation.rs` 及各自测试。

- [ ] **步骤 1：写失败测试**

```rust
#[test]
fn same_text_in_different_projects_stays_distinct() {
    let a = fixture("deployment failed", project("alpha"));
    let b = fixture("deployment failed", project("beta"));
    assert_eq!(PatternSeparator::default().decide(&a, &[point_from(&b)]), SeparationDecision::CreateDistinct);
}
```

- [ ] **步骤 2：运行聚焦测试**，预期 `PatternSeparator` 缺失。
- [ ] **步骤 3：实现：**

```rust
pub struct CognitivePoint { pub id: PointId, pub scope: MemoryScope, pub kind: PointKind, pub source_fragments: Vec<FragmentId>, pub occurred_at: DateTime<Utc>, pub confidence: f32, pub validity: ValidityStatus }
pub struct SeparationScore { pub content: f32, pub entity: f32, pub temporal: f32, pub project: f32, pub source: f32, pub state: f32 }
pub enum SeparationDecision { ExactDuplicate(PointId), LinkSimilar(PointId), CreateDistinct }
```

ExactDuplicate 仅允许 canonical 内容哈希、Scope、来源、时间桶和状态全部相同；语义相似只能产生 `CoActivated` 候选。

- [ ] **步骤 4：运行 `cargo test -p jc-memory-graph -p jc-memory-episodic`**，覆盖同名实体、跨项目、跨时间和重复写入。
- [ ] **步骤 5：提交 `feat(episodic): add points and pattern separation`**。

## Task 3：实现 Edge、权重与局部图

**文件：** `memory-graph/src/{edge,repository,local_graph}.rs`、`memory-graph/tests/edge_updates.rs`。

- [ ] **步骤 1：写共同出现非因果和权重裁剪测试**

```rust
#[test]
fn co_occurrence_is_not_verified_causality() {
    let edge = EdgeCandidate::co_activated(point_a(), point_b(), evidence());
    assert_eq!(edge.kind, EdgeKind::CoActivated);
    assert!(!edge.human_confirmed && !edge.tool_verified);
}
```

- [ ] **步骤 2：运行聚焦测试**，预期 Edge 类型缺失。
- [ ] **步骤 3：实现 `CognitiveEdge`、EvidenceLink、`EdgeUpdater::apply(eta, alpha, beta, activations, outcome)` 和 `LocalGraphRepository::load(scope, seeds, filter, max_neighbors, max_nodes)`；所有权重裁剪到 `0.0..=1.0`。**
- [ ] **步骤 4：运行 `cargo test -p jc-memory-graph`**，预期方向、证据、权重和预算测试通过。
- [ ] **步骤 5：提交 `feat(graph): add evidence-linked bounded graph`**。

## Task 4：实现状态版本与语义记忆

**文件：** `memory-episodic/src/state.rs`、`memory-semantic/src/{knowledge,procedure}.rs`、`memory-episodic/tests/state_versions.rs`。

- [ ] **步骤 1：写 PostgreSQL -> MySQL 失败测试**

```rust
#[tokio::test]
async fn replacement_preserves_current_and_history() {
    let store = harness().await;
    store.record("database", "PostgreSQL", at(2024, 1, 1), evidence("initial")).await.unwrap();
    store.supersede("database", "MySQL", at(2025, 6, 1), evidence("migration")).await.unwrap();
    assert_eq!(store.current("database").await.unwrap().value, json!("MySQL"));
    assert_eq!(store.at("database", at(2024, 6, 1)).await.unwrap().value, json!("PostgreSQL"));
}
```

- [ ] **步骤 2：运行测试**，预期 StateRepository 缺失。
- [ ] **步骤 3：实现 `StateVersion { id, state_id, value, valid_from, valid_to, validity, evidence, previous_version_id }`；Supersede 在单事务内关闭旧版本、创建新版本和 Supersedes Edge；同一时点冲突返回 `StateResolution::Conflicted`。**
- [ ] **步骤 4：实现 `SemanticEntry` 和 `ProcedureExperience`，稳定提升至少需要两个独立 Event；成功率必须保存样本数与证据 ID。**
- [ ] **步骤 5：运行测试并提交 `feat(memory): add versioned state and evidence-backed semantics`**。

## Task 5：实现 Surface 生命周期

**文件：** `memory-surface/src/{model,membership,lifecycle,repository}.rs`、`memory-surface/tests/lifecycle.rs`。

- [ ] **步骤 1：写多 Surface 和版本历史失败测试**

```rust
#[tokio::test]
async fn point_can_join_multiple_surfaces() {
    let repo = harness().await;
    let point = repo.point().await;
    repo.add_member(repo.explicit().await, point, 0.8, evidence()).await.unwrap();
    repo.add_member(repo.temporary().await, point, 0.6, evidence()).await.unwrap();
    assert_eq!(repo.memberships(point).await.unwrap().len(), 2);
}
```

- [ ] **步骤 2：运行测试**，预期 SurfaceRepository 缺失。
- [ ] **步骤 3：实现 `CognitiveSurface`、`SurfaceVersion`、`SurfaceBoundary` 和 Add/RemoveMember、Merge、Split、Freeze、Deactivate 命令；每次变化增加 version，Merge/Split 不改写旧历史。**
- [ ] **步骤 4：实现增量加权中心与面内 Contradicts 检测，运行 `cargo test -p jc-memory-surface`**。
- [ ] **步骤 5：提交 `feat(surface): add versioned cognitive surfaces`**。

## Task 6：实现有界 Recall 流水线

**文件：** `memory-activation/src/{query,seeds,spread,completion}.rs`、`memory-activation/tests/bounds.rs`。

- [ ] **步骤 1：写预算失败测试**

```rust
#[tokio::test]
async fn recall_never_exceeds_budget() {
    let limits = RecallLimits { max_seeds: 8, max_neighbors: 6, max_hops: 3, max_nodes: 40, timeout_ms: 100 };
    let result = engine().recall(query().limits(limits)).await.unwrap();
    assert!(result.trace.visited_nodes <= 40 && result.trace.max_depth <= 3);
}
```

- [ ] **步骤 2：运行测试**，预期 RecallEngine 缺失。
- [ ] **步骤 3：实现 `RecallQuery { scope, mode, text, entities, at_time, limits }`、Exact/FTS/确定性稀疏向量种子合并和稳定去重；同分按时间和 PointId 排序。**
- [ ] **步骤 4：实现每种 RecallMode 的 EdgeKind 白名单、优先队列扩散、Surface 激活、证据缺口局部扩展和取消/超时。**
- [ ] **步骤 5：运行 `cargo test -p jc-memory-activation` 并提交 `feat(recall): add bounded activation pipeline`**。

## Task 7：实现 Memory Packet 与 Explain

**文件：** `memory-protocol/src/packet.rs`、`memory-activation/src/explain.rs`、`memory-activation/tests/packet.rs`。

- [ ] **步骤 1：写 JSON 契约失败测试**，精确断言需求中的 18 个顶层字段。
- [ ] **步骤 2：运行测试**，预期 MemoryPacket 缺失。
- [ ] **步骤 3：实现：**

```rust
pub struct MemoryPacket { pub query_id: RecallId, pub intent: RecallMode, pub current_states: Vec<StateView>, pub historical_states: Vec<StateView>, pub activated_surfaces: Vec<SurfaceView>, pub episodes: Vec<EpisodeView>, pub points: Vec<PointView>, pub paths: Vec<RecallPath>, pub procedures: Vec<ProcedureView>, pub constraints: Vec<KnowledgeView>, pub preferences: Vec<KnowledgeView>, pub past_failures: Vec<EpisodeView>, pub evidence: Vec<EvidenceView>, pub conflicts: Vec<ConflictView>, pub missing_evidence: Vec<MissingEvidence>, pub coverage: Coverage, pub confidence: f32, pub recommended_context: String }
pub struct ExplainTrace { pub seeds: Vec<SeedTrace>, pub traversals: Vec<TraversalTrace>, pub surfaces: Vec<SurfaceActivationTrace>, pub inferred_edges: Vec<EdgeId>, pub verified_fragments: Vec<FragmentId>, pub limits: RecallLimits, pub truncated: bool }
```

Packet 中的 View 均为只读协议 DTO：`StateView` 含 state_id/key/value/valid_from/valid_to/validity/evidence；`SurfaceView` 含 surface_id/version/status/member_points/confidence/evidence；`EpisodeView` 含 event_id/fragment_id/occurred_at/summary/evidence；`PointView` 含 point_id/kind/confidence/validity/source_fragments；`RecallPath` 含 ordered point/edge IDs 和 score；其余 Procedure/Knowledge/Evidence/Conflict/MissingEvidence View 均必须包含来源 ID，不能只返回自然语言。

- [ ] **步骤 4：确认 recommended_context 由 Packet 确定性渲染且不含模型专用 Prompt，运行测试**。
- [ ] **步骤 5：提交 `feat(recall): return packets and explanation traces`**。

## Task 8：接通核心 API 与变更关系

**文件：** `memory-core/src/{commands,recall,mutation}.rs`、`memory-core/tests/memory_api.rs`。

- [ ] **步骤 1：写 Recall、Correct、Reinforce、Weaken、Supersede、Contradict、Forget、Explain、Stats 失败测试；普通 Forget 必须追加 Invalidated Event。**
- [ ] **步骤 2：运行 `cargo test -p jc-memory-core --test memory_api`**，预期方法缺失。
- [ ] **步骤 3：实现命令和 MemoryEngine 方法；Mutation 先生成 Fragment/Event，再在事务或可恢复 Job 中更新图，不直接 UPDATE 原始 Fragment/Event。**
- [ ] **步骤 4：运行 Core 测试与 Clippy**。
- [ ] **步骤 5：提交 `feat(core): expose recall and evidence-preserving mutations`**。

## Task 9：验收、文档与门禁

**文件：** `memory-core/tests/{state_history,cross_year,prediction_isolation,recall_bounds}.rs`、`docs/concepts/{point-edge-surface,pattern-completion}.md`、`docs/protocol/memory-packet.md`、ADR 0004-0006、README、CHANGELOG。

- [ ] **步骤 1：实现 Cross-Year Fixture**，断言 2026 年不同措辞查询找回 2018 原始 FragmentId，且节点预算未超限。
- [ ] **步骤 2：实现状态替换、共同出现非因果和 Prediction 不成为 Fact 的验收测试。**
- [ ] **步骤 3：编写点线面、模式补全、快慢记忆、局部传播和 Memory Packet 中文文档。**
- [ ] **步骤 4：运行门禁**

```bash
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo doc --workspace --no-deps
git diff --check
```

- [ ] **步骤 5：提交 `docs: complete M2 cognitive recall milestone`**。

## M2 完成定义

- 十种 Recall 共用有界流水线且没有全图扫描；
- Point、Edge、Surface 和状态版本均能回溯 Fragment/Event；
- CurrentState、HistoricalState、冲突与缺失证据通过测试；
- Explain 说明种子、边、Surface、事实和推断；
- Mutation 全部通过追加事件表达；
- Cross-Year 与状态更新场景通过；
- 全仓 Rust 门禁通过，无占位实现。
