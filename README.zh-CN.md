# 疆诚记忆

疆诚记忆是独立于具体大语言模型的 Memory-Native Agent Kernel 与神经认知记忆数据库。

当前代码实现 M1 基础可信存储：认知片段协议、不可变事件、SQLite WAL/FTS5、内容寻址 BlobStore、事务写入、完整性审计和恢复基础设施。项目目前为私有源码，不计划公开发布。

## 当前可用范围

- `jc-memory-protocol`：类型化 ID、MemoryScope、CognitiveFragment、Event 和 EvidenceRef；
- `jc-memory-events`：Fragment 校验、认知阶段映射和稳定内容哈希；
- `jc-memory-storage`：SQLite Migration、事务仓储、FTS5、`JCB1` Blob、审计、GC 和 FTS rebuild；
- `jc-memory-core`：本地嵌入式 `MemoryEngine`，公开 Observe、按 ID 读取 Fragment 和 Health。

原始 Fragment、Event 和 EvidenceRef 是权威记录。摘要、Embedding、图关系、Surface 和全文索引不得覆盖它们。Prediction 不允许设置 `ValidityStatus::Verified`；Inference 和 Plan 始终保持 FutureHypothesis 阶段，三者都不会被自动改写成 Fact。

以下能力属于 M2-M6，当前尚不可用：认知图与召回、巩固与遗忘、Runtime 与工具执行、HTTP/gRPC/MCP 服务、多语言 SDK、外部 Adapter、训练/ONNX、Benchmark 和发布流水线。初始 Schema 为部分后续能力预留了表，但表存在不代表业务调用链已经实现。

## 五分钟开始

需要 Rust 1.85 或更高版本。仓库当前使用 Rust 2024 Edition，不依赖外部 LLM、Python、数据库服务或网络 API。

先运行真实的嵌入式持久化测试：

```bash
cargo test -p jc-memory-core --test observe_persistence
```

下面是同一调用链的完整 Rust 示例。`tempfile::TempDir` 在变量离开作用域时清理数据；正式应用应把 `MemoryConfig::local` 指向持久目录。

```rust
use chrono::Utc;
use jc_memory_core::{HealthStatus, MemoryConfig, MemoryEngine};
use jc_memory_protocol::{
    CognitiveFragment, FragmentContent, FragmentId, FragmentKind, FragmentSource, MemoryScope,
    NamespaceId, Perspective, SecurityLabel, SourceType, ValidityStatus,
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

    let receipt = engine.observe(fragment.clone()).await?;
    drop(engine);

    let reopened = MemoryEngine::open(config).await?;
    let stored = reopened.fragment(receipt.fragment_id).await?;
    assert_eq!(stored, fragment);

    let health = reopened.health().await?;
    assert_eq!(health.status, HealthStatus::Healthy);
    assert_eq!(health.schema_version, 1);
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
