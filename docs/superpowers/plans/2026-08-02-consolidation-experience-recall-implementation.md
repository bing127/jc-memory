# Core Consolidation 与经验召回实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将已有十二类 Consolidation Handler 接入受 Core 授权保护的 Job 门面，并让相关 Surface、Procedure、Constraint、Preference 和 PastFailure 进入有界、可追溯的 Memory Packet。

**Architecture:** `MemoryEngine` 复用 SQLite `ConsolidationJobStore` 和 `JobRunner`，按持久化 `JobKind` 分派现有 Handler。Storage 通过精确 Scope、激活 Point/Fragment 和剩余预算读取派生项；Core 在统一 Recall deadline 内组装 Packet 并执行返回前 Evidence/Scope 校验。

**Tech Stack:** Rust 2024、Tokio、SQLx/SQLite、Serde、UUID、现有 `memory-consolidation`、`memory-storage`、`memory-core` 与协议类型。

---

## 文件结构

- `crates/memory-consolidation/src/replay.rs`：区分内部摘要 ID 与协议 UUID，生成规范 Point/Edge/Surface ID。
- `crates/memory-consolidation/src/semantic.rs`：持久化可校验的 Procedure 与 Knowledge Artifact 载荷。
- `crates/memory-consolidation/src/job.rs`：公开取消状态转换供受保护的 Core 门面复用。
- `crates/memory-consolidation/tests/conservative_learning.rs`：规范 ID、Artifact 计数和值的契约测试。
- `crates/memory-storage/src/projection_derivations.rs`：派生召回查询、强类型解码、Evidence 与 Scope 校验。
- `crates/memory-storage/src/projection.rs`、`src/lib.rs`：在 `SqliteProjectionStore` 暴露派生读取入口和类型。
- `crates/memory-storage/tests/recall_derivations.rs`：Procedure、Knowledge、Surface、时间、预算和损坏行测试。
- `crates/memory-core/src/consolidation.rs`：授权后的 Job 提交、查询、运行和生命周期门面。
- `crates/memory-core/src/engine.rs`、`src/lib.rs`、`Cargo.toml`：持有 Job Store、错误转换和公共类型导出。
- `crates/memory-core/src/recall.rs`：在统一预算中填充新增 Packet 字段。
- `crates/memory-core/tests/consolidation_jobs.rs`：授权、十二类分派、生命周期与重启测试。
- `crates/memory-core/tests/experience_recall.rs`：端到端经验召回、隔离、恢复和截断测试。
- `docs/architecture/consolidation.md`、`docs/protocol/memory-packet.md`、`CHANGELOG.md`：已接线能力和验证边界。

### Task 1：规范 Consolidation 派生 ID 与 Artifact

**Files:**
- Modify: `crates/memory-consolidation/src/replay.rs`
- Modify: `crates/memory-consolidation/src/dedup.rs`
- Modify: `crates/memory-consolidation/src/semantic.rs`
- Modify: `crates/memory-consolidation/src/surfaces.rs`
- Modify: `crates/memory-consolidation/tests/conservative_learning.rs`

- [ ] **Step 1：写规范 ID 和可校验 Artifact 红灯测试**

新增测试：Replay 后 Point ID 必须能解析为 `PointId` 且等于来源 `FragmentId`；Surface/派生 Edge 必须能解析为协议 ID；Procedure Artifact 必须包含成功、失败、样本数和成功率；Constraint/Preference Artifact 必须包含可解析 Point ID、原始结构化 `content`、置信度和 Validity。

```rust
let point_id: String = sqlx::query_scalar("SELECT id FROM points LIMIT 1")
    .fetch_one(store.pool()).await.unwrap();
assert_eq!(PointId::from_str(&point_id).unwrap().to_string(), fragment.id.to_string());

let payload: String = sqlx::query_scalar(
    "SELECT payload_json FROM consolidation_artifacts
     WHERE kind = 'procedure_experience'",
).fetch_one(store.pool()).await.unwrap();
let payload: Value = serde_json::from_str(&payload).unwrap();
assert_eq!(payload["sample_count"], 4);
assert_eq!(payload["success_count"], 2);
assert_eq!(payload["failure_count"], 2);
assert_eq!(payload["success_rate"], 0.5);
```

- [ ] **Step 2：运行测试并确认旧 SHA-256 ID 与缺失字段导致失败**

Run: `CARGO_INCREMENTAL=0 cargo test -p jc-memory-consolidation --test conservative_learning canonical_ids_and_recall_artifacts --offline`

Expected: FAIL，Point/Surface ID 不能解析为 UUID，Procedure/Semantic payload 缺少召回字段。

- [ ] **Step 3：实现规范 ID 与完整 Artifact**

基础 Point 直接使用 `event.fragment_id`。内部 Artifact 继续使用 `stable_id`；新增 `stable_protocol_id`，将 SHA-256 前 16 字节设置 RFC 4122 version/variant 位后使用 `Uuid::from_bytes` 输出字符串，供派生 Edge/Surface 使用。

```rust
pub(crate) fn stable_protocol_id(kind: &str, parts: &[&str]) -> String {
    let digest = stable_digest(kind, parts);
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    uuid::Uuid::from_bytes(bytes).to_string()
}
```

Procedure payload 写入 `success_count`、`failure_count`、`sample_count`、`success_rate`、`confidence_interval` 和保守 statement。Semantic payload 写入 `point_id`、`kind`、原始 `content`、派生 `confidence`、`validity` 和 `stable: true`；Evidence 仍只保存权威来源与统计样本。

- [ ] **Step 4：运行 Consolidation 全测和静态门禁**

Run: `CARGO_INCREMENTAL=0 cargo test -p jc-memory-consolidation --all-targets --all-features --offline`

Run: `cargo fmt --all -- --check && CARGO_INCREMENTAL=0 cargo clippy -p jc-memory-consolidation --all-targets --all-features --offline -- -D warnings && git diff --check`

Expected: 全部退出码 0。

- [ ] **Step 5：提交并推送 Task 1**

```bash
git add crates/memory-consolidation
git commit -m "fix(consolidation): 规范派生标识与召回载荷" \
  -m "让 Point、Edge 与 Surface 使用协议可解析 UUID，并保持基础 Point 与 Core Projector 身份一致。" \
  -m "补齐 Procedure 和 Semantic Artifact 的计数、值、置信度、Validity 与 Evidence 字段。"
git push
```

### Task 2：增加 Core Job 授权门面

**Files:**
- Modify: `crates/memory-core/Cargo.toml`
- Modify: `crates/memory-core/src/engine.rs`
- Modify: `crates/memory-core/src/lib.rs`
- Create: `crates/memory-core/src/consolidation.rs`
- Create: `crates/memory-core/tests/consolidation_jobs.rs`

- [ ] **Step 1：写提交、查询和 Tenant 授权红灯测试**

测试要求：缺少 `Consolidate` Grant 时提交被拒绝；合法上下文可提交和查询；Tenant A 上下文即使知道 Job ID 也不能查询 Tenant B Job。

```rust
let error = engine.submit_consolidation(&recall_only, spec(scope.clone())).await.unwrap_err();
assert!(matches!(error, MemoryError::AccessDenied {
    operation: Operation::Consolidate,
    reason: DenyReason::MissingGrant,
}));

let record = engine.submit_consolidation(&context_b, spec(scope_b)).await.unwrap();
assert!(matches!(
    engine.consolidation_job(&context_a, record.id).await,
    Err(MemoryError::AccessDenied { operation: Operation::Consolidate, .. })
));
```

- [ ] **Step 2：运行测试并确认 Core API 缺失**

Run: `CARGO_INCREMENTAL=0 cargo test -p jc-memory-core --test consolidation_jobs submit_and_get --offline`

Expected: FAIL，`submit_consolidation` 和 `consolidation_job` 不存在。

- [ ] **Step 3：接入 Job Store、错误和公共类型**

为 Core 增加 `jc-memory-consolidation` 依赖。`MemoryEngine` 在 `open_inner` 中用同一 `SqliteStore` 构造 `ConsolidationJobStore`，并持有不含敏感信息的进程 Worker ID。`MemoryError` 增加：

```rust
#[error("memory consolidation failed: {0}")]
Consolidation(#[from] jc_memory_consolidation::JobError),
```

`submit_consolidation` 授权 `spec.scope`；`consolidation_job` 先按 ID 加载记录，再授权记录中的 Scope。Core `lib.rs` 导出 `JobId`、`JobKind`、`JobRecord`、`JobSpec`、`JobStatus` 和 `ResourceLimits`。

- [ ] **Step 4：运行授权与 Core 回归测试**

Run: `CARGO_INCREMENTAL=0 cargo test -p jc-memory-core --test consolidation_jobs --test tenant_isolation --offline`

Run: `cargo fmt --all -- --check && CARGO_INCREMENTAL=0 cargo clippy -p jc-memory-core --all-targets --all-features --offline -- -D warnings`

Expected: 全部退出码 0。

- [ ] **Step 5：提交并推送 Task 2**

```bash
git add crates/memory-core
git commit -m "feat(core): 增加巩固任务授权门面" \
  -m "MemoryEngine 复用同一 SQLite Job Store，并在提交和查询前执行精确 Scope Consolidate 授权。" \
  -m "公开稳定 Job 类型和结构化错误，不让调用方通过自报 Scope 访问其他 Tenant 任务。"
git push
```

### Task 3：接通 Job 生命周期与十二类 Handler 分派

**Files:**
- Modify: `crates/memory-consolidation/src/job.rs`
- Modify: `crates/memory-core/src/consolidation.rs`
- Modify: `crates/memory-core/tests/consolidation_jobs.rs`

- [ ] **Step 1：写生命周期、分派与重启红灯测试**

用表驱动测试分别提交十二类 Job 并调用 `run_consolidation`，断言记录保留原 `JobKind` 且进入 `Completed`。另验证 Pause/Resume、Cancel、Retry 非法状态，以及 Core 关闭重开后仍能查询并继续 Pending Job。

```rust
for kind in ALL_JOB_KINDS {
    let record = engine.submit_consolidation(&context, spec_for(&scope, kind)).await.unwrap();
    let completed = engine.run_consolidation(&context, record.id).await.unwrap();
    assert_eq!(completed.spec.kind, kind);
    assert_eq!(completed.status, JobStatus::Completed);
}
```

- [ ] **Step 2：运行测试并确认生命周期方法和分派缺失**

Run: `CARGO_INCREMENTAL=0 cargo test -p jc-memory-core --test consolidation_jobs lifecycle_and_dispatch --offline`

Expected: FAIL，Core 生命周期方法缺失，Store `cancel` 仍不可公开调用。

- [ ] **Step 3：实现最小生命周期门面与 Handler 映射**

将 `ConsolidationJobStore::cancel` 改为有文档的公共方法。Core 的 pause/resume/retry/cancel 全部先加载 Job、按其 Scope 授权，再调用 Store。`run_consolidation` 同样先授权，然后按 `JobKind` 构造对应 `JobRunner`：

```rust
match record.spec.kind {
    JobKind::Replay => self.run_task(record.id, Replay).await,
    JobKind::Deduplication => self.run_task(record.id, Deduplication).await,
    JobKind::EdgeReinforcement => self.run_task(record.id, EdgeReinforcement).await,
    JobKind::EdgeWeakening => self.run_task(record.id, EdgeWeakening).await,
    JobKind::SurfaceDiscovery => self.run_task(record.id, SurfaceDiscovery).await,
    JobKind::SurfaceMerge => self.run_task(record.id, SurfaceMerge).await,
    JobKind::SurfaceSplit => self.run_task(record.id, SurfaceSplit).await,
    JobKind::SemanticPromotion => self.run_task(record.id, SemanticPromotion).await,
    JobKind::ConflictDetection => self.run_task(record.id, ConflictDetection).await,
    JobKind::StaleStateDetection => self.run_task(record.id, StaleStateDetection).await,
    JobKind::ColdTierMovement => self.run_task(record.id, ColdTierMovement).await,
    JobKind::Rebuild => self.run_task(record.id, Rebuild).await,
}
```

- [ ] **Step 4：运行 Core 与 Consolidation 聚焦测试**

Run: `CARGO_INCREMENTAL=0 cargo test -p jc-memory-core --test consolidation_jobs --offline`

Run: `CARGO_INCREMENTAL=0 cargo test -p jc-memory-consolidation --test job_recovery --offline`

Run: `cargo fmt --all -- --check && CARGO_INCREMENTAL=0 cargo clippy -p jc-memory-core -p jc-memory-consolidation --all-targets --all-features --offline -- -D warnings && git diff --check`

Expected: 全部退出码 0。

- [ ] **Step 5：提交并推送 Task 3**

```bash
git add crates/memory-consolidation/src/job.rs crates/memory-core/src/consolidation.rs crates/memory-core/tests/consolidation_jobs.rs
git commit -m "feat(core): 接通巩固任务生命周期" \
  -m "按持久化 JobKind 分派十二类既有 Handler，并复用 Lease、Checkpoint、预算与重试状态机。" \
  -m "为暂停、恢复、重试、取消和重启查询保留精确 Scope 授权与失败关闭语义。"
git push
```

### Task 4：实现 Procedure 与 Knowledge 派生读取

**Files:**
- Create: `crates/memory-storage/src/projection_derivations.rs`
- Modify: `crates/memory-storage/src/projection.rs`
- Modify: `crates/memory-storage/src/lib.rs`
- Create: `crates/memory-storage/tests/recall_derivations.rs`

- [ ] **Step 1：写 Procedure、Constraint、Preference 和损坏行红灯测试**

在临时 SQLite 中写入两个独立 Verified 来源及对应 Artifact，要求精确 Scope 加载为强类型 View；重复 Event、跨 Scope Event、计数不一致、Prediction 支撑稳定知识和非法 Point ID 必须返回错误。

```rust
let projected = projection.load_recall_derivations(
    &scope,
    &[constraint_point, preference_point],
    &[first_fragment, second_fragment],
    Utc::now(),
    8,
).await.unwrap();
assert_eq!(projected.procedures[0].sample_count, 2);
assert_eq!(projected.constraints[0].kind, KnowledgeKind::Constraint);
assert_eq!(projected.preferences[0].kind, KnowledgeKind::Preference);
```

- [ ] **Step 2：运行测试并确认派生读取入口缺失**

Run: `CARGO_INCREMENTAL=0 cargo test -p jc-memory-storage --test recall_derivations procedure_and_knowledge --offline`

Expected: FAIL，`load_recall_derivations` 与 `ProjectedRecallDerivations` 不存在。

- [ ] **Step 3：实现强类型读取和 Evidence 校验**

新增 `ProjectedRecallDerivations`，并让 `SqliteProjectionStore::load_recall_derivations` 把内部 pool 交给新模块。所有 SQL 以 `scope_key = ?1` 开始并参数化 ID 集合；解析 Artifact payload 后重新计算 Procedure 计数和成功率，校验 Semantic 的 Point、Validity、内容和独立来源门槛。Evidence Event 必须存在、指向声明的 Fragment 且属于同一 Scope。

- [ ] **Step 4：运行 Storage 测试和静态门禁**

Run: `CARGO_INCREMENTAL=0 cargo test -p jc-memory-storage --test recall_derivations --test projection_schema --offline`

Run: `cargo fmt --all -- --check && CARGO_INCREMENTAL=0 cargo clippy -p jc-memory-storage --all-targets --all-features --offline -- -D warnings && git diff --check`

Expected: 全部退出码 0。

- [ ] **Step 5：提交并推送 Task 4**

```bash
git add crates/memory-storage/src crates/memory-storage/tests/recall_derivations.rs
git commit -m "feat(storage): 读取可追溯经验派生项" \
  -m "按精确 Scope 和激活集合解码 Procedure、Constraint 与 Preference，并重新校验计数和独立证据。" \
  -m "跨 Tenant 引用、非法强类型标识和损坏 Artifact 均失败关闭，不返回部分经验结果。"
git push
```

### Task 5：实现相关 Surface 的时间与预算读取

**Files:**
- Modify: `crates/memory-storage/src/projection_derivations.rs`
- Modify: `crates/memory-storage/tests/recall_derivations.rs`

- [ ] **Step 1：写 Surface 相关性、时间、状态和截断红灯测试**

构造 active、frozen、inactive、merged、split 和未来版本。只返回查询时刻有效、成员与激活 Point 相交的 active/frozen 最新版本；完整 Surface 超过剩余节点预算时不返回半个版本并设置 `truncated`。

```rust
assert_eq!(projected.surfaces.len(), 2);
assert!(projected.surfaces.iter().all(|surface| {
    matches!(surface.status, SurfaceStatus::Active | SurfaceStatus::Frozen)
}));
assert!(limited.surfaces.is_empty());
assert!(limited.truncated);
```

- [ ] **Step 2：运行测试并确认 Surface 尚未加载**

Run: `CARGO_INCREMENTAL=0 cargo test -p jc-memory-storage --test recall_derivations surface --offline`

Expected: FAIL，结果没有 Surface 或未执行时间、状态、成员预算过滤。

- [ ] **Step 3：实现 Surface 查询、排序和强类型解码**

先按 Scope、有效区间和状态选择每个 ID 的最新版本，再要求 `EXISTS` 激活成员。批量加载成员，解析 `SurfaceId`/`PointId`，校验成员 Point 同 Scope 和 Evidence 来源，按交集权重、置信度和 ID 排序。扣减 Surface 本身与成员节点预算，不能返回部分 Surface。

- [ ] **Step 4：运行完整派生读取测试与 Clippy**

Run: `CARGO_INCREMENTAL=0 cargo test -p jc-memory-storage --test recall_derivations --offline`

Run: `cargo fmt --all -- --check && CARGO_INCREMENTAL=0 cargo clippy -p jc-memory-storage --all-targets --all-features --offline -- -D warnings && git diff --check`

Expected: 全部退出码 0。

- [ ] **Step 5：提交并推送 Task 5**

```bash
git add crates/memory-storage/src/projection_derivations.rs crates/memory-storage/tests/recall_derivations.rs
git commit -m "feat(storage): 加载相关认知 Surface" \
  -m "只返回与激活 Point 相交且时间和生命周期有效的最新 Surface 版本，并批量校验成员与 Evidence。" \
  -m "完整 Surface 共享 Recall 节点预算，容量不足时停止后续读取并显式标记截断。"
git push
```

### Task 6：把经验与 Surface 接入 Core Memory Packet

**Files:**
- Modify: `crates/memory-core/src/recall.rs`
- Create: `crates/memory-core/tests/experience_recall.rs`

- [ ] **Step 1：写完整 Packet 与统一预算红灯测试**

通过 Core 写入权威 Fixture，运行 Replay、SurfaceDiscovery 和 SemanticPromotion Job，再 Recall。断言 Procedure、Constraint、Preference、Surface 和相关 Failure 进入 Packet，Evidence 可回溯，`recommended_context` 包含这些字段；低 `max_nodes` 时按固定顺序截断。

```rust
let packet = engine.recall(&context, query(scope.clone(), "deploy.guard")).await.unwrap();
assert_eq!(packet.procedures[0].procedure_key, "deploy.guard");
assert_eq!(packet.constraints.len(), 1);
assert_eq!(packet.preferences.len(), 1);
assert_eq!(packet.activated_surfaces.len(), 1);
assert_eq!(packet.past_failures.len(), 1);
assert!(packet.recommended_context.contains("deploy.guard"));
```

- [ ] **Step 2：运行测试并确认 Packet 字段仍为空**

Run: `CARGO_INCREMENTAL=0 cargo test -p jc-memory-core --test experience_recall packet_contains_consolidated_memory --offline`

Expected: FAIL，`activated_surfaces`、`procedures`、`constraints` 或 `preferences` 为空。

- [ ] **Step 3：在统一 deadline 和节点预算内组装 Packet**

从激活结果收集 Point/Fragment ID，扣除 State 和 Point 后计算剩余预算；在同一 `with_recall_deadline` Future 内调用 `load_recall_derivations`。`build_packet` 合并派生 View 和 Evidence，`past_failures` 从已激活 Failure Episode 筛选；Coverage 合并派生截断并刷新确定性上下文。返回前把派生 Evidence 的 Fragment Scope 注册到现有安全校验集合。

- [ ] **Step 4：运行 Recall、状态、隔离和污染回归**

Run: `CARGO_INCREMENTAL=0 cargo test -p jc-memory-core --test experience_recall --test recall_bounds --test trusted_state_recall --test tenant_isolation --test poisoning --test quarantine_security --offline`

Run: `cargo fmt --all -- --check && CARGO_INCREMENTAL=0 cargo clippy -p jc-memory-core --all-targets --all-features --offline -- -D warnings && git diff --check`

Expected: 全部退出码 0。

- [ ] **Step 5：提交并推送 Task 6**

```bash
git add crates/memory-core/src/recall.rs crates/memory-core/tests/experience_recall.rs
git commit -m "feat(recall): 返回巩固经验与认知 Surface" \
  -m "在统一 deadline 和 max_nodes 内把 Procedure、Constraint、Preference、Surface 与相关 Failure 接入 Memory Packet。" \
  -m "所有派生 View 保留权威 Evidence，并在返回前继续执行精确 Scope 安全校验。"
git push
```

### Task 7：验证 Tenant 隔离、重启恢复与旧 ID 重建

**Files:**
- Modify: `crates/memory-core/tests/consolidation_jobs.rs`
- Modify: `crates/memory-core/tests/experience_recall.rs`
- Modify: `crates/memory-core/tests/snapshot_restore.rs`

- [ ] **Step 1：写跨 Tenant、重启和旧派生 ID 红灯验收**

两个 Tenant 使用相同 `procedure_key` 和相同正文时必须各自返回自己的 Evidence。关闭并重新打开 Engine 后 Job 和 Packet 保持一致。手工插入旧 64 字符 Point/Surface ID 时 Recall 必须失败；运行精确 Scope Rebuild 后强类型 ID 可解析、Packet 恢复且 `verify_authority_hash` 不变。

- [ ] **Step 2：运行验收并确认旧 ID 还不能恢复**

Run: `CARGO_INCREMENTAL=0 cargo test -p jc-memory-core --test experience_recall --test consolidation_jobs --test snapshot_restore legacy_consolidation_ids --offline`

Expected: 在旧 ID Fixture 上 FAIL，或 Rebuild 后仍存在非 UUID 派生行。

- [ ] **Step 3：补齐精确 Scope Rebuild 的规范化顺序**

复用现有 `Rebuild` Handler 与 Core `rebuild_derived` 权威哈希门禁：Rebuild 先删除目标 Scope 的旧 Artifact/Surface/Member/Edge/Point，再按 Event 重放规范 Point 与 FTS；随后由调用方显式提交 `SurfaceDiscovery`、`SemanticPromotion` 等所需 Job。Core 不隐式编排整条流水线。任一 Job 失败时不推进自身 Checkpoint，不得修改权威 Fragment、Event、EvidenceRef 或其他 Scope 行。

- [ ] **Step 4：运行 Core、Storage、Consolidation 全量聚焦测试**

Run: `CARGO_INCREMENTAL=0 cargo test -p jc-memory-core -p jc-memory-storage -p jc-memory-consolidation --all-targets --all-features --offline`

Run: `cargo fmt --all -- --check && CARGO_INCREMENTAL=0 cargo clippy -p jc-memory-core -p jc-memory-storage -p jc-memory-consolidation --all-targets --all-features --offline -- -D warnings && git diff --check`

Expected: 全部退出码 0。

- [ ] **Step 5：提交并推送 Task 7**

```bash
git add crates/memory-core/tests crates/memory-storage crates/memory-consolidation
git commit -m "test(core): 验证巩固经验恢复与隔离" \
  -m "覆盖相同经验键的跨 Tenant 隔离、Core 重启、Job Checkpoint 与 Snapshot/Restore 后的 Packet 一致性。" \
  -m "固定旧派生标识失败关闭和精确 Scope Rebuild 规范化，同时证明权威哈希保持不变。"
git push
```

### Task 8：更新文档并完成阶段门禁

**Files:**
- Modify: `docs/architecture/consolidation.md`
- Modify: `docs/protocol/memory-packet.md`
- Modify: `CHANGELOG.md`
- Modify: `docs/superpowers/plans/2026-08-02-consolidation-experience-recall-implementation.md`

- [ ] **Step 1：更新中文架构和协议真值说明**

记录 Core Job 门面、已接线 Packet 字段、相关性与预算、规范 ID 和重建边界。将 `memory-packet.md` 中“Core 尚未全部接入”的说明改为本阶段真实完成状态；仍明确远程 API、Web、Runtime 自动采集和外部平台未验证。

- [ ] **Step 2：运行全仓 Rust 门禁**

Run: `cargo fmt --all -- --check`

Run: `CARGO_INCREMENTAL=0 cargo clippy --workspace --all-targets --all-features --offline -- -D warnings`

Run: `CARGO_INCREMENTAL=0 cargo test --workspace --all-features --offline`

Run: `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps --offline`

Run: `git diff --check`

Expected: 全部退出码 0。

- [ ] **Step 3：检查占位实现、工作树和提交范围**

Run: `rg -n 'TODO|unimplemented!\(|todo!\(' crates/memory-core crates/memory-storage crates/memory-consolidation`

Expected: 没有本阶段新增占位实现。

Run: `git status --short`

Expected: 仅有本 Task 文档改动和用户原有 `sdk/java/bin/`、`sdk/kotlin/bin/`。

- [ ] **Step 4：勾选计划并提交验收记录**

把本计划所有已完成复选框改为 `[x]`，写明实际测试数量、当前主机、SQLite、Fixture/实机边界和已知警告。

```bash
git add docs/architecture/consolidation.md docs/protocol/memory-packet.md CHANGELOG.md \
  docs/superpowers/plans/2026-08-02-consolidation-experience-recall-implementation.md
git commit -m "docs(core): 记录巩固经验召回验收" \
  -m "勾选已完成 Task 并记录 Job 门面、派生 Packet、隔离、恢复与全仓 Rust 门禁。" \
  -m "明确当前结果只覆盖本机 Core 与 SQLite，不扩展为远程协议、Web 或外部平台实机验证。"
git push
```

- [ ] **Step 5：创建并推送阶段 Tag**

Run: `git tag -a core-consolidation-experience-recall -m "Core Consolidation 与经验召回闭环"`

Run: `git push origin core-consolidation-experience-recall`

Expected: Tag 指向 Task 8 验收提交，远端推送成功。
