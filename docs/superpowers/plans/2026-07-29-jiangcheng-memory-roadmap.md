# 疆诚记忆全仓交付路线图

> **面向执行代理：** 每个里程碑必须先使用对应的详细实施计划；执行时必须使用 `superpowers:subagent-driven-development`（推荐）或 `superpowers:executing-plans`，并按复选框逐项跟踪。

**目标：** 将已批准设计拆成六个有依赖顺序、可独立验收、始终保持仓库可运行的交付里程碑。

**架构：** 先建立不可变证据与本地存储，再构建认知图和召回，随后加入巩固、安全、Runtime、服务、SDK、适配器、模型与发布能力。每个里程碑都从前一阶段的已测试公共接口继续，避免为尚未验证的接口提前生成大量脚手架。

**技术栈：** Rust Stable 2024、Tokio、Serde、SQLite WAL/FTS5、Protobuf、gRPC、HTTP/OpenAPI、MCP、ONNX Runtime、TypeScript、Python、Go、Kotlin、Swift、Java、GitHub Actions。

---

## 1. 计划组合

| 里程碑 | 详细计划文件 | 独立验收结果 |
| --- | --- | --- |
| M1 基础可信存储 | `docs/superpowers/plans/2026-07-29-m1-foundation-implementation.md` | 可离线写入、读取并校验不可变 Fragment/Event，SQLite 与 BlobStore 可恢复 |
| M2 认知图与召回 | `docs/superpowers/plans/2026-07-29-m2-cognitive-recall-implementation.md` | Point/Edge/Surface、状态版本、十种 Recall、Explain 与 Memory Packet 闭环 |
| M3 巩固与治理 | `docs/superpowers/plans/2026-07-29-m3-consolidation-security-implementation.md` | 保守巩固、确定性 Neural Fallback、ACL 双重过滤、审计、快照恢复与重建 |
| M4 Agent 与服务 | `docs/superpowers/plans/2026-07-29-m4-runtime-services-implementation.md` | 模型/工具/审批闭环经 Embedded、REST、gRPC、Sidecar、MCP 一致运行 |
| M5 SDK 与适配器 | `docs/superpowers/plans/2026-07-29-m5-sdks-adapters-implementation.md` | 六种 SDK 和五种适配器通过共享 Fixture 契约测试 |
| M6 模型、评测与发布 | `docs/superpowers/plans/2026-07-29-m6-models-benchmarks-release-implementation.md` | ONNX 工具链、Benchmark、跨平台 CI、私有发布和完整文档通过最终门禁 |

后续详细计划不提前固化内部函数和行号，因为这些必须依据上一阶段的真实公共接口编写；里程碑范围、目标路径和验收条件在本路线图中已经锁定。

## 2. 全局执行规则

1. 每个里程碑开始前，先核对 `git status`、`AGENTS.md`、上一阶段验收报告和当前公共 API。
2. 所有行为变更按 TDD 执行：先写失败测试，确认失败原因正确，再写最小实现，随后运行聚焦测试。
3. 每个 Task 独立提交；提交前运行该 Task 的聚焦测试和 `git diff --check`。
4. 原始 Fragment/Event、EvidenceRef、显式验证、Scope、权限、审计和审批是权威数据；派生数据不得反向覆盖它们。
5. 所有 Recall 在存储查询前和返回 Memory Packet 前执行权限过滤。
6. LLM 输出只能进入 Hypothesis、Prediction、Plan、DecisionCandidate、ToolRequest 或 FinalResponse；事实只能来自验证事件。
7. 外部平台不可用时使用版本化 Fixture 与契约测试，并在报告中明确标注未做实机验证。
8. 不提交 `TODO`、`unimplemented!()`、空函数、伪造 Benchmark 结果或没有调用链的目录。
9. 核心逻辑禁止 `unsafe`；绑定边界确需使用时必须隔离、说明不变量并增加边界测试。
10. 每个里程碑结束时更新 `CHANGELOG.md`、相关 ADR、运行命令和真实验证边界。

## 3. M1：基础可信存储

**范围：** Monorepo 基线、协议类型、`CognitiveFragment`、不可变 Event、SQLite WAL/FTS5、完整初始 Schema、Migration、内容寻址 BlobStore、事务化 Observe、完整性与恢复检查。

**主要目录：**

- `crates/memory-protocol/`
- `crates/memory-events/`
- `crates/memory-storage/`
- `crates/memory-core/`
- `docs/adr/`
- `docs/architecture/`

**验收命令：**

```bash
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo doc --workspace --no-deps
git diff --check
```

**完成定义：** 权威事件与 Blob 在正常写入、事务回滚、并发写入和模拟中断后保持一致；FTS 和其他派生数据可重建；工作区无占位实现。

## 4. M2：认知图、状态与召回

**范围：**

- `memory-episodic`：快速事件写入、状态链、模式分离和模糊线索；
- `memory-semantic`：带证据的稳定事实、约束、偏好和程序经验；
- `memory-graph`：Point、Edge、在线有界权重调整和局部子图；
- `memory-surface`：显式、动态和临时 Surface，以及合并、分裂、冻结、失活和版本历史；
- `memory-activation`：Exact、Semantic、Temporal、Causal、Entity、Associative、Procedural、CurrentState、HistoricalState、DeepReconstruction；
- `memory-core`：Recall、Correct、Reinforce、Weaken、Supersede、Contradict、Forget、Explain 和 Memory Packet。

**关键验收：**

- “PostgreSQL 改为 MySQL”能够区分当前状态与历史状态；
- 共同出现不生成已验证因果；
- Recall 受种子数、邻居数、跳数、节点数和超时限制；
- Explain 返回起始种子、路径、Surface、证据、推断和缺失证据；
- 跨年 Fixture 能从不同措辞恢复原始证据。

## 5. M3：巩固、神经 Fallback 与安全治理

**范围：**

- `memory-consolidation` 的可暂停、可恢复、可重试、有 Checkpoint 的增量 Job；
- Replay、Deduplication、Edge Reinforcement/Weakening、Surface Discovery/Merge/Split、Semantic Promotion、Conflict Detection、Stale State Detection、Cold Tier Movement 和 Rebuild；
- `memory-neural` 的八个插件 Trait、确定性本地 Fallback、Manifest、版本、校验和、批处理、超时、熔断和设备选择；
- `memory-security` 的 Scope、ACL、Security Label、Key Provider、审计、注入隔离、污染检测、Retention、Quarantine、Export 和 Hard Delete；
- Snapshot、Restore、派生索引清理和全量/局部 Rebuild。

**关键验收：**

- 未验证推断和恶意内容不能参与默认召回或语义提升；
- 两个 Tenant 的相似内容不存在交叉种子、路径或 Surface；
- 反复成功/失败形成带证据的统计经验，但不生成绝对事实；
- Snapshot/Restore 后权威数据哈希一致，派生索引可重新生成。

## 6. M4：Agent Runtime 与服务模式

**范围：**

- `memory-runtime` 的显式状态机、模型 Provider、OpenAI 兼容 HTTP、通用 HTTP、自定义 Provider；
- Tool/Skill/Prompt/MCP/Workflow/Policy Registry、Policy Engine、Approval Manager、Completion Judge；
- WaitingUser、Pause/Resume、取消、超时、重试、Trace 和非幂等工具恢复决策；
- `memory-sidecar` 的本地 Token、Unix Domain Socket、Windows Loopback 传输抽象、文件锁、健康检查和优雅关闭；
- `memory-server` 的 Axum REST、Tonic gRPC、OpenAPI、API Key、多租户、限流、Metrics、TLS 配置和优雅关闭；
- `memory-mcp` 的七个结构化工具。

**关键验收：**

- 确定性模型 Fixture 与确定性 Tool 完成 Observe -> Recall -> Reason -> Approve -> Execute -> Commit；
- “测试应该通过”的 Prediction 被实际失败 ToolResult 标记为 Disproved；
- Embedded、REST、gRPC 和 MCP 对同一 Fixture 返回语义一致结果；
- 中断后不自动重复非幂等工具调用。

## 7. M5：多语言 SDK 与兼容适配器

**范围：**

- Node/TypeScript：napi-rs、HTTP/gRPC、Promise、Electron 示例和私有 npm 构建；
- Python：PyO3、HTTP/gRPC、同步/异步 API、类型注解和 wheel；
- Go：HTTP/gRPC、C ABI、Context 取消和模块测试；
- Kotlin/Android：UniFFI/JNI、协程、AAR 和 Instrumented Test 配置；
- Swift/iOS/macOS：UniFFI、async/await、Swift Package 和 XCFramework；
- Java：JVM API、gRPC、Gradle 和测试；
- Hermes、OpenClaw、Mem0、Graphiti、Dify 的来源保真映射和版本化 Fixture。

**关键验收：** 所有可在当前主机运行的 SDK 使用同一协议 Fixture 完成 Observe/Recall/Commit/Explain；不可用平台通过独立 CI 工作流和静态契约验证，并清楚记录未验证边界。

## 8. M6：模型、Benchmark、CI 与私有发布

**范围：**

- PyTorch 数据加载、配置、训练、Checkpoint、ONNX 导出与一致性验证；
- 小型许可清晰的测试模型和确定性 Fixture；
- LongMemEval、LoCoMo、Cross-Year、Model Swap、Memory Poisoning、Million Fragments Harness；
- 全部质量指标、JSON/Markdown 报告和可重复数据生成；
- Linux/macOS/Windows CI，Node/Python/Go 测试，Protobuf 兼容，安全审计，依赖策略和 Benchmark Smoke；
- Windows x64、macOS x64/ARM64、Linux x64/ARM64、Node 原生包、Python Wheel、Go SDK、Server 和 MCP 私有产物；
- 中英文 README、部署、安全、SDK、训练、Benchmark、恢复、迁移、升级、威胁模型和性能文档。

**关键验收：**

- 当前主机所有可运行门禁通过；
- 百万 Fragment 长测可显式启动、支持中断并输出 P50/P95/P99、吞吐、存储放大和巩固成本；
- 模型切换不改变证据、当前状态或身份；
- 最终交付报告逐项说明真实运行、CI 配置、外部验证缺口、已知风险和占位实现扫描结果。

## 9. 原始需求覆盖映射

| 原始需求章节 | 落地里程碑 |
| --- | --- |
| 一至五：定位、约束、技术栈、仓库结构、时间模型 | M1 建立边界，M2-M6 持续验证 |
| 六：CognitiveFragment | M1 |
| 七至十一：Point/Edge/Surface、双系统、模式、API、Memory Packet | M2 |
| 十二至十三：Runtime 与 Capability | M4 |
| 十四：SQLite 与 BlobStore | M1，M3 补齐 Snapshot/Rebuild/治理删除 |
| 十五至十七：Neural、Consolidation、安全 | M3，M6 完成训练工具链 |
| 十八至十九：Embedded/Sidecar/Server/MCP | M4 |
| 二十至二十一：SDK 与 Adapter | M5 |
| 二十二至二十三：Benchmark 与指标 | M6 |
| 二十四：测试体系 | 每个里程碑持续加入，M6 最终收口 |
| 二十五：CI/CD | M6；早期里程碑先建立必要 Rust 门禁 |
| 二十六：文档与 ADR | 每个里程碑同步维护，M6 收口 |
| 二十七：实现顺序 | 本路线图六阶段依赖顺序 |
| 二十八：七个验收场景 | M1 崩溃基础，M2 状态/跨年，M3 隔离/经验，M4 预测/模型闭环，M6 全量回归 |
| 二十九：最终交付清单 | M6 |

## 10. 里程碑关闭门禁

每个里程碑只能在以下条件全部满足后关闭：

- 详细计划中的复选框全部完成；
- 聚焦测试和当时适用的全仓测试真实运行成功；
- `cargo fmt --check`、Clippy、文档和 `git diff --check` 无错误；
- 新增公共 API 有使用路径、测试和文档；
- 没有 `TODO`、`unimplemented!()`、空实现或伪造结果；
- 未验证平台和外部服务被明确记录；
- `git status --short` 只包含用户明确保留的无关修改；
- 生成下一里程碑详细计划时，以当前已提交接口为准。
