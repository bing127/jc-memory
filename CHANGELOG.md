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
- 增加 `memory-episodic`、`memory-semantic`、`memory-graph`、`memory-surface` 和 `memory-activation`，实现模式分离、状态版本、证据约束的语义提升、Point/Edge/Surface 与有界局部传播。
- 定义十种 `RecallMode`、固定 18 字段的模型无关 Memory Packet、确定性 `recommended_context` 和可追溯 Explain。
- 将 SQLite FTS5 与近期有界候选接入 Core Recall；候选按精确 Scope、历史截止时间、稳定顺序和硬 `LIMIT` 读取，再使用确定性词项、稀疏字符特征和实体匹配评分。
- 为 `MemoryEngine` 增加 Recall、Explain、Stats、Correct、Reinforce、Weaken、Supersede、Contradict 和 Forget；所有 Mutation 通过追加 Fragment/Event 表达，不改写原始记录。
- 增加 Cross-Year、状态历史、共同出现非因果、Prediction 隔离和 Core Recall 预算验收测试。
- 增加 Point/Edge/Surface、模式补全、Memory Packet 中文文档与 ADR 0004-0006。
- 增加精确 Scope 默认拒绝 ACL、Recall 查询前与返回前双重授权，以及跨 Tenant 隔离验收。
- 增加 Prompt Injection、跨 Scope Evidence、重复争议声明、可疑 ToolResult 和敏感数据的确定性 Quarantine；授权 Release/Reject 通过追加记录表达。
- 增加八个 Neural Trait、版本化 Manifest、三态熔断和完全离线的确定性 Fallback；派生输出保存 `model_version`。
- 增加 12 种保守巩固 Handler，以及带 Lease、Checkpoint、Pause/Resume/Retry/Cancel 和资源预算的持久化 Job Runner。
- 增加 Snapshot/Restore、固定阶段派生重建、治理 Export、先计划后执行的 Retention、Key Provider 契约和无正文凭证的 Hard Delete。
- 增加 M3 访问控制、Quarantine、Retention、威胁模型、巩固/重建架构文档与 ADR 0007-0008。
- 为 `MemoryEngine` 增加受 `Operation::Consolidate` 保护的 Job 提交、查询、运行、暂停、恢复、重试和取消门面，按持久化 `JobKind` 分派 12 类 Handler。
- 把有效 Surface、Procedure、Constraint、Preference 和相关 Failure 接入 Core Memory Packet，统一 Recall deadline、`max_nodes`、Evidence 去重和返回前 Scope 校验。
- 增加同键跨 Tenant 隔离、Core 重启、Snapshot/Restore、旧 64 字符派生 ID 失败关闭和显式 Rebuild 规范化验收，重建前后权威哈希不变。

## [0.1.0]

### Added

- 增加 M6 模型、Benchmark、跨平台 CI 和私有 Release 构建流水线。
- 私有 Release 产物包含 SHA-256 清单、CycloneDX SBOM、构建元数据和分离签名接口；移动端产物明确标记为未签名。

### M5 已实现范围

- 增加 Node、Python、Go、Kotlin、Swift、Java 的统一 JSON/HTTP SDK 契约与嵌入式边界，并保留当前 `memory-uniffi` 生成绑定未完成的验证边界。
- 增加 Hermes、OpenClaw、Mem0、Graphiti、Dify 五种来源保真 Adapter；覆盖 Provenance、Scope、时间区间、原置信度和 Import/Export round-trip。
- 增加 Graphiti temporal/inferred Edge、Dify Scope 与 HTTP/MCP MemoryPacket fail-closed 映射，以及无密钥 MCP Client 示例。
- 增加跨语言 SDK 门禁脚本、Embedded 绑定部署边界和 SDK/Adapter 中文支持矩阵。

### M4 已实现范围

- 增加可恢复 `RuntimeService`，固定编排 Observe、Recall、Model、Policy/Approval、Execute、Commit 和 Completed，并保留每阶段的 Scope、Trace、截止时间和 EvidenceRef。
- 增加 Server 的 HTTP/gRPC、YAML/环境变量配置、文件型 TLS、请求超时、并发/速率限制和 Docker/Kubernetes 资源；增加 Sidecar 的本地认证、单实例锁、生命周期检查点、优雅关闭、`jcmem`/`jcmemd` 和 Windows Named Pipe 代码路径。
- 增加 MCP 七类工具、结构化结果和经 Sidecar 的 `ExecuteAs` 主体转发；增加文件型 `KeyProvider` 的受限权限、原子发布、轮换元数据和损坏记录拒绝。
- 保留原始 Fragment、Event、EvidenceRef 的权威性，Prediction、Inference、Plan 和 ToolResult 不会自动升级为 Verified Fact。

### M4 Verification

- `cargo test --workspace` 通过；Runtime、Server、Sidecar、MCP、文件型 KeyProvider 和真实 Server 重启测试均通过。
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`、`RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps`、`cargo fmt --all -- --check` 和 `git diff --check` 通过。
- 协议一致性使用本地临时 SQLite Fixture 和真实本机进程验收；不把 Fixture 描述为 Windows、Docker、Kubernetes 或其他平台实机结果。
- 当前主机缺少 Windows MinGW 工具链、Docker、kubectl 和 Wine，因此 Windows 实机传输、容器构建/启动、Kubernetes 集群和独立部署进程组合 E2E 保留为待外部环境验收项。

### M3 已知边界

- M3 验证限定为本地 Rust、SQLite 和文件系统调用链；没有验证远程身份、HTTP/gRPC/MCP、TLS、KMS/HSM、云备份或外部模型。
- `Operation::Consolidate` 已接入 Embedded Core Job 门面；尚未扩展为远程 CommandBus、HTTP/gRPC 或 Web 命令。
- `MemoryEngine::rebuild_derived` 的 FTS 阶段真实重建索引；Embedding、Edge Candidate、Surface Member 和 Summary/Cache 阶段当前只验证本地表与 Scope 契约。
- State、Surface、Procedure、Constraint、Preference 和 PastFailure 已汇入 Core 的持久化 Memory Packet；进程内 Mutation 图尚未在 Core 重启时自动恢复。
- 当前结果不声明 Runtime、服务、SDK、Adapter、训练/ONNX、Benchmark、跨平台实机或公开发布已经完成。

### M3 Verification

- 验证主机：Darwin 25.6.0，Apple Silicon arm64；Rust `rustc 1.97.1`；Workspace 最低声明版本为 1.85。
- Task 9 聚焦验收覆盖 74 项：跨 Tenant 2 项、注入/污染 6 项、Snapshot/Restore 8 项、保守经验与 Handler 14 项、Job 恢复/预算 21 项、Hard Delete/Retention/Export 13 项、Neural Fallback 10 项；全部通过、0 失败。
- `cargo fmt --all -- --check` 通过。
- `CARGO_INCREMENTAL=0 cargo clippy --workspace --all-targets --all-features --offline -- -D warnings` 通过。
- `CARGO_INCREMENTAL=0 cargo test --workspace --all-features --offline` 共 210 项通过、0 失败，包含 1 项 compile-fail doctest。
- `RUSTDOCFLAGS='-D warnings' CARGO_INCREMENTAL=0 cargo doc --workspace --all-features --no-deps --offline` 通过。
- `git diff --check` 通过。
- 聚焦结果来自版本化本地 Fixture，不是外部平台、跨平台实机、模型质量或性能 Benchmark 结果。

### M2 已知边界

- 状态、语义和 Surface 模块已有领域实现与测试，但尚未全部汇入 Core 的持久化 Memory Packet。
- Mutation 派生图当前为进程内状态；权威元数据保留重建信息，但 Core 重启后的自动重建尚未接通。
- 当前跨措辞召回使用确定性词项和稀疏字符特征，不包含外部 Embedding、向量数据库或远程模型。
- 本里程碑不声明 M3-M6 的巩固治理、Runtime、服务、SDK、模型、Benchmark、跨平台实机或发布能力。

### M2 Verification

- 验证主机：Darwin 25.6.0，Apple Silicon，`aarch64-apple-darwin`。
- Rust：`rustc 1.97.1 (8bab26f4f 2026-07-14)`；Workspace 最低声明版本为 1.85。
- `cargo fmt --all --check` 通过。
- `CARGO_INCREMENTAL=0 cargo clippy --workspace --all-targets --all-features -- -D warnings` 通过。
- `CARGO_INCREMENTAL=0 cargo test --workspace --all-features` 共 121 项通过、0 失败。
- `CARGO_INCREMENTAL=0 cargo doc --workspace --no-deps` 通过。
- `git diff --check` 通过。
- Cross-Year 使用本地 SQLite Fixture 验证 2026 年不同措辞查询可返回 2018 年原始 FragmentId；这不是外部语义模型或大规模 Benchmark 结果。
- Linux、Windows、macOS x64 和移动平台未在本次 M2 验收中实机验证。

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
