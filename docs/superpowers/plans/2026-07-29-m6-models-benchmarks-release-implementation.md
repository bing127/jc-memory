# 疆诚记忆 M6 模型、Benchmark、CI 与私有发布实施计划

> **面向执行代理：** 必须使用 `superpowers:subagent-driven-development`（推荐）或 `superpowers:executing-plans` 逐 Task 执行本计划；所有步骤使用复选框跟踪。

**目标：** 完成可重复的小型神经模型训练/ONNX 部署、六套 Benchmark、全指标报告、跨平台 CI、私有发布产物和最终文档验收。

**架构：** Python 只负责训练、评测和 ONNX 导出；本地产品运行只加载带 Manifest/Checksum 的 ONNX 或确定性 Rust Fallback。Benchmark 统一生成规范化 Case/Prediction/Metric，普通 CI 跑 Smoke，百万 Fragment 作为可恢复长任务。

**技术栈：** Python 3.11+、PyTorch、ONNX/ONNX Runtime、Rust ort、Criterion、自定义 Harness、psutil、GitHub Actions、cargo-deny、cargo-audit、Buf、cross/cargo-zigbuild。

---

## 文件边界

- `models/training/`：数据、配置、训练和 Checkpoint。
- `models/evaluation/`：离线评测与指标。
- `models/export/`：ONNX 导出及一致性验证。
- `models/manifests/`、`models/onnx/fixtures/`：版本化 Manifest 和小型测试模型。
- `crates/memory-neural/src/onnx/`：Rust ONNX Runtime 实现。
- `benchmarks/harness/`：统一 Case/Runner/Metric/Report。
- `benchmarks/{longmemeval,locomo,cross-year,model-swap,memory-poisoning,million-fragments}/`：各套数据与 Runner。
- `.github/workflows/`、`scripts/{build,test,release,benchmark}/`：CI 与私有发布。

## Task 1：建立可重复 Python 模型工程

**文件：** `models/pyproject.toml`、`models/training/{config.py,data.py,model.py,train.py,checkpoint.py}`、`models/tests/`。

- [x] **步骤 1：写固定 Seed、Dataset Split 和 Checkpoint round-trip 失败测试。** 已覆盖配置安全、输入顺序无关的稳定切分、训练参数摘要和 Checkpoint 尾部篡改拒绝。
- [x] **步骤 2：运行 `python -m pytest models/tests/test_training.py -q`**，先确认 `models.training` 缺失红灯，再完成 4 个测试通过。
- [x] **步骤 3：实现 YAML/JSON `TrainingConfig` 的 schema 校验、全局 seed、按样本 ID 稳定 split、DataLoader worker seed；小型多任务网络覆盖 Router/Importance/Contradiction Fixture。**
- [x] **步骤 4：Checkpoint 保存 model/optimizer/config/schema_version/git_commit/metrics，不保存绝对路径或 Token；恢复时校验文件内 SHA-256、结构和配置。**
- [x] **步骤 5：运行 pytest 和一次 CPU 两 epoch 小训练，提交 `feat(models): add reproducible training pipeline`。** `models/.venv` 中 4 个测试、Ruff、编译检查和两 epoch Smoke 均已通过；实现已提交于 `3dded8f`。

## Task 2：实现 ONNX 导出、一致性验证与 Rust 部署

**文件：** `models/export/{export.py,verify.py}`、`models/manifests/test-model.json`、`models/onnx/fixtures/test-model.onnx`、`memory-neural/src/onnx/{session,provider}.rs`。

- [x] **步骤 1：写 PyTorch/ONNX 输出容差、Checksum、错误 Shape、Circuit Breaker Fallback 失败测试。** 已覆盖 Python 输出容差、Manifest/Checksum/Shape 拒绝和 Rust 确定性 Fallback 契约。
- [x] **步骤 2：运行 Python 与 Rust 聚焦测试**，先确认导出器和 `OnnxProvider` 缺失红灯，再完成 Python 6 个测试和 Rust 5 个测试通过。
- [x] **步骤 3：导出使用固定 opset、dynamic batch axis 和明确 input/output names；Manifest 包含 model_id、version、license、sha256、schema、opset、训练数据说明、设备支持。** 已生成并提交最小私有 Fixture 与 Manifest。
- [x] **步骤 4：Rust `OnnxProvider` 校验 Manifest/文件 checksum/Shape，支持 CPU Provider、批处理、超时、熔断；加载或推理失败自动返回确定性 Fallback 并记录原因。** 当前主机 CPU Session 已通过真实 Fixture 验证。
- [x] **步骤 5：运行一致性测试并提交 `feat(neural): add verified ONNX deployment`**。Python 6 个测试、Rust neural 全部测试、严格 Clippy、Rustfmt、Ruff 和当前主机 CPU Fixture 均已通过；测试模型体积小且许可证为 PRIVATE。

## Task 3：建立统一 Benchmark Harness 与指标

**文件：** `benchmarks/harness/{Cargo.toml,src/{case,runner,metrics,report,main}.rs}`、`tests/metrics.rs`。

- [x] **步骤 1：写 Recall@K、Evidence Coverage、Accuracy、Leakage、Latency Percentile 的已知输入失败测试。** 已先确认 crate 缺失红灯，再补充 5 个确定性 Fixture 测试，覆盖 Recall@K、Evidence Coverage、nearest-rank、19 个指标名称、JSON/Markdown 和 Runner 错误边界。
- [x] **步骤 2：运行 `cargo test -p jc-memory-benchmark-harness`**，预期 crate 缺失。已在实现前确认 `package ID specification ... did not match any packages` 红灯。
- [x] **步骤 3：实现：**

```rust
pub struct BenchmarkCase { pub id: String, pub setup: Vec<CognitiveFragment>, pub query: RecallQuery, pub expected: ExpectedAnswer, pub tags: BTreeSet<String> }
pub struct CaseResult { pub case_id: String, pub packet: MemoryPacket, pub latency: Duration, pub database_bytes: u64, pub process_peak_bytes: u64 }
pub trait Metric { fn name(&self) -> &'static str; fn evaluate(&self, cases: &[BenchmarkCase], results: &[CaseResult]) -> MetricValue; }
```

- [x] **步骤 4：实现需求中的 19 个命名输出（把 P50/P95/P99 分别计数），百分位使用 nearest-rank 并记录样本数；报告同时输出 schema-versioned JSON 和中文 Markdown。** 已实现 `jcmem.benchmark.v1` JSON、中文 Markdown、统一 CaseExecutor 和全部 19 个指标；当前测试只使用仓库内确定性 Fixture，不宣称外部数据集或生产负载验证。
- [x] **步骤 5：运行指标测试并提交 `feat(benchmarks): add unified harness and metrics`**。聚焦测试、严格 Clippy、Rustfmt、rustdoc 和 `git diff --check` 已通过；实现提交为 `b85082f feat(benchmarks): 增加统一 Benchmark Harness 与指标`，workspace 测试因当前主机既有 Python C API 链接环境缺失未通过，详见本轮交付说明。

## Task 4：实现 LongMemEval 与 LoCoMo Harness

**文件：** `benchmarks/{longmemeval,locomo}/{manifest.json,prepare.py,runner.rs,tests.rs}`、`scripts/benchmark/fetch-datasets.sh`。

- [x] **步骤 1：写最小许可 Fixture 的信息提取、跨 Session、时间、更新、拒答、单跳、多跳和因果失败测试。** 已新增 LongMemEval/LoCoMo `PRIVATE-FIXTURE`，覆盖上述标签并验证 Case ID、Session 数量和结构化拒答预期。
- [x] **步骤 2：运行两个 Smoke Test**，预期 Runner 缺失。已先确认 Harness 未暴露两个 Runner 的编译红灯，再补齐 Rust Fixture/Runner 测试；当前 4 个 Suite 测试通过。
- [x] **步骤 3：下载脚本要求显式接受数据集许可、固定 URL/version/SHA-256，下载到 gitignored cache；prepare 转换为 BenchmarkCase 并保留原 case ID。** 已新增两个 `manifest.json`、两个标准库 `prepare.py`、`scripts/benchmark/fetch-datasets.sh` 和 `.cache/benchmarks/` 忽略规则；本机已验证两个 Fixture 的许可门禁、规范化、复制和 SHA-256。
- [x] **步骤 4：拒答按 missing_evidence 和 abstention 判断，不能用模型措辞字符串匹配；完整数据缺失时只运行仓库内许可 Fixture 并标为 smoke。** Runner 只根据 Packet 的 Point/`missing_evidence` 判定拒答；Suite 标记 `smoke: true`，注入式 `run_smoke` 负责转换、执行和生成统一报告。
- [x] **步骤 5：运行 Smoke 并提交 `feat(benchmarks): add LongMemEval and LoCoMo runners`**。聚焦测试、严格 Clippy、Rustfmt、rustdoc、Python 编译、Shell 语法和 `git diff --check` 均通过；实现提交为 `c4ff0cc feat(benchmarks): 增加 LongMemEval 与 LoCoMo Runner`。

## Task 5：实现 Cross-Year、Model Swap 与 Memory Poisoning

**文件：** 三个 Benchmark 目录的 generator/runner/tests；`examples/model-swap/`。

- [x] **步骤 1：写 Generator 同 Seed 同哈希、不同 Seed 不同数据失败测试。** 已先确认三个 Task 5 模块缺失的编译红灯，再加入确定性 Generator、Model Swap 和 Poisoning Fixture 测试。
- [x] **步骤 2：运行 Smoke**，预期 Generator 缺失。已运行 Harness Task 5 Smoke；Cross-Year、Model Swap、Poisoning 测试均通过。
- [x] **步骤 3：Cross-Year 生成跨 1 天/1 月/1 年、状态多次变更、模糊线索、同名实体、相似事件和稀有事实；Manifest 保存 seed、case_count 和 digest。** 已实现固定 LCG、SHA-256、32 Case Smoke Manifest 和八类标签。
- [x] **步骤 4：Model Swap 使用至少两个 DeterministicModel 表达器，比较 Fragment/Evidence/CurrentState canonical hash，只允许 FinalResponse 文本不同；Poisoning 覆盖注入、跨 Tenant、未验证推断、重复错误、恶意 ToolResult 和权限绕过。** 已实现结构化 hash 比较和六类 fail-closed 污染场景。
- [x] **步骤 5：运行 Smoke 并提交 `feat(benchmarks): add temporal model-swap and poisoning suites`**。聚焦测试、严格 Clippy、Rustfmt、rustdoc 和 `git diff --check` 均通过；实现提交为 `de36a6a feat(benchmarks): 增加 Cross-Year、Model Swap 与污染套件`。

## Task 6：实现 Million Fragments 可恢复压力测试

**文件：** `benchmarks/million-fragments/{Cargo.toml,src/{generate,load,measure,checkpoint,main}.rs}`、`scripts/benchmark/million-fragments.sh`。

- [x] **步骤 1：写 1,000 Fragment 小规模 Checkpoint/Resume、崩溃恢复和报告失败测试。** 已覆盖 Checkpoint 往返、固定批次崩溃恢复、config digest 篡改、原始计数不一致和测量阶段缺失。
- [x] **步骤 2：运行 Smoke**，预期 Runner 缺失。已先确认 crate 缺失红灯，再补齐 Million Fragments Runner 和脚本 Smoke。
- [x] **步骤 3：Generator 流式产生至少一百万 Fragment，不把全数据保存在内存；批量写入每批提交 Checkpoint，Resume 校验 config digest 和最后 sequence。** 已实现流式 Generator、原子批次 Checkpoint、Resume sequence 和配置 Digest 校验。
- [x] **步骤 4：测量写吞吐、索引时间、数据库/Blob 字节、Recall P50/P95/P99、局部图加载、巩固耗时、RSS 峰值、崩溃恢复；预热与测量阶段分离。** 已接入实际写入吞吐计时；尚未采集的索引、存储、Recall、图、巩固、RSS 和恢复字段保持 `null`，报告明确测量边界。
- [x] **步骤 5：运行 1,000 Smoke；百万长测只在显式 `--fragments 1000000 --output ...` 下运行，提交 `feat(benchmarks): add resumable million-fragment stress test`**。聚焦测试、严格 Clippy、rustdoc、Shell 语法、Smoke 输出和百万缺少 output 的拒绝门禁均已通过；实现提交为 `4b22e04 feat(benchmarks): 增加可恢复 Million Fragments 压测`。百万真实长测未在本轮执行，未采集指标保持 `null`。

## Task 7：建立 Benchmark 结果真实性门禁

**文件：** `benchmarks/schema/report.schema.json`、`scripts/benchmark/{smoke,verify-report}.sh`、`docs/benchmarks/methodology.md`。

- [x] **步骤 1：写篡改 config digest、缺失 raw counts、虚构 platform 字段被拒绝的测试。** 已覆盖配置 Digest 篡改、原始计数缺失、未知 platform 字段和 dirty 长测门禁。
- [x] **步骤 2：运行 Validator**，预期失败。已先确认验证器缺失导致契约测试红灯，再实现严格 JSON 审计验证。
- [x] **步骤 3：报告必须包含 git commit、dirty 标志、Rust/Python/OS/CPU、数据 digest、配置、开始结束时间、原始计数、派生指标和是否 smoke；dirty 长测默认拒绝，除非显式标记 non-release。** 已实现 `jcmem.benchmark.audit.v1` Schema、Digest 重算、时间/计数/平台校验和 dirty 长测门禁。
- [x] **步骤 4：Markdown 只由 JSON 报告生成；仓库只提交可重复 Smoke 基线，不提交未实际运行的百万结果。** 已新增 Smoke 生成脚本和中文方法论；Smoke 报告写入 ignored 的 `target`，生产结果未执行时明确为 `null`/0，不提交百万结果。
- [x] **步骤 5：运行验证并提交 `test(benchmarks): make reports reproducible and auditable`**。契约测试、Smoke、Rustfmt、严格 Clippy、rustdoc、Shell/JSON 校验和 `git diff --check` 均已通过；实现提交为 `18b681f test(benchmarks): make reports reproducible and auditable`。

## Task 8：实现跨平台 CI 与供应链门禁

**文件：** `.github/workflows/{rust,sdks,proto,security,benchmarks,mobile}.yml`、`deny.toml`、`.github/dependabot.yml`。

- [ ] **步骤 1：使用 actionlint/yamllint 验证失败的最小工作流 Fixture。**
- [ ] **步骤 2：先运行本地 YAML/Action 校验，预期缺少工作流。**
- [ ] **步骤 3：Rust 矩阵覆盖 ubuntu/windows/macos，执行 fmt、clippy、test、doc；SDK Workflow 执行 Node/Python/Go/Kotlin JVM/Swift/Java；Proto 执行 Buf lint/breaking/generated-diff。**
- [ ] **步骤 4：Security 执行 cargo-audit、cargo-deny、npm/pip/go 审计和 Secret Scan；Benchmark 跑 Smoke；Mobile 独立执行 Android/iOS 可用构建。第三方 Action 固定完整 commit SHA。**
- [ ] **步骤 5：运行 actionlint 并提交 `ci: add cross-platform quality and supply-chain gates`**。

## Task 9：实现私有 Release 构建

**文件：** `.github/workflows/release.yml`、`scripts/release/{version,build,package,checksums,verify}.sh`、各 SDK 发布配置。

- [ ] **步骤 1：写版本不一致、dirty tree、缺失 Artifact、Checksum 不符的脚本测试。**
- [ ] **步骤 2：运行脚本测试**，预期 Release 命令缺失。
- [ ] **步骤 3：workflow_dispatch 输入 version/ref/prerelease；验证 tag/版本/CHANGELOG 一致且 License 为私有；构建 jcmem/jcmemd/server/mcp、Node 包、Python Wheel、Go SDK、Java/Kotlin 包、Swift Package/XCFramework，以及 Windows x64、macOS x64、macOS ARM64、Linux x64、Linux ARM64 目标。**
- [ ] **步骤 4：每个产物生成 SHA-256、SBOM、构建元数据和签名接口；只上传私有 Actions Artifact，不调用 crates.io/npm/PyPI/public Release；移动端单独产出未签名构建并标注。**
- [ ] **步骤 5：在当前主机构建可用目标并运行 verify，提交 `build: add manual private release pipeline`**。

## Task 10：完成全部文档与最终验收报告

**文件：** README、README.zh-CN、`docs/{architecture,concepts,protocol,sdk,deployment,security,benchmarks,adr}/`、`docs/release/verification-report.md`、CHANGELOG。

- [ ] **步骤 1：补齐项目介绍、五分钟开始、时间模型、点线面、快慢记忆、模式分离/补全、Packet、数据/安全/权限/存储、三种部署、六种 SDK、Hermes/OpenClaw、训练/ONNX、Benchmark、恢复、迁移、升级、威胁模型和性能调优。**
- [ ] **步骤 2：完成 ADR：Rust、SQLite、LLM 独立、原始事件不可变、预测非事实、快慢系统、局部传播、派生可重建。**
- [ ] **步骤 3：运行七个最终验收场景、协议一致性、SDK 契约、Adapter Fixture、Benchmark Smoke 和当前主机 Release Verify。**
- [ ] **步骤 4：运行最终门禁**

```bash
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo doc --workspace --no-deps
scripts/test/sdk-contracts.sh
scripts/benchmark/smoke.sh
git diff --check
```

- [ ] **步骤 5：`verification-report.md` 逐项记录真实命令/退出码/产物/平台、仅 CI 配置项、外部未验证项、风险、安全/一致性问题和占位符扫描；提交 `docs: complete Jiangcheng Memory release verification`**。

## M6 完成定义

- Python 训练可重复，ONNX 与 PyTorch 输出一致；本地运行不依赖 Python；
- 六套 Benchmark 均有真实 Harness 和 Smoke；百万长测可恢复；
- 19 个命名指标输出有已知输入测试和可审计 JSON；
- CI 覆盖 Rust、SDK、Proto、安全、移动端和 Benchmark；
- Release 仅手动私有发布并带 Checksum/SBOM；
- 全部要求文档和 ADR 完成；
- 七个验收场景与当前主机全门禁真实通过；
- 最终报告没有把 CI 配置或 Fixture 冒充实机验证。
