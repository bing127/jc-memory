# 疆诚记忆 M1 基础可信存储实施计划

> **面向执行代理：** 必须使用 `superpowers:subagent-driven-development`（推荐）或 `superpowers:executing-plans` 逐 Task 执行本计划；所有步骤使用复选框跟踪。

**目标：** 从空仓库建立可离线运行的 Rust Monorepo，完成类型安全的认知片段、不可变事件、SQLite WAL/FTS5、内容寻址 BlobStore、事务化 Observe 与恢复检查。

**架构：** `jc-memory-protocol` 定义稳定领域协议，`jc-memory-events` 负责验证并生成不可变事件，`jc-memory-storage` 提供 SQLite、Migration、BlobStore 和 Repository，`jc-memory-core` 通过单一 Embedded 门面编排它们。权威数据先事务化持久化，FTS 等派生数据可校验和重建。

**技术栈：** Rust Stable 2024、Tokio、Serde、Chrono、UUID v7、SQLx SQLite、FTS5、SHA-256、Zstd、AES-256-GCM、tracing、proptest。

---

## 1. M1 文件边界

| 路径 | 单一职责 |
| --- | --- |
| `Cargo.toml` | Workspace 成员、统一版本和依赖 |
| `rust-toolchain.toml` | Stable、rustfmt、clippy 工具链 |
| `AGENTS.md` | 仓库内开发、测试、权威数据约束 |
| `README.md` / `README.zh-CN.md` | M1 可运行能力和快速开始 |
| `LICENSE` | 私有源码、保留所有权利 |
| `crates/memory-protocol/src/ids.rs` | 强类型 ID |
| `crates/memory-protocol/src/scope.rs` | Namespace/Tenant/User/Agent/Project Scope |
| `crates/memory-protocol/src/fragment.rs` | CognitiveFragment 及关联枚举 |
| `crates/memory-protocol/src/event.rs` | 事件阶段和不可变 Event Envelope 协议 |
| `crates/memory-events/src/validation.rs` | Fragment 不变量校验 |
| `crates/memory-events/src/factory.rs` | 生成内容哈希和不可变 Event |
| `crates/memory-storage/migrations/0001_initial.sql` | 完整首版逻辑 Schema、索引和 FTS5 |
| `crates/memory-storage/src/sqlite.rs` | SQLite 连接、PRAGMA、Migration 和 Health |
| `crates/memory-storage/src/blob/format.rs` | Blob 二进制格式与 Header |
| `crates/memory-storage/src/blob/codec.rs` | Zstd 与 AES-GCM 编解码 |
| `crates/memory-storage/src/blob/store.rs` | 内容寻址、原子写、读取与校验 |
| `crates/memory-storage/src/repository.rs` | Fragment/Event/Evidence 事务仓储 |
| `crates/memory-storage/src/recovery.rs` | Blob 引用检查、孤儿 GC、派生索引重建 |
| `crates/memory-core/src/engine.rs` | Embedded MemoryEngine 编排门面 |
| `crates/memory-core/src/health.rs` | 统一健康和完整性结果 |
| `crates/memory-core/tests/common/mod.rs` | Core 集成测试专用 Fixture 与 Failpoint Harness |
| `crates/memory-core/tests/observe_persistence.rs` | Observe 持久化端到端测试 |
| `crates/memory-core/tests/failure_recovery.rs` | 回滚、损坏、并发和恢复测试 |
| `docs/adr/0001-rust.md` | 选择 Rust 的决策 |
| `docs/adr/0002-sqlite-first.md` | 选择 SQLite WAL/FTS5 的决策 |
| `docs/adr/0003-immutable-events.md` | 原始事件不可变的决策 |
| `docs/architecture/storage.md` | Schema、事务、Blob 与恢复说明 |

## Task 1：建立 Workspace 与仓库约束

**文件：**

- 创建：`Cargo.toml`
- 创建：`rust-toolchain.toml`
- 创建：`.editorconfig`
- 创建：`.gitignore`
- 创建：`deny.toml`
- 创建：`AGENTS.md`
- 创建：`LICENSE`
- 创建：`README.md`
- 创建：`README.zh-CN.md`
- 创建：`SECURITY.md`
- 创建：`CONTRIBUTING.md`
- 创建：`CHANGELOG.md`
- 创建：`crates/memory-protocol/Cargo.toml`
- 创建：`crates/memory-protocol/src/lib.rs`
- 创建：`crates/memory-events/Cargo.toml`
- 创建：`crates/memory-events/src/lib.rs`
- 创建：`crates/memory-storage/Cargo.toml`
- 创建：`crates/memory-storage/src/lib.rs`
- 创建：`crates/memory-core/Cargo.toml`
- 创建：`crates/memory-core/src/lib.rs`

- [ ] **步骤 1：写入 Workspace 清单**

`Cargo.toml` 使用下列成员和统一依赖；M1 不创建其余空 crate：

```toml
[workspace]
resolver = "3"
members = [
  "crates/memory-protocol",
  "crates/memory-events",
  "crates/memory-storage",
  "crates/memory-core",
]

[workspace.package]
version = "0.1.0"
edition = "2024"
rust-version = "1.85"
license-file = "LICENSE"
authors = ["Jiangcheng Memory Contributors"]

[workspace.dependencies]
aes-gcm = "0.10"
async-trait = "0.1"
bytes = "1"
chrono = { version = "0.4", features = ["serde"] }
fs2 = "0.4"
futures = "0.3"
getrandom = "0.3"
hex = "0.4"
proptest = "1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
sha2 = "0.10"
sqlx = { version = "0.8", default-features = false, features = ["runtime-tokio", "sqlite", "migrate", "chrono", "uuid", "json"] }
tempfile = "3"
thiserror = "2"
tokio = { version = "1", features = ["fs", "io-util", "macros", "rt-multi-thread", "sync", "time"] }
tracing = "0.1"
uuid = { version = "1", features = ["serde", "v7"] }
zeroize = { version = "1", features = ["derive"] }
zstd = "0.13"

[workspace.lints.rust]
unsafe_code = "forbid"
missing_docs = "warn"

[workspace.lints.clippy]
all = { level = "deny", priority = -1 }
pedantic = { level = "warn", priority = -1 }
```

各 crate 清单统一继承 Workspace 元数据和 lint。依赖方向固定为 `memory-core -> memory-storage -> memory-events -> memory-protocol`，其中 `memory-storage` 也直接依赖 `memory-protocol`。

四个 crate manifest 的依赖必须精确按职责分配：`memory-protocol` 使用 chrono、serde、serde_json、thiserror、uuid；`memory-events` 使用 chrono、serde_json、sha2、thiserror 和 `jc-memory-protocol`，dev-dependency 使用 proptest；`memory-storage` 使用 aes-gcm、async-trait、fs2、getrandom、hex、serde_json、sha2、sqlx、tempfile、thiserror、tokio、tracing、zeroize、zstd、`jc-memory-protocol` 和 `jc-memory-events`；`memory-core` 使用 serde、thiserror、tokio、tracing 和三个下层 crate，dev-dependency 使用 futures、sha2 和 tempfile。

- [ ] **步骤 2：写入工具链与仓库级配置**

`rust-toolchain.toml`：

```toml
[toolchain]
channel = "stable"
profile = "minimal"
components = ["clippy", "rustfmt"]
```

`.gitignore` 至少忽略 `/target/`、本地数据库、Blob 数据目录、密钥、覆盖率和各 SDK 构建目录；不得忽略 `Cargo.lock`。`LICENSE` 使用 `Copyright (c) 2026 Jiangcheng Memory. All rights reserved.` 并明确未经书面许可不得复制、修改、分发或再许可。

- [ ] **步骤 3：写入仓库开发规则**

`AGENTS.md` 必须明确：中文产品文档优先；代码标识使用英文；不得覆盖权威事件；预测不得成为事实；核心禁止 `unsafe`；行为变更使用 TDD；每个 Task 运行聚焦测试；提交前运行 `fmt`、`clippy`、`test`、`doc` 和 `git diff --check`；保留用户已有改动。

- [ ] **步骤 4：验证 Workspace 元数据**

运行：

```bash
cargo metadata --no-deps --format-version 1
```

预期：退出码为 0，输出包含四个 `jc-memory-*` package，且没有缺失 manifest 或循环依赖。

- [ ] **步骤 5：提交基线**

```bash
git add Cargo.toml rust-toolchain.toml .editorconfig .gitignore deny.toml AGENTS.md LICENSE README.md README.zh-CN.md SECURITY.md CONTRIBUTING.md CHANGELOG.md crates
git commit -m "chore: initialize Jiangcheng Memory workspace"
```

## Task 2：实现强类型 ID 与 Scope

**文件：**

- 创建：`crates/memory-protocol/src/ids.rs`
- 创建：`crates/memory-protocol/src/scope.rs`
- 修改：`crates/memory-protocol/src/lib.rs`

- [ ] **步骤 1：先写 ID 与 Scope 失败测试**

在 `ids.rs` 和 `scope.rs` 的 `#[cfg(test)]` 模块中写入：

```rust
#[test]
fn typed_ids_do_not_lose_their_wire_value() {
    let id = NamespaceId::new();
    let json = serde_json::to_string(&id).unwrap();
    assert_eq!(serde_json::from_str::<NamespaceId>(&json).unwrap(), id);
}

#[test]
fn scope_requires_a_namespace_but_allows_local_optional_scopes() {
    let scope = MemoryScope::new(NamespaceId::new());
    assert!(scope.tenant_id.is_none());
    assert!(scope.user_id.is_none());
    assert!(scope.agent_id.is_none());
    assert!(scope.project_id.is_none());
}
```

- [ ] **步骤 2：运行测试并确认按预期失败**

运行：

```bash
cargo test -p jc-memory-protocol typed_ids_do_not_lose_their_wire_value
```

预期：编译失败，错误指出 `NamespaceId` 或 `MemoryScope` 尚未定义，而不是 manifest 错误。

- [ ] **步骤 3：实现强类型 ID**

`ids.rs` 使用私有 UUID 字段和宏生成 `FragmentId`、`EventId`、`NamespaceId`、`TenantId`、`UserId`、`AgentId`、`ProjectId`、`SessionId`、`TraceId`、`EvidenceId` 和 `BlobId`。UUID 类型提供 `new()`、`as_uuid()`、`Display`、`FromStr`、Serde；`BlobId` 是经过校验的 64 位小写十六进制 SHA-256 字符串，不能复用 UUID 宏。

核心形状如下：

```rust
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, serde::Deserialize, serde::Serialize)]
#[serde(transparent)]
pub struct FragmentId(uuid::Uuid);

impl FragmentId {
    #[must_use]
    pub fn new() -> Self { Self(uuid::Uuid::now_v7()) }
    #[must_use]
    pub const fn as_uuid(&self) -> &uuid::Uuid { &self.0 }
}

impl Default for FragmentId {
    fn default() -> Self { Self::new() }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, serde::Deserialize, serde::Serialize)]
#[serde(try_from = "String", into = "String")]
pub struct BlobId(String);

impl BlobId {
    #[must_use]
    pub fn as_str(&self) -> &str { &self.0 }
}
```

为 `BlobId` 实现 `TryFrom<String>`、`From<BlobId> for String`、`Display` 和 `FromStr`；构造时拒绝非 64 字符、非十六进制或包含大写字符的输入。

- [ ] **步骤 4：实现 Scope**

```rust
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct MemoryScope {
    pub namespace_id: NamespaceId,
    pub tenant_id: Option<TenantId>,
    pub user_id: Option<UserId>,
    pub agent_id: Option<AgentId>,
    pub project_id: Option<ProjectId>,
}

impl MemoryScope {
    #[must_use]
    pub const fn new(namespace_id: NamespaceId) -> Self {
        Self { namespace_id, tenant_id: None, user_id: None, agent_id: None, project_id: None }
    }
}
```

`lib.rs` 只公开模块和明确 re-export，不使用通配符 re-export。

- [ ] **步骤 5：运行协议测试**

```bash
cargo test -p jc-memory-protocol
```

预期：所有 ID 往返、非法 `BlobId` 拒绝和 Scope 默认测试通过。

- [ ] **步骤 6：提交**

```bash
git add crates/memory-protocol
git commit -m "feat(protocol): add typed identifiers and memory scope"
```

## Task 3：实现 CognitiveFragment 与不变量校验

**文件：**

- 创建：`crates/memory-protocol/src/fragment.rs`
- 创建：`crates/memory-events/src/validation.rs`
- 修改：`crates/memory-protocol/src/lib.rs`
- 修改：`crates/memory-events/src/lib.rs`

- [ ] **步骤 1：写入完整领域测试**

测试必须覆盖：19 种 `FragmentKind` 的 Serde 名称稳定；`confidence` 和 `importance` 只接受 `0.0..=1.0`；`observed_at` 不得早于可接受时钟偏差后的 `occurred_at`；Verified 必须有 Evidence；Prediction 默认不能是 Verified；文本不能为空；Blob 内容必须有 BlobId、媒体类型和非零字节数。

关键失败测试：

```rust
#[test]
fn prediction_cannot_enter_as_verified_fact() {
    let mut fragment = fixture(FragmentKind::Prediction);
    fragment.validity = ValidityStatus::Verified;
    assert_eq!(
        validate_fragment(&fragment).unwrap_err(),
        FragmentValidationError::PredictionCannotBeVerified
    );
}

#[test]
fn verified_fragment_requires_evidence() {
    let mut fragment = fixture(FragmentKind::Fact);
    fragment.validity = ValidityStatus::Verified;
    fragment.evidence.clear();
    assert_eq!(
        validate_fragment(&fragment).unwrap_err(),
        FragmentValidationError::VerifiedWithoutEvidence
    );
}
```

- [ ] **步骤 2：确认测试因缺少类型而失败**

```bash
cargo test -p jc-memory-events prediction_cannot_enter_as_verified_fact
```

预期：编译失败并指出 `CognitiveFragment` 或 `validate_fragment` 不存在。

- [ ] **步骤 3：实现协议类型**

`fragment.rs` 定义：

```rust
pub type Metadata = serde_json::Map<String, serde_json::Value>;

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FragmentKind {
    Observation, Claim, Inference, Prediction, Decision, Action, ToolCall,
    ToolResult, Outcome, Correction, Constraint, Preference, Fact, Failure,
    Success, Plan, Approval, Rejection, StateTransition,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FragmentContent {
    Text { text: String },
    Json { value: serde_json::Value },
    Blob { blob_id: BlobId, media_type: String, byte_len: u64 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidityStatus { Unverified, Verified, Disputed, Superseded, Invalidated, Quarantined }

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceType { User, Agent, Model, Tool, Import, System }

#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct FragmentSource {
    pub source_type: SourceType,
    pub source_id: String,
    pub actor_id: Option<String>,
    pub observer_id: Option<String>,
    pub tool_id: Option<String>,
    pub uri: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Perspective { FirstPerson, SecondPerson, ThirdPerson, System, Tool }

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SecurityLabel { Public, Internal, Confidential, Restricted }

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationMethod { Manual, Tool, ExternalSource, CrossEvidence }

#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct EvidenceRef {
    pub id: EvidenceId,
    pub fragment_id: Option<FragmentId>,
    pub event_id: Option<EventId>,
    pub method: VerificationMethod,
    pub note: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct CognitiveFragment {
    pub id: FragmentId,
    pub scope: MemoryScope,
    pub session_id: Option<SessionId>,
    pub trace_id: Option<TraceId>,
    pub kind: FragmentKind,
    pub content: FragmentContent,
    pub source: FragmentSource,
    pub perspective: Perspective,
    pub occurred_at: chrono::DateTime<chrono::Utc>,
    pub observed_at: chrono::DateTime<chrono::Utc>,
    pub confidence: f32,
    pub importance: f32,
    pub validity: ValidityStatus,
    pub security_label: SecurityLabel,
    pub evidence: Vec<EvidenceRef>,
    pub metadata: Metadata,
}
```

`FragmentSource` 明确记录 `source_type`、`source_id`、`actor_id`、`observer_id`、`tool_id` 和可选 URI；`EvidenceRef` 记录 EvidenceId、被引用 Fragment/Event、验证方式和说明；`Perspective` 和 `SecurityLabel` 使用稳定 snake_case Serde。

- [ ] **步骤 4：实现纯函数校验器**

```rust
pub fn validate_fragment(fragment: &CognitiveFragment) -> Result<(), FragmentValidationError> {
    validate_unit_interval("confidence", fragment.confidence)?;
    validate_unit_interval("importance", fragment.importance)?;
    validate_content(&fragment.content)?;
    if fragment.observed_at + chrono::Duration::minutes(5) < fragment.occurred_at {
        return Err(FragmentValidationError::ObservedBeforeOccurred);
    }
    if fragment.kind == FragmentKind::Prediction && fragment.validity == ValidityStatus::Verified {
        return Err(FragmentValidationError::PredictionCannotBeVerified);
    }
    if fragment.validity == ValidityStatus::Verified && fragment.evidence.is_empty() {
        return Err(FragmentValidationError::VerifiedWithoutEvidence);
    }
    Ok(())
}
```

错误枚举使用 `thiserror`，不得返回无结构字符串。校验器还要拒绝空 `source_id`、空媒体类型、同时缺少 `fragment_id` 和 `event_id` 的 EvidenceRef，以及同时填写两个引用目标的 EvidenceRef。

- [ ] **步骤 5：运行测试和 Clippy**

```bash
cargo test -p jc-memory-protocol -p jc-memory-events
cargo clippy -p jc-memory-protocol -p jc-memory-events --all-targets -- -D warnings
```

预期：测试通过，Clippy 无 warning。

- [ ] **步骤 6：提交**

```bash
git add crates/memory-protocol crates/memory-events
git commit -m "feat(events): define and validate cognitive fragments"
```

## Task 4：生成不可变 Event Envelope

**文件：**

- 创建：`crates/memory-protocol/src/event.rs`
- 创建：`crates/memory-events/src/factory.rs`
- 修改：`crates/memory-protocol/src/lib.rs`
- 修改：`crates/memory-events/src/lib.rs`

- [ ] **步骤 1：写入确定性哈希与阶段隔离测试**

```rust
#[test]
fn same_fragment_produces_same_canonical_content_hash() {
    let fragment = fixture(FragmentKind::Observation);
    let first = EventFactory::new().from_fragment(fragment.clone()).unwrap();
    let second = EventFactory::new().from_fragment(fragment).unwrap();
    assert_eq!(first.content_hash, second.content_hash);
    assert_ne!(first.id, second.id);
}

#[test]
fn prediction_event_keeps_future_hypothesis_stage() {
    let event = EventFactory::new()
        .from_fragment(fixture(FragmentKind::Prediction))
        .unwrap();
    assert_eq!(event.stage, CognitiveStage::FutureHypothesis);
}
```

- [ ] **步骤 2：确认失败原因正确**

```bash
cargo test -p jc-memory-events same_fragment_produces_same_canonical_content_hash
```

预期：`EventFactory` 尚未定义导致编译失败。

- [ ] **步骤 3：实现协议与工厂**

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CognitiveStage {
    PastEvidence,
    CurrentObservation,
    FutureHypothesis,
    RuntimeDecision,
    ToolAction,
    ActualResult,
    ResultVerification,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct EventDraft {
    pub id: EventId,
    pub fragment_id: FragmentId,
    pub scope: MemoryScope,
    pub stage: CognitiveStage,
    pub recorded_at: chrono::DateTime<chrono::Utc>,
    pub content_hash: BlobId,
    pub payload: CognitiveFragment,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct MemoryEvent {
    pub draft: EventDraft,
    pub sequence: u64,
}
```

`EventFactory::from_fragment` 返回 `EventDraft`：先调用 `validate_fragment`，再使用递归键排序后的 canonical JSON 计算 SHA-256。Repository 在事务内分配 sequence 并封装成 `MemoryEvent`，不得先生成临时 sequence 后修改 Event。阶段映射使用穷尽 match：Inference/Prediction/Plan -> `FutureHypothesis`；Decision/Approval/Rejection -> `RuntimeDecision`；Action/ToolCall -> `ToolAction`；ToolResult/Outcome/Failure/Success -> `ActualResult`；StateTransition -> `ResultVerification`；Verified Fact/Correction/Constraint/Preference -> `PastEvidence`；其余 Observation/Claim/Fact/Correction/Constraint/Preference -> `CurrentObservation`。

- [ ] **步骤 4：加入 canonical JSON 属性测试**

使用 proptest 随机改变 Metadata 插入顺序，断言内容哈希不变；改变正文、来源或 Scope 时哈希必须变化。

- [ ] **步骤 5：运行聚焦测试**

```bash
cargo test -p jc-memory-events
```

预期：阶段隔离、确定性哈希和属性测试全部通过。

- [ ] **步骤 6：提交**

```bash
git add crates/memory-protocol crates/memory-events
git commit -m "feat(events): create immutable memory event envelopes"
```

## Task 5：实现 SQLite WAL、完整 Schema 与 Migration

**文件：**

- 创建：`crates/memory-storage/migrations/0001_initial.sql`
- 创建：`crates/memory-storage/src/config.rs`
- 创建：`crates/memory-storage/src/sqlite.rs`
- 创建：`crates/memory-storage/tests/migrations.rs`
- 修改：`crates/memory-storage/src/lib.rs`

- [ ] **步骤 1：写入失败的 Migration 契约测试**

```rust
#[tokio::test]
async fn migration_enables_required_pragmas_and_tables() {
    let db = TestDatabase::new().await;
    let journal_mode: String = sqlx::query_scalar("PRAGMA journal_mode")
        .fetch_one(db.pool()).await.unwrap();
    assert_eq!(journal_mode.to_ascii_lowercase(), "wal");
    let foreign_keys: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
        .fetch_one(db.pool()).await.unwrap();
    assert_eq!(foreign_keys, 1);

    let names: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type IN ('table', 'view') ORDER BY name"
    ).fetch_all(db.pool()).await.unwrap();
    for required in REQUIRED_TABLES {
        assert!(names.iter().any(|name| name == required), "missing {required}");
    }
}
```

`REQUIRED_TABLES` 精确列出：`fragments`、`fragment_contents`、`events`、`scope_sequences`、`points`、`edges`、`surfaces`、`surface_members`、`entities`、`entity_aliases`、`states`、`state_versions`、`evidence_refs`、`recall_sessions`、`recall_activations`、`transitions`、`hypotheses`、`decisions`、`actions`、`outcomes`、`capabilities`、`capability_runs`、`approvals`、`model_versions`、`memory_models`、`audit_logs`、`snapshots`、`quarantine`、`blob_refs`、`consolidation_jobs` 和 `_sqlx_migrations`。

- [ ] **步骤 2：运行测试并确认数据库构造尚不存在**

```bash
cargo test -p jc-memory-storage --test migrations
```

预期：编译失败，指出 `TestDatabase` 或 `SqliteStore` 不存在。

- [ ] **步骤 3：编写初始 Schema**

所有权威表使用 `TEXT` ID、`INTEGER` 微秒时间、显式外键和 `CHECK`。关键表结构必须是：

```sql
CREATE TABLE fragments (
    id TEXT PRIMARY KEY NOT NULL,
    scope_key TEXT NOT NULL,
    namespace_id TEXT NOT NULL,
    tenant_id TEXT,
    user_id TEXT,
    agent_id TEXT,
    project_id TEXT,
    session_id TEXT,
    trace_id TEXT,
    kind TEXT NOT NULL,
    content_id TEXT NOT NULL,
    source_json TEXT NOT NULL CHECK (json_valid(source_json)),
    perspective TEXT NOT NULL,
    occurred_at_us INTEGER NOT NULL,
    observed_at_us INTEGER NOT NULL,
    confidence REAL NOT NULL CHECK (confidence >= 0.0 AND confidence <= 1.0),
    importance REAL NOT NULL CHECK (importance >= 0.0 AND importance <= 1.0),
    validity TEXT NOT NULL,
    security_label TEXT NOT NULL,
    metadata_json TEXT NOT NULL CHECK (json_valid(metadata_json)),
    created_at_us INTEGER NOT NULL,
    FOREIGN KEY (content_id) REFERENCES fragment_contents(id) ON DELETE RESTRICT
) STRICT;

CREATE TABLE fragment_contents (
    id TEXT PRIMARY KEY NOT NULL,
    storage_kind TEXT NOT NULL CHECK (storage_kind IN ('inline_text', 'inline_json', 'blob')),
    inline_value BLOB,
    blob_id TEXT,
    media_type TEXT NOT NULL,
    byte_len INTEGER NOT NULL CHECK (byte_len >= 0),
    sha256 TEXT NOT NULL CHECK (length(sha256) = 64),
    created_at_us INTEGER NOT NULL,
    UNIQUE (sha256, media_type),
    CHECK ((storage_kind = 'blob' AND blob_id IS NOT NULL AND inline_value IS NULL)
        OR (storage_kind != 'blob' AND blob_id IS NULL AND inline_value IS NOT NULL))
) STRICT;

CREATE TABLE events (
    id TEXT PRIMARY KEY NOT NULL,
    fragment_id TEXT NOT NULL,
    scope_key TEXT NOT NULL,
    namespace_id TEXT NOT NULL,
    tenant_id TEXT,
    stage TEXT NOT NULL,
    sequence INTEGER NOT NULL CHECK (sequence >= 0),
    recorded_at_us INTEGER NOT NULL,
    content_hash TEXT NOT NULL CHECK (length(content_hash) = 64),
    payload_json TEXT NOT NULL CHECK (json_valid(payload_json)),
    FOREIGN KEY (fragment_id) REFERENCES fragments(id) ON DELETE RESTRICT,
    UNIQUE (scope_key, sequence)
) STRICT;

CREATE TABLE scope_sequences (
    scope_key TEXT PRIMARY KEY NOT NULL,
    next_sequence INTEGER NOT NULL CHECK (next_sequence >= 0)
) STRICT;

CREATE TABLE evidence_refs (
    id TEXT PRIMARY KEY NOT NULL,
    owner_fragment_id TEXT NOT NULL,
    referenced_fragment_id TEXT,
    referenced_event_id TEXT,
    verification_method TEXT NOT NULL,
    note TEXT,
    created_at_us INTEGER NOT NULL,
    FOREIGN KEY (owner_fragment_id) REFERENCES fragments(id) ON DELETE CASCADE,
    FOREIGN KEY (referenced_fragment_id) REFERENCES fragments(id) ON DELETE RESTRICT,
    FOREIGN KEY (referenced_event_id) REFERENCES events(id) ON DELETE RESTRICT,
    CHECK (referenced_fragment_id IS NOT NULL OR referenced_event_id IS NOT NULL)
) STRICT;

CREATE VIRTUAL TABLE fragment_fts USING fts5(
    fragment_id UNINDEXED,
    scope_key UNINDEXED,
    body,
    tokenize = 'unicode61 remove_diacritics 2'
);
```

其余逻辑表必须使用独立主键、`scope_key`、来源/证据列、版本列和适用外键，不能用单个无约束 JSON 表替代。`scope_key` 是 MemoryScope 各强类型 ID 的 canonical JSON SHA-256，解决 SQLite `NULL` 参与复合唯一约束时允许重复的问题。为 Fragment Scope/时间、Event Scope/sequence、Edge 起终点/类型/有效时间、Surface Member、State 当前版本、Recall Query、Transition Trace、Capability Version、Audit Scope 和 Blob 引用建立明确索引。Migration 不使用触发器覆盖原始行；FTS 由 Repository 在同一事务内维护。

其余表的最小列集合在 M1 固定如下，实施时逐项转换为带类型、外键、`CHECK` 和索引的 SQL，不得删减：

| 表 | 必需列 |
| --- | --- |
| `points` | id、scope_key、kind、source_fragment_id、valid_from_us、valid_to_us、confidence、created_at_us |
| `edges` | id、scope_key、source_point_id、target_point_id、kind、weight、confidence、source_fragment_id、valid_from_us、valid_to_us、model_version_id、human_confirmed、tool_verified、created_at_us |
| `surfaces` | id、scope_key、kind、status、semantic_center_json、activation_threshold、confidence、model_version_id、version、valid_from_us、valid_to_us、source_evidence_json、created_at_us |
| `surface_members` | surface_id、point_id、member_weight、version、created_at_us，复合主键为 surface_id/point_id/version |
| `entities` | id、scope_key、entity_type、canonical_name、source_fragment_id、created_at_us |
| `entity_aliases` | entity_id、alias、source_fragment_id、created_at_us，复合主键为 entity_id/alias |
| `states` | id、scope_key、state_key、current_version_id、created_at_us，scope_key/state_key 唯一 |
| `state_versions` | id、state_id、value_json、valid_from_us、valid_to_us、validity、source_fragment_id、previous_version_id、created_at_us |
| `recall_sessions` | id、scope_key、intent、query_json、limits_json、started_at_us、completed_at_us、status |
| `recall_activations` | recall_session_id、point_id、activation、depth、via_edge_id、created_at_us |
| `transitions` | id、scope_key、trace_id、from_stage、to_stage、payload_json、created_at_us |
| `hypotheses` | id、transition_id、kind、content_json、validity、source_fragment_id、created_at_us |
| `decisions` | id、transition_id、candidate_id、policy_result_json、approval_id、created_at_us |
| `actions` | id、transition_id、capability_id、request_json、idempotency_key、status、started_at_us、completed_at_us |
| `outcomes` | id、action_id、result_json、validity、source_fragment_id、created_at_us |
| `capabilities` | id、scope_key、kind、version、source、permissions_json、input_schema_json、output_schema_json、risk_level、requires_approval、created_at_us，id/version 唯一 |
| `capability_runs` | id、capability_id、capability_version、transition_id、status、failure_mode、started_at_us、completed_at_us |
| `approvals` | id、scope_key、transition_id、principal_id、status、reason、created_at_us、decided_at_us |
| `model_versions` | id、provider、model_name、version、manifest_json、checksum、created_at_us |
| `memory_models` | id、kind、version、manifest_json、checksum、status、created_at_us |
| `audit_logs` | id、scope_key、principal_id、operation、target_type、target_id、detail_digest、created_at_us |
| `snapshots` | id、scope_key、schema_version、manifest_json、checksum、created_at_us |
| `quarantine` | id、scope_key、fragment_id、reason_code、status、reviewed_by、created_at_us、reviewed_at_us |
| `blob_refs` | blob_id、owner_type、owner_id、ref_count、created_at_us、updated_at_us，blob_id/owner_type/owner_id 为复合主键 |
| `consolidation_jobs` | id、scope_key、kind、status、checkpoint_json、attempts、resource_limits_json、created_at_us、updated_at_us |

- [ ] **步骤 4：实现 SQLite 连接**

`SqliteConfig` 包含路径、最大连接数和 Busy Timeout。`SqliteStore::open` 使用 `SqliteConnectOptions` 开启 create-if-missing、foreign_keys、WAL、NORMAL synchronous 和 busy_timeout，然后执行 `sqlx::migrate!()`。内存测试库限制为单连接，避免每个连接获得不同数据库。

```rust
pub struct SqliteStore { pool: sqlx::SqlitePool }

impl SqliteStore {
    pub async fn open(config: &SqliteConfig) -> Result<Self, StorageError>;
    #[must_use]
    pub const fn pool(&self) -> &sqlx::SqlitePool;
    pub async fn integrity_check(&self) -> Result<(), StorageError>;
}
```

- [ ] **步骤 5：测试 Migration 幂等性和回滚**

同一路径连续打开两次，断言只有一条 Migration 记录；复制一份包含故意失败 SQL 的测试 Migration，断言 Schema 不出现半张表。

```bash
cargo test -p jc-memory-storage --test migrations
```

预期：PRAGMA、表、索引、FTS、幂等和回滚测试通过。

- [ ] **步骤 6：提交**

```bash
git add crates/memory-storage
git commit -m "feat(storage): add SQLite WAL schema and migrations"
```

## Task 6：实现内容寻址 BlobStore

**文件：**

- 创建：`crates/memory-storage/src/blob/mod.rs`
- 创建：`crates/memory-storage/src/blob/format.rs`
- 创建：`crates/memory-storage/src/blob/codec.rs`
- 创建：`crates/memory-storage/src/blob/store.rs`
- 创建：`crates/memory-storage/tests/blob_store.rs`
- 修改：`crates/memory-storage/src/lib.rs`

- [ ] **步骤 1：写入失败的 Blob 行为测试**

```rust
#[tokio::test]
async fn blob_id_is_plaintext_sha256_and_round_trips_all_codecs() {
    use sha2::Digest as _;
    let bytes = b"immutable evidence";
    for options in fixtures::codec_options() {
        let store = fixtures::store(options).await;
        let receipt = store.put(bytes).await.unwrap();
        assert_eq!(receipt.id.as_str(), hex::encode(sha2::Sha256::digest(bytes)));
        assert_eq!(store.get(&receipt.id).await.unwrap(), bytes);
    }
}

#[tokio::test]
async fn tampered_blob_is_rejected() {
    let fixture = fixtures::store_fixture(BlobOptions::plain()).await;
    let receipt = fixture.store.put(b"evidence").await.unwrap();
    fixtures::flip_payload_byte(fixture.path(&receipt.id)).await;
    assert!(matches!(fixture.store.get(&receipt.id).await, Err(BlobError::ChecksumMismatch { .. })));
}
```

另写测试覆盖重复写入幂等、并发相同内容写入、Zstd、AES-GCM 错误密钥、临时文件不被读取、缺失 Blob 和零字节 Blob。

- [ ] **步骤 2：确认测试因 BlobStore 缺失而失败**

```bash
cargo test -p jc-memory-storage --test blob_store
```

预期：编译失败并指出 `BlobStore` 不存在。

- [ ] **步骤 3：实现稳定二进制格式**

Blob 文件格式固定为：4 字节 `JCB1` magic、1 字节 flags、1 字节 nonce 长度、8 字节原始长度、nonce、payload、32 字节明文 SHA-256。Flags bit 0 表示 Zstd，bit 1 表示 AES-256-GCM。Header 解析拒绝未知 flag、长度溢出、截断和超过配置上限的内容。

```rust
pub struct BlobReceipt {
    pub id: BlobId,
    pub stored_bytes: u64,
    pub original_bytes: u64,
    pub created: bool,
}

pub struct BlobOptions {
    pub compression: Compression,
    pub encryption: Encryption,
    pub max_blob_bytes: u64,
}

pub enum Compression { None, Zstd { level: i32 } }
pub enum Encryption { None, Aes256Gcm { key: zeroize::Zeroizing<[u8; 32]> } }

impl BlobOptions {
    #[must_use]
    pub const fn plain() -> Self {
        Self { compression: Compression::None, encryption: Encryption::None, max_blob_bytes: 64 * 1024 * 1024 }
    }
}
```

- [ ] **步骤 4：实现原子内容寻址存储**

`BlobStore::put` 先对明文计算 SHA-256，以 `<root>/<hash[0..2]>/<hash[2..4]>/<hash>.jcb` 分片；在目标目录内创建随机临时文件，写入、`sync_all`、原子 persist，再同步父目录。目标已存在时读取并校验明文哈希，不能盲目信任同名文件。`get` 依次校验 Header、解密、解压、长度和明文 SHA-256。

```rust
impl BlobStore {
    pub async fn open(root: impl AsRef<std::path::Path>, options: BlobOptions) -> Result<Self, BlobError>;
    pub async fn put(&self, plaintext: &[u8]) -> Result<BlobReceipt, BlobError>;
    pub async fn get(&self, id: &BlobId) -> Result<Vec<u8>, BlobError>;
    pub async fn verify(&self, id: &BlobId) -> Result<BlobReceipt, BlobError>;
}
```

随机 nonce 使用 `getrandom::fill`，密钥类型必须 `Zeroizing`；错误不得包含密钥或明文。

- [ ] **步骤 5：运行 Blob 测试**

```bash
cargo test -p jc-memory-storage --test blob_store
```

预期：Plain、Zstd、AES-GCM、组合编解码、损坏、并发和原子写测试通过。

- [ ] **步骤 6：提交**

```bash
git add crates/memory-storage
git commit -m "feat(storage): add verified content-addressed blob store"
```

## Task 7：实现事务化 Fragment/Event Repository 与 FTS5

**文件：**

- 创建：`crates/memory-storage/src/error.rs`
- 创建：`crates/memory-storage/src/repository.rs`
- 创建：`crates/memory-storage/tests/repository.rs`
- 修改：`crates/memory-storage/src/lib.rs`

- [ ] **步骤 1：写入 Observe 持久化失败测试**

```rust
#[tokio::test]
async fn append_persists_fragment_event_evidence_and_fts_atomically() {
    let harness = Harness::new().await;
    let draft = harness.event("2018 payment migration failed", ValidityStatus::Unverified);
    let receipt = harness.repository.append(&draft).await.unwrap();

    assert_eq!(harness.repository.get_fragment(draft.fragment_id).await.unwrap(), draft.payload);
    assert_eq!(harness.repository.get_event(draft.id).await.unwrap(), receipt.event);
    assert_eq!(harness.repository.search_fts(&draft.scope, "payment migration", 10).await.unwrap().len(), 1);
}

#[tokio::test]
async fn duplicate_event_is_idempotent_but_conflicting_payload_is_rejected() {
    let harness = Harness::new().await;
    let draft = harness.event("evidence", ValidityStatus::Unverified);
    harness.repository.append(&draft).await.unwrap();
    harness.repository.append(&draft).await.unwrap();
    let conflicting = draft.with_changed_payload_for_test("different");
    assert!(matches!(harness.repository.append(&conflicting).await, Err(StorageError::IdentityConflict { .. })));
}
```

另写测试确保失败 Evidence 外键导致 Fragment、Content、Event 和 FTS 全部回滚。

- [ ] **步骤 2：确认测试因 Repository 缺失而失败**

```bash
cargo test -p jc-memory-storage --test repository
```

预期：编译失败并指出 `MemoryRepository` 不存在。

- [ ] **步骤 3：实现事务 Repository**

```rust
#[async_trait::async_trait]
pub trait MemoryRepository: Send + Sync {
    async fn append(&self, draft: &EventDraft) -> Result<AppendReceipt, StorageError>;
    async fn get_fragment(&self, id: FragmentId) -> Result<CognitiveFragment, StorageError>;
    async fn get_event(&self, id: EventId) -> Result<MemoryEvent, StorageError>;
    async fn search_fts(&self, scope: &MemoryScope, query: &str, limit: u32) -> Result<Vec<FragmentSearchHit>, StorageError>;
}

pub struct AppendReceipt {
    pub event: MemoryEvent,
    pub created: bool,
}
```

`SqliteMemoryRepository::append` 以 `BEGIN IMMEDIATE` 开始；原子分配 `scope_key` 对应 sequence；按内容哈希去重 `fragment_contents`；插入 Fragment、Event、EvidenceRef 和 `fragment_fts`；只有所有操作成功才提交。重复相同 ID 和相同哈希返回 `created: false`；相同 ID 不同哈希返回 `IdentityConflict`。FTS Query 先绑定精确 `scope_key`，再绑定转义后的 MATCH 表达式和硬上限，不拼接用户输入。

- [ ] **步骤 4：实现数据库行到领域类型的严格解码**

无效枚举、非法 JSON、越界数值和缺失引用返回 `StorageError::CorruptRow { table, id, reason }`，不使用默认值掩盖损坏。所有时间戳按 UTC 微秒往返测试。

- [ ] **步骤 5：运行仓储测试**

```bash
cargo test -p jc-memory-storage --test repository
```

预期：原子写入、幂等、冲突、回滚、Scope 过滤、FTS 转义和严格解码测试通过。

- [ ] **步骤 6：提交**

```bash
git add crates/memory-storage
git commit -m "feat(storage): persist immutable events transactionally"
```

## Task 8：实现恢复、完整性检查、引用计数与 GC

**文件：**

- 创建：`crates/memory-storage/src/recovery.rs`
- 创建：`crates/memory-storage/tests/recovery.rs`
- 修改：`crates/memory-storage/src/lib.rs`

- [ ] **步骤 1：写入失败的恢复测试**

```rust
#[tokio::test]
async fn audit_reports_missing_and_corrupt_referenced_blobs_without_deleting_evidence() {
    let harness = Harness::new().await;
    let missing = harness.insert_referenced_blob_then_remove_file().await;
    let corrupt = harness.insert_referenced_blob_then_corrupt_file().await;
    let report = harness.recovery.audit().await.unwrap();
    assert_eq!(report.missing_blobs, vec![missing]);
    assert_eq!(report.corrupt_blobs, vec![corrupt]);
    assert_eq!(harness.fragment_count().await, 2);
}

#[tokio::test]
async fn garbage_collection_only_removes_unreferenced_blobs_after_grace_period() {
    let harness = Harness::new().await;
    let referenced = harness.put_referenced_blob().await;
    let orphan = harness.put_old_orphan_blob().await;
    let report = harness.recovery.collect_garbage(GcPolicy::default()).await.unwrap();
    assert!(report.removed.contains(&orphan));
    assert!(!report.removed.contains(&referenced));
}
```

- [ ] **步骤 2：确认测试因 RecoveryManager 缺失而失败**

```bash
cargo test -p jc-memory-storage --test recovery
```

预期：编译失败，指出 `RecoveryManager` 不存在。

- [ ] **步骤 3：实现只读 Audit 和显式 GC**

```rust
pub struct IntegrityReport {
    pub database_ok: bool,
    pub checked_blobs: u64,
    pub missing_blobs: Vec<BlobId>,
    pub corrupt_blobs: Vec<BlobId>,
    pub orphan_blobs: Vec<BlobId>,
    pub fts_mismatches: u64,
}

impl RecoveryManager {
    pub async fn audit(&self) -> Result<IntegrityReport, RecoveryError>;
    pub async fn collect_garbage(&self, policy: GcPolicy) -> Result<GcReport, RecoveryError>;
    pub async fn rebuild_fts(&self) -> Result<RebuildReport, RecoveryError>;
}
```

`audit` 运行 `PRAGMA quick_check`、枚举 `blob_refs` 与磁盘 Blob、逐个 Verify，并比较可索引 Fragment 与 FTS 行数。`collect_garbage` 必须获得进程文件锁、重新核对数据库引用、满足 Grace Period 后才原子移动到 trash，再删除；默认只报告不删除。`rebuild_fts` 在事务中建立临时 FTS 表、填充、核对数量后切换，失败时保留原索引。

- [ ] **步骤 4：加入崩溃点测试**

通过测试专用 `Failpoint` Trait 在 Blob 临时写后、原子重命名后、数据库提交前和 FTS 重建切换前返回错误。每个测试重新打开 Store，断言权威 Event 不出现半写，孤儿可被报告，原 FTS 仍可查询。

- [ ] **步骤 5：运行恢复测试**

```bash
cargo test -p jc-memory-storage --test recovery
```

预期：缺失/损坏检测、Grace Period、文件锁、Failpoint 和 FTS 重建测试通过。

- [ ] **步骤 6：提交**

```bash
git add crates/memory-storage
git commit -m "feat(storage): add integrity audit and crash recovery"
```

## Task 9：实现 MemoryEngine 的 Observe 与 Health 门面

**文件：**

- 创建：`crates/memory-core/src/config.rs`
- 创建：`crates/memory-core/src/engine.rs`
- 创建：`crates/memory-core/src/health.rs`
- 创建：`crates/memory-core/tests/common/mod.rs`
- 创建：`crates/memory-core/tests/observe_persistence.rs`
- 修改：`crates/memory-core/src/lib.rs`

- [ ] **步骤 1：写入 Embedded 闭环失败测试**

```rust
#[tokio::test]
async fn observe_returns_authoritative_event_and_survives_reopen() {
    let data_dir = tempfile::tempdir().unwrap();
    let engine = MemoryEngine::open(MemoryConfig::local(data_dir.path())).await.unwrap();
    let fragment = fixtures::observation("2018 project failed because migration timed out");
    let receipt = engine.observe(fragment.clone()).await.unwrap();
    drop(engine);

    let reopened = MemoryEngine::open(MemoryConfig::local(data_dir.path())).await.unwrap();
    assert_eq!(reopened.fragment(receipt.fragment_id).await.unwrap(), fragment);
    assert_eq!(reopened.health().await.unwrap().status, HealthStatus::Healthy);
}
```

- [ ] **步骤 2：确认测试因 MemoryEngine 缺失而失败**

```bash
cargo test -p jc-memory-core --test observe_persistence
```

预期：编译失败并指出 `MemoryEngine` 不存在。

- [ ] **步骤 3：实现公开门面**

```rust
#[derive(Clone)]
pub struct MemoryEngine {
    repository: std::sync::Arc<dyn MemoryRepository>,
    recovery: std::sync::Arc<RecoveryManager>,
    event_factory: EventFactory,
}

impl MemoryEngine {
    pub async fn open(config: MemoryConfig) -> Result<Self, MemoryError>;
    pub async fn observe(&self, fragment: CognitiveFragment) -> Result<ObserveReceipt, MemoryError>;
    pub async fn fragment(&self, id: FragmentId) -> Result<CognitiveFragment, MemoryError>;
    pub async fn health(&self) -> Result<HealthReport, MemoryError>;
}


pub struct ObserveReceipt {
    pub fragment_id: FragmentId,
    pub event_id: EventId,
    pub sequence: u64,
    pub content_hash: BlobId,
    pub created: bool,
    pub quarantined: bool,
}

pub struct HealthReport {
    pub status: HealthStatus,
    pub database_ok: bool,
    pub blob_store_ok: bool,
    pub schema_version: i64,
}

pub enum HealthStatus { Healthy, Degraded, Unhealthy }
```

`observe` 的 M1 返回值只包含已真实产生的 FragmentId、EventId、sequence、内容哈希、是否新建和 Quarantine 状态。M2 在实现 Point、Candidate Edge、Surface 和 Conflict 后再引入完整 `ObserveResult`，M1 不返回虚假空认知结构。

- [ ] **步骤 4：实现结构化 tracing**

Span 只记录 `event_id`、`fragment_id`、Namespace/Tenant 的不可逆短哈希、耗时和结果；不记录正文、密钥或完整敏感 Scope。错误链保留结构化来源。

- [ ] **步骤 5：运行 Embedded 闭环测试**

```bash
cargo test -p jc-memory-core --test observe_persistence
```

预期：首次 Observe、重开读取、重复幂等和 Health 测试通过。

- [ ] **步骤 6：提交**

```bash
git add crates/memory-core
git commit -m "feat(core): expose embedded observe and health APIs"
```

## Task 10：补齐并发、回滚和恢复验收

**文件：**

- 创建：`crates/memory-core/tests/failure_recovery.rs`
- 修改：`crates/memory-storage/src/repository.rs`
- 修改：`crates/memory-storage/src/recovery.rs`

- [ ] **步骤 1：写入并发与故障测试**

```rust
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_observe_assigns_unique_monotonic_namespace_sequences() {
    let engine = fixtures::engine().await;
    let receipts = futures::future::try_join_all((0..64).map(|n| {
        let engine = engine.clone();
        async move { engine.observe(fixtures::observation(&format!("event {n}"))).await }
    })).await.unwrap();
    let mut sequences: Vec<_> = receipts.into_iter().map(|r| r.sequence).collect();
    sequences.sort_unstable();
    assert_eq!(sequences, (0..64).collect::<Vec<_>>());
}

#[tokio::test]
async fn failed_commit_never_exposes_fragment_event_or_fts_row() {
    let harness = fixtures::engine_with_failpoint(Failpoint::BeforeCommit).await;
    let fragment = fixtures::observation("must roll back");
    assert!(harness.engine.observe(fragment.clone()).await.is_err());
    let reopened = harness.reopen().await;
    assert!(matches!(reopened.fragment(fragment.id).await, Err(MemoryError::NotFound { .. })));
    assert!(reopened.search_for_test("must roll back").await.unwrap().is_empty());
}
```

再加入：相同 ID 并发幂等、不同 Tenant 独立 sequence、Busy Timeout 可诊断、Blob rename 后事务失败产生可报告孤儿、损坏 Blob 不返回内容、FTS rebuild 中断后原索引可用。

- [ ] **步骤 2：确认至少一个新测试失败**

```bash
cargo test -p jc-memory-core --test failure_recovery
```

预期：并发 sequence 或 Failpoint 测试失败，证明测试能捕获尚未处理的边界。

- [ ] **步骤 3：收紧 sequence 分配与错误分类**

在 `BEGIN IMMEDIATE` 事务内从每个 Namespace/Tenant 的 sequence 计数表执行原子递增；SQLite Busy、约束冲突、损坏和 IO 错误映射为不同错误码。仅对 Busy/Locked 做有上限、带 jitter 的重试，身份冲突和校验失败不得重试。

- [ ] **步骤 4：运行压力回归 20 次**

```bash
for run in $(seq 1 20); do cargo test -p jc-memory-core --test failure_recovery --quiet || exit 1; done
```

预期：20 次全部退出码 0，无偶发 sequence 重复或数据库锁失败。

- [ ] **步骤 5：提交**

```bash
git add crates/memory-core crates/memory-storage
git commit -m "test: harden concurrent writes and recovery paths"
```

## Task 11：完成 M1 文档、ADR 与质量门禁

**文件：**

- 创建：`docs/adr/0001-rust.md`
- 创建：`docs/adr/0002-sqlite-first.md`
- 创建：`docs/adr/0003-immutable-events.md`
- 创建：`docs/architecture/storage.md`
- 修改：`README.md`
- 修改：`README.zh-CN.md`
- 修改：`CHANGELOG.md`

- [ ] **步骤 1：编写 ADR**

每份 ADR 固定包含 Context、Decision、Consequences、Rejected Alternatives。必须明确：Rust 提供无 GC 的可移植内核；SQLite 避免自研 B-Tree/WAL 并提供成熟事务与 FTS5；原始 Event 只追加，Correction/Supersession 通过新事件表达；派生索引可以重建。

- [ ] **步骤 2：编写存储架构文档**

`docs/architecture/storage.md` 必须列出所有逻辑表、主外键、Scope 索引、权威/派生分类、Observe 事务序列、Blob `JCB1` 格式、故障点恢复表、GC 安全条件、Migration 策略和本地数据目录布局。

- [ ] **步骤 3：更新快速开始**

中文 README 为主，英文 README 提供对应入口。快速开始只使用本地临时目录和 Rust 示例，展示创建 `MemoryEngine`、写入 Observation、重开读取和 Health；不依赖外部 LLM 或 Python，不宣称 M2-M6 能力已经完成。

- [ ] **步骤 4：运行占位符和 unsafe 扫描**

```bash
rg -n 'TODO|FIXME|unimplemented!|todo!' crates README.md README.zh-CN.md docs/adr docs/architecture
rg -n 'unsafe' crates
```

预期：无实现占位；`unsafe` 只可能出现在解释禁止使用的文档语句中，`crates/` 内无匹配。

- [ ] **步骤 5：运行 M1 全量门禁**

```bash
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo doc --workspace --no-deps
git diff --check
```

预期：所有命令退出码为 0。记录测试数量、运行时长、当前主机 `rustc -Vv` 和 SQLite/FTS5 可用性；不得把 macOS 当前主机结果描述为 Linux 或 Windows 已验证。

- [ ] **步骤 6：提交文档与 M1 验收记录**

```bash
git add README.md README.zh-CN.md CHANGELOG.md docs/adr docs/architecture
git commit -m "docs: document M1 storage architecture and decisions"
```

## 2. M1 完成定义

执行者在宣布 M1 完成前必须逐项确认：

- 四个 M1 crate 均有真实调用链和测试；
- `CognitiveFragment` 覆盖需求中的 Scope、来源、时间、Validity、安全标签、证据和元数据；
- Prediction 不能以 Verified 身份写入；
- Event 内容哈希稳定且原始 Event 不可原地修改；
- SQLite 实际运行在 WAL、Foreign Keys 和 Busy Timeout 配置下；
- 初始 Migration 创建需求列出的全部逻辑表、索引和 FTS5；
- BlobStore 实现 SHA-256、原子写、分片、Zstd、AES-GCM、损坏检测、引用检查和 GC；
- Fragment、Event、EvidenceRef 和 FTS 在同一事务语义下保持一致；
- 回滚、并发、模拟崩溃、重开、损坏和 Rebuild 测试真实通过；
- `fmt`、`clippy -D warnings`、`test --all-features`、`doc` 和 `git diff --check` 真实通过；
- M2 尚未实现的能力在 README 中明确标注，不存在假实现或虚假完成声明。
