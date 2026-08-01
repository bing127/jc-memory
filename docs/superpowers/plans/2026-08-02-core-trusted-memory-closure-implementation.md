# Core 可信记忆闭环 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将显式 State、Mutation 关系和精确 Scope Checkpoint 持久化到 SQLite，使 Recall 能在重启、重建和所有稳定服务边界上可靠返回当前状态、历史状态、原因、证据与冲突。

**Architecture:** Fragment、Event 和 EvidenceRef 继续通过现有 `MemoryRepository` 事务追加并保持权威；新增 `SqliteProjectionStore` 按精确 Scope 和 Event sequence 在独立 SQLite 事务中生成 State、Point、Edge 与 Checkpoint。Core 在 Recall deadline 内追平投影，再按 State key 或激活 Fragment 有界加载 State/Graph，构造同一 `MemoryPacket` 供 Embedded、REST、gRPC、Sidecar、MCP 和 Web 使用。

**Tech Stack:** Rust 1.97、Tokio、SQLx 0.9、SQLite WAL/FTS5、Serde/serde_json、Protobuf/Tonic、React 19、TypeScript 7、Fastify 5、Zod 4、Vitest 4、Playwright 1.62。

---

## 0. 执行约束与文件结构

本计划直接在当前 `feat/performance-observability` 分支执行，不创建 worktree。每个 Task 必须按 RED -> GREEN -> REFACTOR 顺序完成，只暂存该 Task 列出的文件；`sdk/java/bin/` 与 `sdk/kotlin/bin/` 始终保持未跟踪且不得提交。

核心文件职责：

- `crates/memory-protocol/src/fragment.rs`：跨边界共享的 `StateDeclaration` 值类型；
- `crates/memory-protocol/src/packet.rs`：向后兼容的 State 原因与来源 Event；
- `crates/memory-events/src/validation.rs`：权威写入前的 State metadata 解析和纯值校验；
- `crates/memory-storage/migrations/0006_trusted_memory_projection.sql`：State 扩展与精确 Scope Checkpoint；
- `crates/memory-storage/src/projection.rs`：单事务 Projector、确定性 ID、投影读取和故障注入；
- `crates/memory-core/src/projector.rs`：Recall deadline、Mutation 前置校验与 Storage Projector 的 Core 编排；
- `crates/memory-core/src/recall.rs`：持久化 Point/Edge/State 到 Memory Packet 的有界构包；
- `crates/memory-core/src/mutation.rs`：只追加 Mutation Event，不再维护进程内派生图；
- `crates/memory-storage/src/recovery.rs`：State/Graph 投影清理、重放和重建续跑；
- `web/frontend/src/pages/memory-lab-page.tsx`：显式 State 写入与按 key Recall；
- `web/frontend/src/components/memory-packet.tsx`：当前状态、历史时间线和冲突的可读展示；
- `web/e2e/control-plane.spec.ts`：Fixture 与真实 Rust Server 分层闭环。

每个提交使用以下格式：

```text
feat(scope): 中文主题

中文说明第一行。
中文说明第二行。
```

## Task 1：扩展向后兼容的 State 协议契约

**Files:**
- Modify: `crates/memory-protocol/src/fragment.rs`
- Modify: `crates/memory-protocol/src/packet.rs`
- Modify: `crates/memory-protocol/src/command.rs`
- Modify: `crates/memory-protocol/src/lib.rs`
- Modify: `crates/memory-protocol/src/protobuf.rs`
- Modify: `proto/jiangcheng/memory/v1/memory.proto`
- Test: `crates/memory-protocol/tests/proto_conversion.rs`
- Test: `crates/memory-protocol/tests/service_contract.rs`

- [ ] **Step 1：写 StateDeclaration、可选 Mutation State 和 StateView 新字段的失败测试**

在 `proto_conversion.rs` 构造包含 State 的 Correct 和 Recall Packet，断言 Rust -> Protobuf -> Rust 无损：

```rust
let declaration = StateDeclaration {
    key: "project.deployment_strategy".to_owned(),
    value: serde_json::json!("canary"),
    reason: "降低全量发布风险".to_owned(),
};
let state = StateView {
    state_id: StateId::new(),
    version_id: StateVersionId::new(),
    key: declaration.key.clone(),
    value: declaration.value.clone(),
    valid_from: timestamp("2026-08-02T10:00:00Z"),
    valid_to: None,
    validity: ValidityStatus::Verified,
    evidence: Vec::new(),
    reason: Some(declaration.reason.clone()),
    source_event_id: Some(EventId::new()),
};
packet.current_states.push(state);
```

同时将旧版、不含 `state` 的 Mutation JSON 反序列化为 `state == None`，并断言 `StateView` 的两个 `None` 字段不会出现在 canonical JSON 中。

- [ ] **Step 2：运行协议测试并确认 RED**

Run: `CARGO_INCREMENTAL=0 cargo test -p jc-memory-protocol --test proto_conversion --test service_contract`

Expected: FAIL，编译器报告 `StateDeclaration`、`StateView::reason`、`StateView::source_event_id` 或 `MutationCommand::state` 尚不存在。

- [ ] **Step 3：添加协议类型和 Serde 兼容属性**

在 `fragment.rs` 添加：

```rust
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct StateDeclaration {
    pub key: String,
    pub value: serde_json::Value,
    pub reason: String,
}
```

在两层 `MutationCommand` 后续共享的协议结构中使用：

```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
pub state: Option<StateDeclaration>,
```

在 `StateView` 添加：

```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
pub reason: Option<String>,
#[serde(default, skip_serializing_if = "Option::is_none")]
pub source_event_id: Option<EventId>,
```

- [ ] **Step 4：只追加 Protobuf field number 并补齐转换**

在 `memory.proto` 添加：

```protobuf
message StateDeclaration {
  string key = 1 [json_name = "key"];
  JsonValue value = 2 [json_name = "value"];
  string reason = 3 [json_name = "reason"];
}

message MutationCommand {
  string target_fragment_id = 1 [json_name = "targetFragmentId"];
  FragmentContent content = 2 [json_name = "content"];
  FragmentSource source = 3 [json_name = "source"];
  Perspective perspective = 4 [json_name = "perspective"];
  google.protobuf.Timestamp occurred_at = 5 [json_name = "occurredAt"];
  VerificationMethod verification_method = 6 [json_name = "verificationMethod"];
  string reason = 7 [json_name = "reason"];
  optional StateDeclaration state = 8 [json_name = "state"];
}
```

`StateView` 只追加 `optional string reason = 9` 和 `optional string source_event_id = 10`。在 `protobuf.rs` 添加 `state_declaration_to_proto/from_proto`，所有可选字段依次使用 `map` 和 `transpose`，禁止给缺失字段 invent fallback。

- [ ] **Step 5：运行 GREEN、格式化和严格 Clippy**

Run: `cargo fmt --all -- --check`

Run: `CARGO_INCREMENTAL=0 cargo test -p jc-memory-protocol --test proto_conversion --test service_contract`

Run: `CARGO_INCREMENTAL=0 cargo clippy -p jc-memory-protocol --all-targets --all-features -- -D warnings`

Expected: 全部 PASS；旧 JSON 不出现新增的 `null` 字段。

- [ ] **Step 6：提交协议契约**

```bash
git add crates/memory-protocol/src/fragment.rs crates/memory-protocol/src/packet.rs crates/memory-protocol/src/command.rs crates/memory-protocol/src/lib.rs crates/memory-protocol/src/protobuf.rs proto/jiangcheng/memory/v1/memory.proto crates/memory-protocol/tests/proto_conversion.rs crates/memory-protocol/tests/service_contract.rs
git diff --cached --check
git commit -m "feat(protocol): 增加显式状态契约" -m "追加 StateDeclaration、状态原因和来源 Event。" -m "保持旧 JSON 与 Protobuf 客户端向后兼容。"
```

## Task 2：在权威写入前验证 State metadata

**Files:**
- Modify: `crates/memory-events/src/validation.rs`
- Modify: `crates/memory-events/src/lib.rs`
- Modify: `crates/memory-core/tests/common/mod.rs`
- Test: `crates/memory-events/tests/state_metadata.rs`

- [ ] **Step 1：写合法、非法和非事实身份的失败测试**

创建表驱动测试覆盖：合法 `Fact/StateTransition/Correction`；空 key；257 字节 key；非法字符；空 reason；2049 字符 reason；超过 1 MiB 的 value；错误 JSON shape；Prediction/Inference/Plan。另证明 Quarantined 和 Invalidated Fact 的 metadata 仍可被结构化解析，但后续 Projector 不会为其创建 StateVersion。核心断言：

```rust
let declaration = validate_state_metadata(&fragment)
    .expect("合法 State metadata")
    .expect("应解析 State");
assert_eq!(declaration.key, "project.deployment_strategy");
assert_eq!(declaration.value, serde_json::json!("blue_green"));
```

同时添加 `state_fragment(scope, key, value, reason, occurred_at)` 测试工厂，只产生 `Fact + Unverified`，避免 Verified 无 Evidence 干扰 State 测试。

- [ ] **Step 2：运行测试并确认 RED**

Run: `CARGO_INCREMENTAL=0 cargo test -p jc-memory-events --test state_metadata`

Expected: FAIL，`validate_state_metadata` 与 State 专用错误分类不存在。

- [ ] **Step 3：实现纯值校验和稳定错误分类**

在 `validation.rs` 增加：

```rust
const MAX_STATE_KEY_BYTES: usize = 256;
const MAX_STATE_REASON_CHARS: usize = 2_048;
const MAX_STATE_VALUE_BYTES: usize = 1_048_576;

pub fn validate_state_metadata(
    fragment: &CognitiveFragment,
) -> Result<Option<StateDeclaration>, FragmentValidationError> {
    let Some(raw) = fragment.metadata.get("state") else {
        return Ok(None);
    };
    let declaration: StateDeclaration = serde_json::from_value(raw.clone())
        .map_err(|_| FragmentValidationError::InvalidStateShape)?;
    validate_state_declaration(fragment.kind, &declaration)?;
    Ok(Some(declaration))
}
```

`validate_state_declaration` 必须逐项执行：key 非空且字节数不超过 256；每个 byte 只能是 ASCII 字母、数字、`.`、`_`、`-`；reason trim 后非空且字符数不超过 2048；`serde_json::to_vec(value)` 不超过 1 MiB；Kind 仅允许 Fact、StateTransition、Correction。`validate_fragment` 在返回 `Ok(())` 前调用该函数；Quarantined/Invalidated 是否参与投影由 State Projector 的 `state_source_is_visible` 单独判断，校验层不得删除合法 metadata。

- [ ] **Step 4：运行 GREEN 与事件包门禁**

Run: `CARGO_INCREMENTAL=0 cargo test -p jc-memory-events --all-targets`

Run: `CARGO_INCREMENTAL=0 cargo clippy -p jc-memory-events --all-targets --all-features -- -D warnings`

Expected: 全部 PASS。

- [ ] **Step 5：提交 State 校验**

```bash
git add crates/memory-events/src/validation.rs crates/memory-events/src/lib.rs crates/memory-events/tests/state_metadata.rs crates/memory-core/tests/common/mod.rs
git diff --cached --check
git commit -m "feat(events): 校验显式状态元数据" -m "限制 State key、value、reason 与 Fragment Kind。" -m "保留 Quarantined 和 Invalidated metadata 的权威原文。"
```

## Task 3：扩展 SQLite State Schema 与 Checkpoint

**Files:**
- Create: `crates/memory-storage/migrations/0006_trusted_memory_projection.sql`
- Create: `crates/memory-storage/src/projection.rs`
- Modify: `crates/memory-storage/src/lib.rs`
- Modify: `crates/memory-storage/src/error.rs`
- Modify: `crates/memory-storage/Cargo.toml`
- Test: `crates/memory-storage/tests/migrations.rs`
- Test: `crates/memory-storage/tests/projection_schema.rs`

- [ ] **Step 1：写 Migration 与类型契约失败测试**

测试迁移后存在 `reason`、`source_event_id`、`projection_checkpoints` 及复合索引，并验证 `(scope_key, projector)` 唯一：

```rust
let columns = table_columns(store.pool(), "state_versions").await;
assert!(columns.contains(&"reason".to_owned()));
assert!(columns.contains(&"source_event_id".to_owned()));
assert!(table_exists(store.pool(), "projection_checkpoints").await);
```

再构造 `ProjectionCheckpoint`，要求 projector 只能是 `State` 或 `Graph`，sequence 为 `u64`。

- [ ] **Step 2：运行测试并确认 RED**

Run: `CARGO_INCREMENTAL=0 cargo test -p jc-memory-storage --test migrations --test projection_schema`

Expected: FAIL，迁移、表和投影类型不存在。

- [ ] **Step 3：创建兼容 Migration**

`0006_trusted_memory_projection.sql` 使用：

```sql
ALTER TABLE state_versions ADD COLUMN reason TEXT;
ALTER TABLE state_versions ADD COLUMN source_event_id TEXT REFERENCES events(id) ON DELETE RESTRICT;

CREATE TABLE projection_checkpoints (
    scope_key TEXT NOT NULL CHECK (length(scope_key) = 64),
    projector TEXT NOT NULL CHECK (projector IN ('state', 'graph')),
    sequence INTEGER NOT NULL CHECK (sequence >= 0),
    updated_at_us INTEGER NOT NULL,
    PRIMARY KEY (scope_key, projector)
) STRICT;

CREATE INDEX idx_state_versions_source_fragment
    ON state_versions(source_fragment_id);
CREATE INDEX idx_state_versions_source_event
    ON state_versions(source_event_id);
CREATE INDEX idx_state_versions_state_effective
    ON state_versions(state_id, valid_from_us DESC, valid_to_us, validity);
CREATE INDEX idx_points_scope_source_fragment
    ON points(scope_key, source_fragment_id);
```

- [ ] **Step 4：定义 Storage 投影边界**

在 `projection.rs` 定义并从 `lib.rs` 导出：

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProjectorKind { State, Graph }

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectionCheckpoint {
    pub projector: ProjectorKind,
    pub sequence: u64,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProjectionTarget {
    pub sequence: u64,
}

pub trait ProjectionFailpoint: Send + Sync {
    fn before_commit(
        &self,
        projector: ProjectorKind,
        next_sequence: u64,
    ) -> Result<(), StorageError>;
}

#[derive(Clone)]
pub struct SqliteProjectionStore {
    pool: SqlitePool,
    failpoint: Arc<dyn ProjectionFailpoint>,
}
```

`SqliteProjectionStore::target(scope)` 使用 `SELECT COALESCE(MAX(sequence), 0) FROM events WHERE scope_key = ?1` 得到一次 Recall 的固定追赶目标；Checkpoint 读取必须同时绑定 scope hash 与 projector。

新增 `StorageError::ProjectionEventCorrupt { projector, field, scope_hash }`、`StorageError::ProjectionCheckpointCorrupt { projector, scope_hash }`、`StorageError::ProjectionScopeMismatch { projector, scope_hash }` 和 `StorageError::StateChainCorrupt { field, scope_hash }`。错误文本只含 projector、字段名和 scope hash，不含 State value、Fragment 正文、API Key 或 Evidence note。

- [ ] **Step 5：运行 GREEN 与 SQL 检查**

Run: `CARGO_INCREMENTAL=0 cargo test -p jc-memory-storage --test migrations --test projection_schema`

Run: `CARGO_INCREMENTAL=0 cargo clippy -p jc-memory-storage --all-targets --all-features -- -D warnings`

Expected: 全部 PASS。

- [ ] **Step 6：提交 Schema**

```bash
git add crates/memory-storage/migrations/0006_trusted_memory_projection.sql crates/memory-storage/src/projection.rs crates/memory-storage/src/lib.rs crates/memory-storage/src/error.rs crates/memory-storage/Cargo.toml crates/memory-storage/tests/migrations.rs crates/memory-storage/tests/projection_schema.rs
git diff --cached --check
git commit -m "feat(storage): 增加状态投影结构" -m "扩展 StateVersion 并新增精确 Scope Checkpoint。" -m "为状态、Point 和 Event 来源查询补充索引。"
```

## Task 4：实现事务性 State Projector

**Files:**
- Modify: `crates/memory-storage/src/projection.rs`
- Test: `crates/memory-storage/tests/state_projection.rs`

- [ ] **Step 1：写初始状态、版本替换、同刻冲突和幂等 RED 测试**

使用真实 `SqliteMemoryRepository` 追加 `blue_green` 与 `canary` Event，再调用 State Projector。断言：当前值为 `canary`；旧版本 `valid_to == canary.occurred_at` 且为 Superseded；相同 sequence 重跑行数不变；同一时刻不同值双方 Disputed 且 `current_version_id IS NULL`。

用测试专用 SQL 构造 current pointer 指向已关闭版本和多个非 Disputed 当前版本，断言 `load_states` 返回 `StateChainCorrupt`，错误文本不包含 state value。

- [ ] **Step 2：写 Forget/Weaken/Contradict 与批次回滚 RED 测试**

测试 Mutation metadata 的 `target_fragment_id`：Forget 关闭版本并标记 Invalidated；Weaken 标记 Disputed；Contradict 携带同 key State 时写竞争版本；跨 Scope、key 不匹配和注入 failpoint 均不留下部分 State 行，Checkpoint 保持原值。

- [ ] **Step 3：运行 RED**

Run: `CARGO_INCREMENTAL=0 cargo test -p jc-memory-storage --test state_projection`

Expected: FAIL，`catch_up_state`、State 投影读取和 failpoint 尚未实现。

- [ ] **Step 4：实现确定性标识与单事务批次**

确定性规则固定为：`StateId` 使用该 key 首个合法 State Fragment 的 UUID；`StateVersionId` 使用来源 Fragment UUID；同一来源 Fragment 的 `PointId` 也使用该 Fragment UUID。转换统一通过强类型 `FromStr`，不得使用随机 ID。

核心批次必须保持以下结构：

```rust
pub async fn catch_up_state(
    &self,
    scope: &MemoryScope,
    target: ProjectionTarget,
    batch_size: u32,
) -> Result<ProjectionCheckpoint, StorageError> {
    let mut checkpoint = self.checkpoint(scope, ProjectorKind::State).await?;
    while checkpoint.sequence < target.sequence {
        checkpoint = self.project_state_batch(scope, target, batch_size).await?;
    }
    Ok(checkpoint)
}
```

`project_state_batch` 必须 `BEGIN IMMEDIATE`，在同一 transaction 内读取当前 Checkpoint 之后、目标 sequence 之前的 Event，按 sequence/id 排序，逐条调用 `apply_state_event`，执行 failpoint，再 UPSERT Checkpoint，最后 commit。错误路径显式 rollback。

- [ ] **Step 5：实现 State 事件状态机**

`apply_state_event` 按以下穷尽规则处理：

```rust
match (mutation_kind(fragment), validate_state_metadata(fragment)?) {
    (Some(StoredMutationKind::Forget), _) => invalidate_target(transaction, scope_key, fragment).await?,
    (Some(StoredMutationKind::Weaken), _) => dispute_target(transaction, scope_key, fragment).await?,
    (Some(StoredMutationKind::Contradict), Some(state)) => {
        dispute_target(transaction, scope_key, fragment).await?;
        insert_competing_version(transaction, scope_key, event, fragment, state).await?;
    }
    (Some(StoredMutationKind::Contradict), None) => dispute_target(transaction, scope_key, fragment).await?,
    (Some(StoredMutationKind::Correct | StoredMutationKind::Supersede), Some(state)) => {
        supersede_version(transaction, scope_key, event, fragment, state).await?;
    }
    (Some(StoredMutationKind::Correct | StoredMutationKind::Supersede), None) => {}
    (_, Some(state)) if state_source_is_visible(fragment) => {
        record_or_advance_version(transaction, scope_key, event, fragment, state).await?;
    }
    _ => {}
}
```

Quarantined、普通 Invalidated、Prediction、Inference 和 Plan 不创建 StateVersion。Forget/Weaken/Contradict 的目标 Fragment 存在但未映射到 StateVersion，且 Mutation 未携带 State metadata 时，State Projector 只做 no-op 并允许 Checkpoint 前进，Graph Projector 仍可记录关系；携带 State metadata 时按损坏 Event 拒绝。重复 `(state_id, value_json, valid_from_us, source_fragment_id)` 视为幂等；非法旧 Event 返回 `ProjectionEventCorrupt` 并阻止 Checkpoint 前进。

- [ ] **Step 6：实现 State 的有界读取契约**

在 `projection.rs` 定义：

```rust
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StateSelector {
    ExactKey(String),
    SourceFragments(Vec<FragmentId>),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProjectedStateSet {
    pub current: Vec<StateView>,
    pub history: Vec<StateView>,
    pub conflicts: Vec<ConflictView>,
    pub evidence: Vec<EvidenceView>,
    pub truncated: bool,
}

pub async fn load_states(
    &self,
    scope: &MemoryScope,
    selector: StateSelector,
    effective_at: DateTime<Utc>,
    max_nodes: usize,
) -> Result<ProjectedStateSet, StorageError>
```

查询必须绑定精确 `scope_key`；current 使用半开区间且排除 Superseded/Invalidated/Quarantined；history 仅返回 `valid_from_us < effective_at` 的相关版本并按时间/ID 倒序；Evidence 从 `source_fragment_id + source_event_id + evidence_refs.id` 组成并去重；同刻 Disputed 版本产生含确定性 PointId 的 `ConflictView`。达到 `max_nodes` 时截断并设置 `truncated = true`。版本链出现断裂、多个非 Disputed 当前版本或 current pointer 指向已关闭版本时返回 `StateChainCorrupt`。

- [ ] **Step 7：运行 GREEN、事务与并发测试**

Run: `CARGO_INCREMENTAL=0 cargo test -p jc-memory-storage --test state_projection`

Run: `CARGO_INCREMENTAL=0 cargo test -p jc-memory-storage --all-targets`

Expected: 全部 PASS；failpoint 前后 State 行数与 Checkpoint 均保持一致；两个独立 Store 同时投影同一 Scope 时由 `BEGIN IMMEDIATE` 串行化且不产生重复版本。

- [ ] **Step 8：提交 State Projector**

```bash
git add crates/memory-storage/src/projection.rs crates/memory-storage/tests/state_projection.rs
git diff --cached --check
git commit -m "feat(storage): 持久化状态版本投影" -m "按 Event sequence 生成当前状态、历史状态与争议版本。" -m "批次失败同时回滚派生行和 Checkpoint。"
```

## Task 5：实现持久化 Graph Projector 与有界加载

**Files:**
- Modify: `crates/memory-storage/Cargo.toml`
- Modify: `crates/memory-storage/src/projection.rs`
- Test: `crates/memory-storage/tests/graph_projection.rs`

- [ ] **Step 1：写 Point/Edge 确定性、重启和 Scope RED 测试**

追加 Correct、Reinforce、Weaken、Supersede、Contradict Event，断言 Point ID 等于来源 Fragment UUID、Edge ID 等于 Mutation Event UUID；关闭并重开 Store 后按 Scope/seed/EdgeKind 加载结果相同。共同出现不得生成 Causes，跨 Scope 端点必须失败且 Checkpoint 不推进。

- [ ] **Step 2：运行 RED**

Run: `CARGO_INCREMENTAL=0 cargo test -p jc-memory-storage --test graph_projection`

Expected: FAIL，Graph Projector 和 `load_graph` 尚不存在。

- [ ] **Step 3：实现 Graph 投影**

为 `memory-storage` 增加 `jc-memory-graph` 依赖。`catch_up_graph` 与 State Projector 使用相同事务模板，但独立保存 `projector = 'graph'` 的 Checkpoint。

每个可映射 Fragment 先 UPSERT 确定性 Point；Mutation 的 `relation` 非空时，解析同 Scope 目标 Fragment，保证两个 Point 存在，再写 Edge：source 为 Mutation Point、target 为目标 Point、source_fragment_id 为 Mutation Fragment。`Manual` Evidence 标记 `human_confirmed = 1`，`Tool` 标记 `tool_verified = 1`，其他方法均为 0。

- [ ] **Step 4：实现有界精确 Scope 读取**

先定义 Storage 返回值：

```rust
#[derive(Clone, Debug, PartialEq)]
pub struct ProjectedPoint {
    pub point: CognitivePoint,
    pub text: String,
    pub entities: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProjectedGraph {
    pub points: Vec<ProjectedPoint>,
    pub edges: Vec<CognitiveEdge>,
}
```

添加：

```rust
pub async fn load_graph(
    &self,
    scope: &MemoryScope,
    seed_fragments: &[FragmentId],
    allowed_kinds: &[EdgeKind],
    effective_at: DateTime<Utc>,
    max_nodes: usize,
) -> Result<ProjectedGraph, StorageError>
```

SQL 必须同时绑定 `scope_key`、seed Point、允许的 EdgeKind、`valid_from_us <= effective_at` 和半开区间 `valid_to_us IS NULL OR effective_at < valid_to_us`。先限制 Edge，再收集端点 Point，总 Point 数不得超过 `max_nodes`；返回前再次验证每个 Point/Edge scope。

- [ ] **Step 5：运行 GREEN 与严格 Clippy**

Run: `CARGO_INCREMENTAL=0 cargo test -p jc-memory-storage --test graph_projection`

Run: `CARGO_INCREMENTAL=0 cargo clippy -p jc-memory-storage --all-targets --all-features -- -D warnings`

Expected: 全部 PASS。

- [ ] **Step 6：提交 Graph Projector**

```bash
git add crates/memory-storage/Cargo.toml crates/memory-storage/src/projection.rs crates/memory-storage/tests/graph_projection.rs
git diff --cached --check
git commit -m "feat(storage): 持久化记忆关系投影" -m "以确定性 Point 和 Edge 标识替代进程内关系。" -m "按 Scope、有效时间和 Recall 预算有界加载。"
```

## Task 6：将 Core Recall 接入持久化投影

**Files:**
- Create: `crates/memory-core/src/projector.rs`
- Modify: `crates/memory-core/src/lib.rs`
- Modify: `crates/memory-core/src/engine.rs`
- Modify: `crates/memory-core/src/recall.rs`
- Modify: `crates/memory-core/src/security.rs`
- Test: `crates/memory-core/tests/trusted_state_recall.rs`
- Test: `crates/memory-core/tests/tenant_isolation.rs`
- Test: `crates/memory-core/tests/recall_bounds.rs`

- [ ] **Step 1：写 `blue_green -> canary` 当前/历史/at_time RED 测试**

通过 `MemoryEngine::observe` 写两条显式 State Fragment，再分别执行 CurrentState、HistoricalState 和普通 Semantic Recall。断言当前值、历史值、valid_from/valid_to、reason、source_event_id 与 Evidence ID；`at_time == None` 使用当前 UTC 时间，显式时间使用 `[valid_from, valid_to)`。

同一测试文件保留原始兼容场景：将“项目计划从 PostgreSQL 迁移到 MySQL”作为普通 Fact 文本 Observe，并用 Exact Recall 召回正文；同时检查临时数据目录中仍只有 `memory.sqlite3`，证明这两个词是被记忆的业务内容而不是存储引擎配置。

- [ ] **Step 2：写冲突、Tenant 隔离和 max_nodes RED 测试**

相同时刻写两个值，要求 `current_states.len() == 2` 且 `conflicts` 非空；两个 Tenant 使用相同 key 不得交叉；历史版本与图端点总量受 `max_nodes` 限制，超限时 `coverage.truncated == true`。

- [ ] **Step 3：运行 RED**

Run: `CARGO_INCREMENTAL=0 cargo test -p jc-memory-core --test trusted_state_recall --test tenant_isolation --test recall_bounds`

Expected: FAIL，当前 `MemoryPacket.current_states/historical_states` 为空，关系仍来自 `derived_graph`。

- [ ] **Step 4：在 Engine 中持有共享 ProjectionStore**

`MemoryEngine::open_inner` 从同一个 `SqliteStore` 构造 `SqliteProjectionStore`。在 `projector.rs` 添加 deadline 感知的追平：

```rust
pub(crate) async fn catch_up_projections(
    &self,
    scope: &MemoryScope,
    deadline: Instant,
) -> Result<ProjectionTarget, MemoryError> {
    let target = self.projection.target(scope).await?;
    with_deadline(deadline, self.projection.catch_up_state(scope, target, 128)).await?;
    with_deadline(deadline, self.projection.catch_up_graph(scope, target, 128)).await?;
    Ok(target)
}
```

`with_deadline` 在同一文件中定义为 `tokio::time::timeout_at(deadline.into(), future)` 的薄封装，超时只返回 `MemoryError::ProjectionTimedOut`；不得复用会把错误误分类为 RecallEngine 超时的 helper。

新增 `MemoryError::ProjectionTimedOut`、`ProjectionCheckpointCorrupt`、`ProjectionScopeMismatch` 和 `StateChainCorrupt`，CommandBus 分别映射为 ResourceExhausted、Internal、Internal、Internal。错误映射测试必须断言公开正文不包含 `blue_green`、`canary`、Fragment 正文或测试 API Key。

- [ ] **Step 5：重写 Recall 数据装配**

在 ACL 后、FTS 前调用 `catch_up_projections`。删除 `derived_graph.snapshot()`，改为 `load_graph`，并直接复用 `jc_memory_activation::edge_kinds_for(query.mode)` 作为允许关系白名单。State 选择规则固定为：CurrentState/HistoricalState 先把完整 `query.text` 当 key；精确 key 未命中或普通 Recall 时，只读取已激活 Fragment 的 StateVersion。

`build_packet` 新增 `ProjectedStateSet` 参数并执行：

```rust
packet.current_states = states.current;
packet.historical_states = states.history;
packet.conflicts.extend(states.conflicts);
packet.evidence.extend(states.evidence);
packet.coverage.evidence_count = packet.evidence.len();
packet.coverage.truncated |= states.truncated;
packet.refresh_recommended_context()?;
```

证据去重键使用 `(fragment_id, event_id, evidence_ref_id)`；返回前的 `validate_packet` 必须覆盖 StateView 与 ConflictView 的所有来源 Fragment/Event。

- [ ] **Step 6：运行 GREEN 和 Core 门禁**

Run: `CARGO_INCREMENTAL=0 cargo test -p jc-memory-core --test trusted_state_recall --test tenant_isolation --test recall_bounds`

Run: `CARGO_INCREMENTAL=0 cargo clippy -p jc-memory-core --all-targets --all-features -- -D warnings`

Expected: 全部 PASS；普通 Recall 不扫描并返回整个 Scope 的 State。

- [ ] **Step 7：提交 Core Recall**

```bash
git add crates/memory-core/src/projector.rs crates/memory-core/src/lib.rs crates/memory-core/src/engine.rs crates/memory-core/src/recall.rs crates/memory-core/src/security.rs crates/memory-core/tests/trusted_state_recall.rs crates/memory-core/tests/tenant_isolation.rs crates/memory-core/tests/recall_bounds.rs
git diff --cached --check
git commit -m "feat(core): 接通可信状态召回" -m "Recall 在 deadline 内追平 State 与 Graph Checkpoint。" -m "返回当前状态、历史版本、原因、证据和可见冲突。"
```

## Task 7：补齐 Mutation 状态语义并移除进程内图

**Files:**
- Modify: `crates/memory-core/src/commands.rs`
- Modify: `crates/memory-core/src/command_bus.rs`
- Modify: `crates/memory-core/src/engine.rs`
- Modify: `crates/memory-core/src/mutation.rs`
- Modify: `crates/memory-protocol/src/command.rs`
- Test: `crates/memory-core/tests/memory_api.rs`
- Test: `crates/memory-core/tests/command_bus.rs`
- Test: `crates/memory-core/tests/trusted_state_recall.rs`

- [ ] **Step 1：写前置拒绝与 Mutation 状态转移 RED 测试**

覆盖：Correct/Supersede/Contradict 的 state key 与目标不一致时返回 InvalidArgument，且 Stats 的 Fragment/Event 数不变；目标不是 State 却携带 state 时不追加权威记录；Forget 关闭目标 State 但原 Fragment/Event/EvidenceRef 仍可按 ID 读取；Weaken/Contradict 产生 Disputed 和持久化关系。关闭并重新打开 `MemoryEngine` 后再次 Recall/Explain，State 与 Mutation Edge ID 必须和关闭前一致。

- [ ] **Step 2：运行 RED**

Run: `CARGO_INCREMENTAL=0 cargo test -p jc-memory-core --test memory_api --test command_bus --test trusted_state_recall`

Expected: FAIL，Core Mutation 尚未携带 StateDeclaration，也未执行目标 key 前置校验。

- [ ] **Step 3：透传 StateDeclaration 并执行写前校验**

Core `MutationCommand` 添加 `state: Option<StateDeclaration>`。`core_mutation` 无损透传。`apply_mutation` 在调用 `observe_admitted` 前：同 Scope 读取目标 Fragment；解析目标 `metadata.state`；当 command.state 为 Some 时要求目标 Kind/Validity 可投影、State metadata 存在且 key 完全一致，否则返回不含 value 的 `MemoryError::InvalidStateMutation { reason }`。

`mutation_fragment` 继续写 `mutation/target_fragment_id/relation`，并仅在 `command.state` 为 Some 时插入：

```rust
metadata.insert(
    "state".to_owned(),
    serde_json::to_value(state).expect("StateDeclaration 始终可以序列化"),
);
```

- [ ] **Step 4：删除 `DerivedGraph` 和即时随机 Edge**

删除 `MemoryEngine::derived_graph`、`DerivedGraph`、`record_relation` 与 `CognitiveEdge::new` 即时写入。Mutation receipt 的 relation 仍由稳定 MutationSpec 返回；真实关系由 Graph Projector 从权威 Event 重建。重启前后 Recall 必须走同一 SQLite 路径。

- [ ] **Step 5：运行 GREEN 与无残留检查**

Run: `CARGO_INCREMENTAL=0 cargo test -p jc-memory-core --test memory_api --test command_bus --test trusted_state_recall`

Run: `rg -n "DerivedGraph|derived_graph" crates/memory-core`

Expected: 测试 PASS；`rg` 无匹配并返回 1。

- [ ] **Step 6：提交 Mutation 闭环**

```bash
git add crates/memory-core/src/commands.rs crates/memory-core/src/command_bus.rs crates/memory-core/src/engine.rs crates/memory-core/src/mutation.rs crates/memory-protocol/src/command.rs crates/memory-core/tests/memory_api.rs crates/memory-core/tests/command_bus.rs crates/memory-core/tests/trusted_state_recall.rs
git diff --cached --check
git commit -m "fix(core): 完整投影记忆变更" -m "写入前拒绝 State 目标缺失、跨 Scope 和 key 不匹配。" -m "Forget、Weaken 与 Contradict 仅修改可重建投影。"
```

## Task 8：让 Rebuild、Snapshot 和 Restore 覆盖新投影

**Files:**
- Modify: `crates/memory-storage/src/recovery.rs`
- Modify: `crates/memory-core/src/maintenance.rs`
- Test: `crates/memory-storage/tests/recovery.rs`
- Test: `crates/memory-core/tests/snapshot_restore.rs`

- [ ] **Step 1：写投影重建、续跑和权威哈希 RED 测试**

创建两版 State 和 Contradicts Edge，记录 Fragment/Event/EvidenceRef canonical hash；清空派生表后执行 `rebuild_derived`，断言 State/Edge/Checkpoint 恢复且权威 hash 不变。Failpoint 在 StateProjection 后中断，第二次从该阶段之后继续。另用两个 `RecoveryManager` 并发重建同一 Scope，第二个必须返回稳定的 `RebuildLockUnavailable`，不同 Scope 可以并行。对纯函数 `verify_authority_hash(before, after)` 输入不同摘要，断言返回 `RebuildAuthorityChanged`。

- [ ] **Step 2：写 Snapshot/Restore RED 测试**

Snapshot 后在当前库追加第三版，Restore 原 Snapshot；断言恢复后的当前/历史 State 与 Snapshot 时一致，并可再次 Rebuild 得到相同投影。

- [ ] **Step 3：运行 RED**

Run: `CARGO_INCREMENTAL=0 cargo test -p jc-memory-storage --test recovery`

Run: `CARGO_INCREMENTAL=0 cargo test -p jc-memory-core --test snapshot_restore`

Expected: FAIL，`RebuildStage` 尚不包含 StateProjection/GraphProjection，Rebuild 未清理和重放投影。

- [ ] **Step 4：扩展固定重建顺序**

固定顺序改为：

```rust
pub const ORDER: [Self; 7] = [
    Self::StateProjection,
    Self::GraphProjection,
    Self::Fts,
    Self::Embedding,
    Self::EdgeCandidate,
    Self::SurfaceMember,
    Self::SummaryCache,
];
```

StateProjection 开始前在一个 Scope 事务内：先清空 `states.current_version_id`，再删除该 Scope 的 Edge、Point、StateVersion、State 和两个 Projection Checkpoint；随后从 sequence 0 重放到当前目标。GraphProjection 复用同一权威目标。不得删除 Fragment/Event/EvidenceRef。

进入重建前，Core 先完成 `Operation::Consolidate` 的精确 Scope 授权。Recovery 使用 `.maintenance/rebuild-<scope_key>.lock` 和 `fs2::FileExt::try_lock_exclusive` 获取跨进程 Lease；所有成功、错误和取消路径都释放 Lease。锁文件名只包含 scope hash，不包含 Namespace/Tenant 原文。

在 `RecoveryError` 增加独立的 `RebuildLockUnavailable`，不得复用 GC 的 `LockUnavailable`，这样日志和上层错误可以准确区分两个维护任务。

重建开始前后分别计算仅覆盖 Fragment/Event/EvidenceRef 的 canonical authority hash；不一致时返回 `RecoveryError::RebuildAuthorityChanged`，不把异常结果标记为完成，也不删除可诊断的维护 Checkpoint。

- [ ] **Step 5：校验快照包含与恢复派生层**

保持 Snapshot 的 SQLite 一致镜像机制，不另写旁路文件。Restore 后打开 Core 时读取现有 Checkpoint；显式 Rebuild 仍应产生等价 State/Graph。将 State/Graph 派生引用加入恢复后的 integrity check。

- [ ] **Step 6：运行 GREEN**

Run: `CARGO_INCREMENTAL=0 cargo test -p jc-memory-storage --test recovery`

Run: `CARGO_INCREMENTAL=0 cargo test -p jc-memory-core --test snapshot_restore`

Expected: 全部 PASS，权威哈希逐项相同。

- [ ] **Step 7：提交恢复闭环**

```bash
git add crates/memory-storage/src/recovery.rs crates/memory-core/src/maintenance.rs crates/memory-storage/tests/recovery.rs crates/memory-core/tests/snapshot_restore.rs
git diff --cached --check
git commit -m "feat(recovery): 重建可信记忆投影" -m "按固定阶段重放 State、Graph 和 FTS。" -m "Snapshot 与 Restore 保持权威哈希和投影一致。"
```

## Task 9：验证 Protobuf、服务和 SDK 契约一致性

**Files:**
- Modify: `sdk/contracts/fixtures/correct-basic.json`
- Modify: `sdk/contracts/fixtures/recall-current-state.json`
- Modify: `sdk/contracts/fixtures/recall-historical-state.json`
- Modify: `crates/memory-protocol/src/sdk_contract.rs`
- Modify: `crates/memory-protocol/tests/sdk_contract_fixtures.rs`
- Test: `services/memory-server/tests/protocol_parity.rs`
- Test: `services/memory-server/tests/server_binary_e2e.rs`
- Test: `services/memory-sidecar/tests/sidecar_contract.rs`
- Test: `services/memory-mcp/tests/mcp_contract.rs`

- [ ] **Step 1：写跨边界非空 State RED 测试**

同一 Fixture 使用 `project.deployment_strategy`，期望 current=`canary`、history=`blue_green`，并包含 reason/source_event_id。HTTP、gRPC、Sidecar、MCP 分别执行 Observe 两版与 Recall，比较规范化 `MemoryPacket`，不比较传输层 request_id。

- [ ] **Step 2：运行 RED**

Run: `CARGO_INCREMENTAL=0 cargo test -p jc-memory-server -p jc-memory-sidecar -p jc-memory-mcp`

Expected: FAIL，现有测试未覆盖非空 State 或 Fixture hash 尚未更新。

- [ ] **Step 3：更新版本化 Fixture 与校验器**

`correct-basic.json` 增加可选 `state`；两个 State Recall Fixture 使用非空 `current_states/historical_states`。`sdk_contract.rs` 对 State Fixture 强制校验 key、value、reason、source_event_id 和 Evidence ID，随后按现有 sorted-json 规则计算并写入真实 `canonical_sha256`。保留一份旧 Mutation JSON 反序列化测试，证明缺失 `state` 仍合法。

- [ ] **Step 4：运行跨适配器门禁**

Run: `CARGO_INCREMENTAL=0 cargo test -p jc-memory-protocol --test sdk_contract_fixtures`

Run: `CARGO_INCREMENTAL=0 cargo test -p jc-memory-server -p jc-memory-sidecar -p jc-memory-mcp`

Run: `bash scripts/test/sdk-contracts.sh`

Expected: Rust、Protobuf、已安装 SDK 和 Adapter 门禁 PASS；缺少的外部工具明确显示“跳过”，不得描述为实机通过。

- [ ] **Step 5：提交跨边界契约**

```bash
git add sdk/contracts/fixtures/correct-basic.json sdk/contracts/fixtures/recall-current-state.json sdk/contracts/fixtures/recall-historical-state.json crates/memory-protocol/src/sdk_contract.rs crates/memory-protocol/tests/sdk_contract_fixtures.rs services/memory-server/tests/protocol_parity.rs services/memory-server/tests/server_binary_e2e.rs services/memory-sidecar/tests/sidecar_contract.rs services/memory-mcp/tests/mcp_contract.rs
git diff --cached --check
git commit -m "test(contract): 固定可信状态跨端结果" -m "版本化 Current/Historical State 与 Correct Fixture。" -m "验证 Embedded、REST、gRPC、Sidecar 和 MCP 结果一致。"
```

## Task 10：在 Web Memory Lab 提供显式 State 工作流

**Files:**
- Modify: `web/backend/src/routes/memory.ts`
- Modify: `web/backend/tests/app.test.ts`
- Modify: `web/frontend/src/pages/memory-lab-page.tsx`
- Modify: `web/frontend/src/components/memory-packet.tsx`
- Modify: `web/frontend/src/index.css`
- Modify: `web/frontend/tests/memory-lab-page.test.tsx`

- [ ] **Step 1：写 BFF Schema 与前端交互 RED 测试**

BFF 测试 State key 字符集、JSON value、reason 长度和可选 Mutation state；断言 BFF 只绑定实例 Scope，不改 State 内容。前端测试 State 模式生成 `metadata.state`，Current/Historical 模式以 State key 作为 query text，并可读展示时间线、原因和 Evidence ID。

- [ ] **Step 2：运行 RED**

Run: `pnpm --dir web --filter @jiangcheng-memory/control-plane-backend test -- app.test.ts`

Run: `pnpm --dir web --filter @jiangcheng-memory/control-plane-frontend test -- memory-lab-page.test.tsx`

Expected: FAIL，State 表单、Zod Schema 和结构化 State 展示不存在。

- [ ] **Step 3：扩展 BFF 严格 Schema**

定义：

```ts
const stateSchema = z.strictObject({
  key: z.string().min(1).max(256).regex(/^[A-Za-z0-9._-]+$/),
  value: z.unknown(),
  reason: z.string().trim().min(1).max(2_048),
});
```

Observe/Commit 仍接受完整 Fragment，但当 `metadata.state` 存在时用 `stateSchema` 二次校验。Mutation body 添加 `state: stateSchema.nullable()`，向上游转换为 snake_case 协议字段且缺失时不发送。

- [ ] **Step 4：实现 State 表单和 Recall 时间线**

Observe 增加 Text/State segmented mode。State 模式字段固定为 State key、JSON value、Reason、Fragment Kind、Validity、发生时间；`JSON.parse` 失败时在字段下显示错误且不发请求。产品示例预填 key=`project.deployment_strategy`、value=`"blue_green"`，不再展示 PostgreSQL/MySQL 为存储迁移示例。

CurrentState/HistoricalState 选择后将 Query Text 标签改为 State key，隐藏 Entities。`MemoryPacketData` 将 State 从 `unknown[]` 改为显式接口；Current States 使用紧凑表格，Historical States 按 `valid_from` 倒序显示时间线，冲突使用现有警示色而不是闪动动画。

- [ ] **Step 5：运行 GREEN、类型检查和 Build**

Run: `pnpm --dir web test`

Run: `pnpm --dir web typecheck`

Run: `pnpm --dir web build`

Expected: 全部 PASS；桌面和移动端文本不溢出。

- [ ] **Step 6：提交 Web 工作流**

```bash
git add web/backend/src/routes/memory.ts web/backend/tests/app.test.ts web/frontend/src/pages/memory-lab-page.tsx web/frontend/src/components/memory-packet.tsx web/frontend/src/index.css web/frontend/tests/memory-lab-page.test.tsx
git diff --cached --check
git commit -m "feat(web): 增加显式状态工作流" -m "Memory Lab 支持 State 写入、当前值和历史时间线。" -m "BFF 严格校验输入并继续绑定服务端 Scope。"
```

## Task 11：完成 Fixture 与真实 Rust Server Web E2E

**Files:**
- Modify: `web/e2e/fixture-memory-server.mjs`
- Modify: `web/e2e/control-plane.spec.ts`
- Modify: `web/e2e/start-real-memory-server.mjs`
- Test: `web/e2e/control-plane.spec.ts`

- [ ] **Step 1：写 State 全链路和模式分层 RED 测试**

E2E 顺序固定为：Observe `blue_green` -> Correct `canary` -> CurrentState Recall -> HistoricalState Recall -> Explain -> Forget Correct 返回的 `canary` Fragment ID -> CurrentState 为空。测试必须从 Correct 响应读取新 `fragment_id`，不得对已经 Superseded 的 `blue_green` 版本执行 Forget。桌面与移动端都断言原因、时间和 Evidence ID。

定义 `const isRealServer = process.env.JC_WEB_E2E_REAL_SERVER === "1"`。`/__fixture/requests`、`/__fixture/metrics-mode` 和 Fixture 请求计数断言只允许在 `!isRealServer` 分支执行；真实模式不得访问 Fixture 专用端点。

- [ ] **Step 2：运行 Fixture E2E 并确认 RED**

Run: `pnpm --dir web test:e2e -- --project=chromium`

Expected: FAIL，Fixture 尚未维护 State 版本链。

- [ ] **Step 3：实现版本化 Fixture State**

Fixture 只作为版本化契约替身：按 `access_scope + state.key` 保存版本链；Observe/Correct/Forget 复现已批准的半开区间、Evidence 和 Invalidated 语义；不实现启发式文本抽取。Fixture 响应必须与真实 Rust Server 字段形状一致。

- [ ] **Step 4：运行 Fixture E2E GREEN**

Run: `pnpm --dir web test:e2e`

Expected: desktop chromium 与 mobile chromium 全部 PASS。

- [ ] **Step 5：运行真实 Rust Server E2E**

Run: `pnpm --dir web test:e2e:real`

Expected: desktop chromium 与 mobile chromium 全部 PASS；日志中不存在对 `/__fixture/requests` 或 `/__fixture/metrics-mode` 的访问。

- [ ] **Step 6：提交 E2E 闭环**

```bash
git add web/e2e/fixture-memory-server.mjs web/e2e/control-plane.spec.ts web/e2e/start-real-memory-server.mjs
git diff --cached --check
git commit -m "test(web): 验证可信状态完整链路" -m "覆盖写入、修正、当前召回、历史召回和遗忘。" -m "分离 Fixture 端点与真实 Rust Server 验收。"
```

## Task 12：更新中文文档并执行最终门禁

**Files:**
- Modify: `docs/superpowers/specs/2026-08-02-core-trusted-memory-closure-design.md`
- Modify: `docs/superpowers/plans/2026-08-02-core-trusted-memory-closure-implementation.md`
- Modify: `docs/concepts/point-edge-surface.md`
- Modify: `docs/architecture/storage.md`
- Modify: `web/README.zh-CN.md`

- [ ] **Step 1：逐项勾选已完成 Task 并记录真实证据**

只有对应提交和聚焦门禁通过后才能把复选框改为 `[x]`。文档记录：SQLite 是唯一底层数据库；PostgreSQL/MySQL 只是兼容记忆内容；Fixture 与真实 Rust Server 结果分开；未安装 SDK 或外部平台保持“未验证”。

- [ ] **Step 2：更新架构与使用文档**

`storage.md` 记录 `projection_checkpoints`、WAL 事务、State/Graph 可重建边界与 Rebuild 顺序；`point-edge-surface.md` 记录确定性 ID；Web README 给出 `blue_green -> canary` 的 Observe/Correct/Recall 操作，不写死真实 API Key。

- [ ] **Step 3：运行 Rust 全量门禁**

Run: `cargo fmt --all -- --check`

Run: `CARGO_INCREMENTAL=0 cargo clippy --workspace --all-targets --all-features -- -D warnings`

Run: `CARGO_INCREMENTAL=0 cargo test --workspace --all-features`

Run: `RUSTDOCFLAGS='-D warnings' CARGO_INCREMENTAL=0 cargo doc --workspace --all-features --no-deps`

Expected: 全部 exit 0，无 warning、失败或忽略的当前 Task 测试。

- [ ] **Step 4：运行 Web 与跨语言门禁**

Run: `pnpm --dir web test`

Run: `pnpm --dir web typecheck`

Run: `pnpm --dir web build`

Run: `pnpm --dir web test:e2e`

Run: `pnpm --dir web test:e2e:real`

Run: `bash scripts/test/sdk-contracts.sh`

Expected: Web 单元、类型、构建、Fixture E2E、真实 Rust Server E2E 全部 PASS；SDK 脚本逐项报告 PASS 或因本机缺工具而明确跳过。

- [ ] **Step 5：检查范围、注释语言和工作树**

Run: `git diff --check`

Run: `rg -n "PostgreSQL|MySQL" web crates docs --glob '!docs/product/original-requirements.zh-CN.md' --glob '!docs/superpowers/specs/2026-08-02-core-trusted-memory-closure-design.md'`

Run: `git status --short`

Expected: diff check exit 0；数据库词只出现在明确标注的兼容测试/历史文档；新增代码注释均为中文；`sdk/java/bin/` 与 `sdk/kotlin/bin/` 仍未跟踪且不在暂存区。

- [ ] **Step 6：提交最终文档、创建阶段 Tag 并推送**

```bash
git add docs/superpowers/specs/2026-08-02-core-trusted-memory-closure-design.md docs/superpowers/plans/2026-08-02-core-trusted-memory-closure-implementation.md docs/concepts/point-edge-surface.md docs/architecture/storage.md web/README.zh-CN.md
git diff --cached --check
git commit -m "docs(core): 记录可信记忆闭环验收" -m "勾选已完成 Task 并记录 Rust、Web、E2E 与 SDK 门禁。" -m "明确 SQLite、Fixture 和外部平台验证边界。"
git tag -a core-trusted-memory-closure -m "完成 Core 可信记忆闭环"
git push origin feat/performance-observability
git push origin core-trusted-memory-closure
```

## 最终验收矩阵

| 能力 | 必须证明的结果 | 证据入口 |
|---|---|---|
| 权威边界 | Mutation 不覆盖 Fragment/Event/EvidenceRef | Core Mutation 与 Snapshot hash 测试 |
| State 契约 | 非法 metadata 在追加前拒绝 | memory-events + Core Stats 测试 |
| 当前/历史 | `blue_green -> canary` 与半开区间正确 | trusted_state_recall |
| 冲突/遗忘 | Disputed 全部可见；Forget 后无当前值 | State Projector + Core Recall |
| 持久化关系 | Core 重启后 Edge/Explain 不丢失 | graph_projection + Core restart |
| Checkpoint | Scope 精确、批次回滚、可续跑 | projection failpoint tests |
| 重建恢复 | Rebuild/Snapshot/Restore 权威 hash 不变 | snapshot_restore |
| 协议一致 | Embedded/REST/gRPC/Sidecar/MCP 规范化一致 | service contract tests |
| Web 可用 | State 写入、当前、历史、原因、Evidence 可操作 | Vitest + Playwright |
| 真实链路 | 不访问 Fixture 专用端点的 Rust Server E2E | `pnpm --dir web test:e2e:real` |
