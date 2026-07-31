# 疆诚记忆

疆诚记忆是独立于具体大语言模型的 Memory-Native Agent Kernel 与神经认知记忆数据库。

当前代码实现 M1 基础可信存储、M2 认知图/有界召回与 M3 巩固/安全治理。项目目前为私有专有源码，不计划公开发布。

## 当前可用范围

- `jc-memory-protocol`：类型化 ID、MemoryScope、CognitiveFragment、Event 和 EvidenceRef；
- `jc-memory-events`：Fragment 校验、认知阶段映射和稳定内容哈希；
- `jc-memory-storage`：SQLite v3 Migration、事务仓储、FTS5、`JCB1` Blob、审计、GC、Snapshot/Restore 和阶段化 rebuild；
- `jc-memory-episodic`：模式分离与带有效时间的状态版本链；
- `jc-memory-semantic`：至少两个独立 Event 支撑的语义提升，以及保留样本数和证据的程序经验；
- `jc-memory-graph`：带权威来源的 Point/Edge、有界局部图和在线权重更新；
- `jc-memory-surface`：多成员 Surface、版本历史和 Add/Remove/Merge/Split/Freeze/Deactivate 生命周期；
- `jc-memory-activation`：十种 Recall 的确定性种子、局部传播、Surface 激活、证据补全、Explain 和硬预算；
- `jc-memory-neural`：八个插件 Trait、版本化 Manifest、超时/熔断，以及完全离线且确定性的规则与词法哈希 Fallback；
- `jc-memory-consolidation`：12 种保守 Handler、持久化 Lease/Checkpoint、Pause/Resume/Retry/Cancel 和批量/CPU/IO/墙钟预算；
- `jc-memory-security`：默认拒绝、精确 Scope、`None` 非通配和跨 Tenant 显式 Admin 的 ACL，Packet 返回前来源复核，以及 Quarantine、Retention、Export、Hard Delete 和 Key Provider 契约；
- `jc-memory-core`：本地嵌入式 `MemoryEngine`，公开 Observe、Fragment、Health、Recall、Explain、Stats、Mutation、Quarantine 审查和 Snapshot/Restore/Rebuild，所有操作显式接收 `AccessContext`。

原始 Fragment、Event 和 EvidenceRef 是权威记录。摘要、Embedding、图关系、Surface 和全文索引不得覆盖它们。Prediction 不允许设置 `ValidityStatus::Verified`；Inference 和 Plan 始终保持 FutureHypothesis 阶段，三者都不会被自动改写成 Fact。

当前 Core 的 SQLite FTS5 已接入真实 Recall 路径。非 Exact Recall 还会读取精确 Scope 内的近期有界候选，并使用确定性词项覆盖率、字符稀疏特征和实体匹配评分；这不是外部 Embedding 或通用语义模型。Mutation 先追加权威 Fragment/Event，再更新进程内派生图，且在权威元数据中保留关系和目标标识。

当前实现仍有明确边界：状态、语义和 Surface 模块尚未全部汇入 Core 的持久化 Memory Packet；Core 重启后尚未自动重建进程内派生图；`Operation::Consolidate` 尚未接入 Core Job 提交门面；派生重建的外部 Embedding 与 Summary backend 尚未接入。没有向量数据库或远程服务。M4-M6 的 Runtime 与工具执行、HTTP/gRPC/MCP 服务、多语言 SDK、外部 Adapter、训练/ONNX、Benchmark 和发布流水线仍未实现。Schema 或协议字段的存在不代表对应上层调用链已经接通。

## 五分钟开始

需要 Rust 1.97 或更高版本。仓库当前使用 Rust 2024 Edition，不依赖外部 LLM、Python、数据库服务或网络 API。

先运行真实的嵌入式持久化与 M3 聚焦验收：

```bash
cargo test -p jc-memory-core --test observe_persistence
CARGO_INCREMENTAL=0 cargo test -p jc-memory-core --test tenant_isolation --test poisoning --test snapshot_restore --offline
CARGO_INCREMENTAL=0 cargo test -p jc-memory-consolidation --test conservative_learning --test job_recovery --offline
CARGO_INCREMENTAL=0 cargo test -p jc-memory-security --test hard_delete --offline
CARGO_INCREMENTAL=0 cargo test -p jc-memory-neural --test fallback --offline
```

下面是同一调用链的完整 Rust 示例。`tempfile::TempDir` 在变量离开作用域时清理数据；正式应用应把 `MemoryConfig::local` 指向持久目录。

```rust
use chrono::Utc;
use jc_memory_core::{HealthStatus, MemoryConfig, MemoryEngine};
use jc_memory_protocol::{
    AccessContext, CognitiveFragment, FragmentContent, FragmentId, FragmentKind, FragmentSource,
    Grant, MemoryScope, NamespaceId, Operation, Perspective, SecurityLabel, SourceType,
    ValidityStatus,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let data_dir = tempfile::tempdir()?;
    let config = MemoryConfig::local(data_dir.path());
    let engine = MemoryEngine::open(config.clone()).await?;
    let now = Utc::now();
    let fragment = CognitiveFragment {
        id: FragmentId::new(),
        scope: MemoryScope::new(NamespaceId::new()),
        session_id: None,
        trace_id: None,
        kind: FragmentKind::Observation,
        content: FragmentContent::Text {
            text: "2018 年的迁移因超时失败".to_owned(),
        },
        source: FragmentSource {
            source_type: SourceType::User,
            source_id: "local-example".to_owned(),
            actor_id: None,
            observer_id: None,
            tool_id: None,
            uri: None,
        },
        perspective: Perspective::FirstPerson,
        occurred_at: now,
        observed_at: now,
        confidence: 1.0,
        importance: 0.7,
        validity: ValidityStatus::Unverified,
        security_label: SecurityLabel::Internal,
        evidence: Vec::new(),
        metadata: serde_json::Map::new(),
    };
    let mut access = AccessContext::new("local-example", fragment.scope.clone());
    access.grants.extend(
        [Operation::Observe, Operation::Recall, Operation::Admin]
            .into_iter()
            .map(|operation| Grant::for_scope(operation, &fragment.scope)),
    );

    let receipt = engine.observe(&access, fragment.clone()).await?;
    drop(engine);

    let reopened = MemoryEngine::open(config).await?;
    let stored = reopened.fragment(&access, receipt.fragment_id).await?;
    assert_eq!(stored, fragment);

    let health = reopened.health(&access).await?;
    assert_eq!(health.status, HealthStatus::Healthy);
    assert_eq!(health.schema_version, 3);
    Ok(())
}
```

示例需要 `jc-memory-core`、`jc-memory-protocol`、`chrono`、`serde_json`、`tempfile` 和启用 `macros`/`rt-multi-thread` 的 `tokio`。仓库内的 Core 集成测试已声明这些开发依赖，可作为可运行参考。

## 数据与恢复

默认本地目录包含 `memory.sqlite3` 和 `blobs/`。SQLite 使用 Foreign Keys、WAL、`synchronous = NORMAL` 和 Busy Timeout。每次 Observe 在一个 `BEGIN IMMEDIATE` 事务中写入 Fragment、Event、EvidenceRef、Blob 引用、Scope sequence 和 FTS 行；失败不会暴露数据库半写。

Blob 以明文 SHA-256 寻址，支持原子发布、两级目录分片、Zstd 和 AES-256-GCM。读取会验证 envelope、认证、长度和 checksum。GC 默认只报告，只有显式删除策略、grace period 和进程锁同时满足时才删除无引用 Blob。

详细设计见：

- [M1 存储架构](docs/architecture/storage.md)
- [ADR 0001：使用 Rust 构建可移植内核](docs/adr/0001-rust.md)
- [ADR 0002：以 SQLite 作为本地权威存储](docs/adr/0002-sqlite-first.md)
- [ADR 0003：原始事件不可变，派生数据可重建](docs/adr/0003-immutable-events.md)
- [Point、Edge 与 Surface](docs/concepts/point-edge-surface.md)
- [模式分离与有界模式补全](docs/concepts/pattern-completion.md)
- [Memory Packet 协议](docs/protocol/memory-packet.md)
- [M3 巩固与派生重建](docs/architecture/consolidation.md)
- [访问控制与权限矩阵](docs/security/access-control.md)
- [Quarantine 与污染防护](docs/security/quarantine.md)
- [Retention、Export 与 Hard Delete](docs/security/retention.md)
- [M3 威胁模型](docs/security/threat-model.md)
- [ADR 0004：认知图是带证据的可重建派生结构](docs/adr/0004-derived-cognitive-graph.md)
- [ADR 0005：所有 Recall 必须有硬预算](docs/adr/0005-bounded-recall.md)
- [ADR 0006：Memory Packet 保持模型无关](docs/adr/0006-model-independent-memory-packet.md)
- [ADR 0007：巩固必须保守、增量且可恢复](docs/adr/0007-conservative-consolidation.md)
- [ADR 0008：治理采用纵深防御与可验证恢复](docs/adr/0008-defense-in-depth-governance.md)
- [产品原始需求](docs/product/original-requirements.zh-CN.md)
- [分阶段实施路线图](docs/superpowers/plans/2026-07-29-jiangcheng-memory-roadmap.md)

## 开发检查

```bash
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps
git diff --check
```

测试仅证明实际运行的平台和调用路径。当前 macOS ARM64 结果不代表 Linux、Windows、macOS x64 或移动平台已经完成验证。

## 许可

本项目为私有专有源码，保留所有权利。未经版权所有者事先书面许可，不得复制、修改、分发、再许可或用于创建衍生作品。
