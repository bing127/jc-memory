# Changelog

本文件记录疆诚记忆各私有版本的重要变化。

## [Unreleased]

### Added

- 建立由 `memory-protocol`、`memory-events`、`memory-storage` 和 `memory-core` 组成的 Rust Workspace。
- 定义带 Scope、Provenance、时间、Validity、安全标签、EvidenceRef 和元数据的 CognitiveFragment 协议。
- 实现不可变 EventDraft/MemoryEvent、规范化 SHA-256 和 Prediction 非 Verified Fact 校验。
- 加入 SQLite WAL/FTS5 初始 Schema、版本化 Migration、Foreign Keys、Busy Timeout 和完整性检查。
- 实现 `JCB1` 内容寻址 BlobStore，覆盖原子发布、目录分片、Zstd、AES-256-GCM、大小限制和损坏检测。
- 实现 Fragment、Event、EvidenceRef、BlobRef、Scope sequence 与 FTS 的事务仓储，以及幂等写入和冲突诊断。
- 实现只读完整性审计、默认 dry-run 的孤儿 Blob GC 和可回滚的 FTS rebuild。
- 提供嵌入式 `MemoryEngine` 的 Observe、Fragment lookup 和 Health API。
- 增加并发 sequence、同 ID 幂等、跨 Tenant 隔离、事务回滚、Busy Timeout、损坏、Blob 孤儿和恢复测试。
- 增加中文 M1 存储架构文档与 Rust、SQLite、不可变事件 ADR。

### M1 Verification

- 验证主机：macOS 26.6，Apple Silicon，`aarch64-apple-darwin`。
- Rust：`rustc 1.97.1 (8bab26f4f 2026-07-14)`；Workspace 最低声明版本为 1.85。
- SQLite：系统 `sqlite3 3.51.0`，FTS5 独立探针成功；仓库 Migration/FTS 测试覆盖实际 SQLx 调用路径。
- 以下耗时为本机已有 Cargo 构建缓存下的 wall-clock 结果，不作为跨平台性能指标。
- `cargo fmt --check` 通过，耗时 0.30 秒。
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` 通过，耗时 0.58 秒。
- `cargo test --workspace --all-features` 共 59 项通过、0 失败，耗时 2.27 秒。
- `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps` 通过，耗时 0.26 秒。
- README Rust 快速开始已在独立临时 Cargo 工程中离线编译并实际运行成功。
- Task 10 并发与恢复测试曾连续运行 20 轮，共 220 项通过。
- Linux、Windows、macOS x64 和移动平台尚未在本验收中实机验证。
