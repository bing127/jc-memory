# 疆诚记忆设计规格

- **状态：** 已确认，待进入实施计划
- **日期：** 2026-07-29
- **产品名称：** 疆诚记忆
- **英文名称：** Jiangcheng Memory
- **仓库标识：** `jiangcheng-memory`（使用现有 `jc-memory` 目录作为仓库根目录）
- **命令行工具：** `jcmem`
- **守护进程：** `jcmemd`

## 1. 项目目标

疆诚记忆是一个独立于具体模型的 Memory-Native Agent Kernel 与神经认知记忆数据库。它在不依赖特定大语言模型的前提下，持续保存 Agent 的身份、原始证据、经验、偏好、项目状态和程序性知识。

职责边界必须保持清晰：

- Memory 保存过去，并从带证据的历史中保守地学习规律；
- LLM 只负责提出假设、预测、计划和行动候选；
- Runtime 负责应用策略、请求审批、选择行动和提交已验证结果；
- Tool 负责作用于现实环境并返回可观察结果。

更换模型提供方不得改变权威记忆数据，也不得把模型输出自动提升为已验证事实。

## 2. 产品与发布边界

当前目录直接作为仓库根目录。内部包名和发布标识统一使用 `jiangcheng-memory`，Rust crate 使用 `jc-memory-*` 前缀。

项目首个版本按私有源码管理：

- `LICENSE` 声明保留所有权利；
- 文档不把项目描述为开源项目；
- Release 工作流默认仅允许手动触发并生成私有构建产物；
- 不自动发布到 crates.io、npm、PyPI 或公开 GitHub Release。

系统默认本地优先，运行时不强制依赖 Python、外部 LLM 或云端向量数据库。Python 只用于模型训练、评测和 ONNX 导出。

## 3. 交付策略

实施采用分阶段纵向闭环。每个里程碑必须提供已集成、可运行、可测试的能力，不能只创建未接通的脚手架。

1. 协议、领域类型、不可变事件、SQLite/FTS5、BlobStore 和 Migration；
2. Point、Edge、Surface、状态版本、模式分离、有界召回、Explain 和 Memory Packet；
3. 记忆巩固、确定性神经模型 Fallback、ACL/隔离/审计、Snapshot、Restore 和 Rebuild；
4. Agent Runtime、能力注册、模型与工具闭环、HTTP、gRPC、Sidecar 和 MCP；
5. Node、Python、Go、Kotlin、Swift、Java SDK，Hermes、OpenClaw、Mem0、Graphiti、Dify 适配器，以及示例和契约测试；
6. ONNX 工具链、Benchmark、CI、私有发布产物和文档收口。

每个里程碑完成后，仓库必须保持可编译、可运行、可测试。目录仅在具备真实代码、测试、公共扩展接口、Fixture、文档或构建配置时创建。

## 4. 总体架构

依赖只能按以下方向流动：

```text
协议与领域类型
        |
不可变事件 + BlobStore + SQLite
        |
情景记忆 / 语义记忆 / 图 / Surface / 激活
        |
Recall + Explain + Consolidation + Security
        |
memory-core 公共嵌入式门面
        |
Runtime / Sidecar / Server / MCP
        |
SDK / Adapter / Example / Benchmark
```

`memory-core` 是高层嵌入式 API 的唯一公共门面。下层 crate 不得反向依赖 Service、SDK 或 Adapter。跨进程共享的协议类型在 `memory-protocol` 和 Protobuf 定义中进行版本化。

首个版本使用 SQLite WAL 作为物理存储，使用 FTS5 提供本地全文检索，使用内容寻址 BlobStore 保存大内容和二进制数据。神经能力是可选插件，并始终提供确定性本地 Fallback。

## 5. 时间认知模型

类型、持久化和文档必须严格区分：

1. 过去的可验证证据；
2. 当前观察；
3. LLM 对未来的假设；
4. Runtime 决策；
5. 工具行动；
6. 实际结果；
7. 结果验证。

一次交互形成不可变状态转移记录，内容包括当前观察、召回证据、模型假设、Runtime 选择、工具行动、实际结果和验证状态。记忆巩固只能在其上增加结构，不能覆盖原始记录。

## 6. 权威领域模型

`CognitiveFragment` 是最小输入单元。它包含 Namespace，以及可选的 Tenant、User、Agent、Project、Session 和 Trace Scope；同时包含种类、内容、来源、视角、发生时间、观察时间、置信度、重要度、有效性、安全标签、证据和元数据。

`FragmentKind` 覆盖 Observation、Claim、Inference、Prediction、Decision、Action、ToolCall、ToolResult、Outcome、Correction、Constraint、Preference、Fact、Failure、Success、Plan、Approval、Rejection 和 StateTransition。`ValidityStatus` 区分 Unverified、Verified、Disputed、Superseded、Invalidated 和 Quarantined。

下列数据属于不可丢失的权威记录：

- 原始 Fragment 和 Event；
- 内容 Blob 和 EvidenceRef；
- 显式纠正与验证事件；
- Scope、权限、安全标签和审计记录；
- 状态转移和状态版本；
- 人工批准和拒绝。

摘要、Embedding、图关系或 Surface 不得静默覆盖这些记录。

## 7. Point、Edge 与 Surface

Point 是由一个或多个原始 Fragment 支撑的原子认知变化。Edge 是带类型、方向、有效时间、权重、置信度、来源、模型版本、人工确认标志和工具验证标志的关系。共同出现只能产生低置信候选关联，不能直接生成已验证因果边。

在线边权更新必须有界且可审计。强化和弱化都要记录触发它的成功或失败证据，并把权重限制在明确范围内。

Surface 是有版本历史的动态认知空间，包含语义中心、成员 Point 和 Edge、激活阈值、时间范围、当前状态、历史状态、置信度、边界、证据和模型版本。一个 Point 可以属于多个 Surface。合并、分裂、冻结、失活、冲突检测、成员更新和中心更新都必须保留历史版本。

## 8. 存储与事务语义

SQLite 必须启用 WAL、外键、Busy Timeout、Migration、事务、完整索引、FTS5 和完整性检查。逻辑表覆盖需求中定义的 Fragment、Event、图、状态版本、Recall、Transition、Capability、Approval、Audit、Snapshot、Quarantine 和 Migration 数据。

权威写入遵循以下顺序：

```text
校验 Scope 和 ACL
-> 持久化 Fragment 与内容引用
-> 持久化 Event 或 Transition
-> 持久化 EvidenceRef
-> 提交 SQLite 事务
-> 增量生成或排队生成可重建派生数据
```

Blob 写入采用临时文件、SHA-256 校验、原子重命名，再提交数据库引用。数据库事务失败时最多留下无引用 Blob，由可审计 GC 回收。启动恢复负责检查引用、校验和与中断任务。

Correction、Contradiction、Supersession 和普通 Forget 都通过追加新事件和关系实现。经过治理授权的彻底删除会清理权威记录、Blob 和全部派生数据，并在审计日志中留下不包含敏感正文的删除凭证。

可重建数据包括 Embedding、FTS/向量索引、候选 Edge、Surface Member、摘要和缓存。

## 9. 状态与冲突语义

状态使用带有效时间范围和支撑证据的不可变版本链。`CurrentState` 返回当前有效且对调用者可见的最新版本；`HistoricalState` 按指定时间计算历史版本。同一时点存在可见冲突证据时，Recall 返回冲突和缺失证据，不得静默选择一个事实。

Prediction 始终保持预测身份。只有后续带真实证据的验证事件才能将其标记为已验证或已否定。

## 10. Recall 与 Explain

所有召回模式共用同一条有界流水线：

```text
Scope/ACL 检索前过滤
-> Exact、FTS 和 Vector 种子检索
-> 有界候选集
-> 按 Intent 选择 Edge 类型
-> 有限跳数局部扩散激活
-> Surface 激活
-> 证据完整性检查
-> 重排并构造最小证据子图
-> Scope/ACL 返回前复核
-> Memory Packet 与 Explain Trace
```

配置必须限制种子数量、每层邻居数量、跳数、总节点数和执行时间。任何 Recall 模式都不得对全图执行无界传播。

确定性 Fallback 提供稳定的词法向量、基于规则的路由、分类和重排，从而保证离线运行与可重复测试。ONNX 模型可以改进候选选择和排序，但不能改变权威数据的 Validity。

Memory Packet 与具体模型无关。Explain 记录起始种子、经过路径、激活 Surface、最终证据、推断关系、已验证事实、冲突、覆盖率和缺失证据。

## 11. 记忆巩固与神经插件

Consolidation 通过可恢复、有 Checkpoint、受资源限制的后台任务执行 Replay、Deduplication、Edge 调整、Surface Discovery/Merge/Split、Semantic Promotion、Conflict Detection、Stale State Detection、Cold Tier Movement 和 Rebuild。任务默认采用保守策略并进行增量局部维护。

神经接口覆盖 FragmentEncoder、EventClassifier、ImportancePredictor、EdgePredictor、QueryRouter、Reranker、SurfaceAssigner 和 ContradictionDetector。每个实现必须声明 Manifest、版本、校验和、输入输出 Schema、设备策略、批处理、超时和熔断行为。

仓库提供确定性 Rust Fallback 和小型可重复 ONNX Fixture。Python 训练与导出代码必须验证导出前后的输出一致性，不提交虚构的大模型权重。

## 12. Runtime 与能力系统

Agent Runtime 使用显式、可恢复的状态机：

```text
Observing -> Recalling -> Reasoning -> AwaitingApproval -> Executing
          -> Committing -> Completed
```

附加状态覆盖 WaitingUser、Paused、Failed、Cancelled、Retry 和 Timeout。每一步都记录 Transition、输入输出引用、重试次数、超时和 Trace。恢复时从最近已提交步骤继续；没有幂等保证的工具行动不得在没有显式恢复决策时重复执行。

模型输出解析为 Hypothesis、Prediction、Plan、DecisionCandidate、ToolRequest 或 FinalResponse。工具执行前必须经过 Policy 和 Approval。只有已验证 Outcome 可以形成事实记忆。

Prompt、Skill、Tool、MCPTool、Workflow 和 Policy 实现统一的版本化 Capability 契约。执行统计形成程序记忆并影响后续排序，但仍然是统计证据，不能冒充事实。

## 13. 安全与治理

所有核心操作都必须携带 `AccessContext`，其中包含 Namespace、Tenant、User、Agent、Project、Principal 和 Grant。存储查询层在检索前执行权限过滤，Memory Packet 构造完成后再次校验。上层服务不得绕过核心过滤。

未验证模型输出、不可信导入、可疑工具结果、Prompt Injection 内容和 Memory Poisoning 指标可以进入 Quarantine。Quarantine 内容默认不参与正常 Recall 和 Semantic Promotion，只有授权审查可以改变其状态。

Security Label、可插拔 Key Provider、可选 AES-GCM Blob 加密、Retention Policy、Export、Hard Delete、派生数据清理和重新巩固都是一等能力。审计记录不得复制敏感正文。

## 14. 服务与协议

- Embedded Rust 是所有调用模式的语义基准；
- Sidecar 复用同一核心。Unix 类系统使用 Unix Domain Socket；Windows 首版通过传输抽象使用受本地 Token 保护的 Loopback。Named Pipe 是兼容扩展，在完成实现和测试前不得宣称支持；
- Server 使用 Axum 提供 REST，使用 Tonic 提供 gRPC，并提供生成的 OpenAPI、API Key、多租户、Request ID、Trace ID、限流、Metrics、结构化日志、TLS 配置和优雅关闭；
- MCP 实现标准协议封装，暴露 Observe、Recall、Commit、Correct、Explain、Forget 和 Stats，并返回结构化 Memory Packet。

协议错误使用稳定错误码和结构化详情。REST、gRPC、MCP 与 Embedded 调用执行同一套核心 Command，避免语义漂移。

## 15. SDK 与适配器

共享 Protobuf/OpenAPI 契约和版本化 Fixture 定义 SDK 行为。Node 和 Python 提供嵌入式与远程模式；Go 提供远程和 C ABI 集成；Kotlin、Swift 和 Java 按需求提供原生绑定和/或远程模式。各 SDK 暴露符合语言习惯的类型化异步、取消和错误接口。

Hermes、OpenClaw、Mem0、Graphiti 和 Dify Adapter 将来源记录映射为 Fragment，并保留 Provenance、原始置信度、时间有效性和 Scope。无法访问真实外部平台时，使用 Fixture 和契约测试，并在文档中记录被测试的外部版本；不得把 Fixture 兼容性描述为实机验证。

## 16. 测试与 Benchmark

测试按风险分层：

- crate 单元测试、属性测试、并发测试和错误路径测试；
- 事务回滚、Migration、损坏、崩溃恢复和 Rebuild 测试；
- 七个产品验收场景的确定性集成测试；
- Scope 权限矩阵、跨 Tenant 泄漏、注入、污染和删除测试；
- Embedded、REST、gRPC 和 MCP 的语义一致性测试；
- SDK 共享契约 Fixture；
- 确定性 Fallback 和 ONNX 导出/运行一致性测试；
- 普通 CI 中的 Benchmark Smoke Test，以及显式触发的百万 Fragment 长任务。

Benchmark Harness 覆盖 LongMemEval、LoCoMo、Cross-Year、Model Swap、Memory Poisoning 和 Million Fragments，并输出产品需求中的全部指标。外部数据集只能通过显式、带校验和的安装步骤下载；生成式 Fixture 必须完全确定。

每个里程碑运行聚焦测试及当时适用的全仓门禁。最终 Rust 门禁为：

```bash
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo doc --workspace --no-deps
```

额外门禁包括各 SDK 测试、依赖审计、许可证策略检查、Protobuf 兼容检查、Benchmark Smoke Test 和 `git diff --check`。

## 17. CI 与私有发布

CI 配置 Linux、macOS 和 Windows 的 Rust 及对应 SDK 检查。Release 工作流通过手动私有任务构建所需桌面端、Server 和语言包产物。移动端签名和应用商店发布不属于首个版本，但必须提供完整构建脚本和独立工作流。

最终报告必须明确区分：

- 当前主机已经真实运行成功的命令；
- 仅存在 CI 配置但尚未实际执行的检查；
- 静态或 Fixture 契约兼容性；
- 仍需对应平台、签名环境、硬件、数据集或外部服务验证的能力。

## 18. 文档与架构决策

中文和英文 README 提供五分钟本地快速开始。架构、概念、协议、SDK、部署、安全、Benchmark、数据迁移、故障恢复、版本升级、威胁模型和性能调优文档必须与代码一同版本化。

ADR 至少记录选择 Rust、选择 SQLite、LLM 独立于记忆主体、原始事件不可变、预测与事实隔离、快慢双记忆、局部有界传播和派生索引可重建等决策。

## 19. 验收标准

只有完成以下验证，才能宣称实现完成：

1. 通过有界模式补全召回跨年原始证据；
2. 状态替换后正确回答当前状态、历史状态、变更时间和变更原因；
3. 实际失败结果否定预测，而不是把预测保存为成功事实；
4. 从多次 Outcome 中形成带证据的程序经验；
5. 更换模型 Provider 后记忆与证据保持一致；
6. 跨 Tenant 隔离测试中不存在任何召回泄漏；
7. 在数据库、Blob、索引和 Consolidation 周围注入故障后能够一致恢复。

最终交付清单必须说明已实现模块、运行命令、Schema、协议、SDK 示例、实际运行的测试和 Benchmark、实际构建的平台、外部验证缺口、已知风险、未解决的安全或一致性问题，以及是否仍存在占位实现。

## 20. 非目标与硬性约束

- 首个版本不自行实现 B-Tree、WAL 或基础大语言模型；
- 运行时不强制依赖云基础设施或 Python；
- 摘要、Embedding、预测 Edge 和 LLM 文本不得成为权威证据；
- 无法访问的外部平台不得宣称已完成实机兼容验证；
- 不为复刻目录树而创建没有调用链的文件；
- 核心逻辑不使用 `unsafe`。绑定层确需使用时，必须隔离代码、说明不变量并增加边界测试。
