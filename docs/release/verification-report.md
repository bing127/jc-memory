# M6 最终验证报告

## 报告范围

本报告记录 M6 模型、Benchmark、跨平台 CI、SDK/Adapter 契约和私有 Release 的本机验证结果。报告只把实际执行的命令标记为通过；GitHub Actions 矩阵、版本化 Fixture 和未安装工具不被描述为外部实机结果。

验证分支为 `feat/m6-models`，提交为 `8a5bfbd`；工作树中已有的用户 `.superpowers/` 目录未纳入提交。

## 验证环境

| 项目 | 实际值 |
| --- | --- |
| 主机 | macOS ARM64 |
| Rust | `rustc 1.97.1` |
| Python | 训练 Fixture 使用 `models/.venv`；SDK 使用仓库 `.venv` |
| 发布 License | `PRIVATE` |
| 发布方式 | 手动 GitHub Actions Artifact，不调用公共注册表或公开 Release |

## 通过的命令

| 命令 | 结果 | 产物或覆盖范围 |
| --- | --- | --- |
| `bash -n scripts/release/*.sh` | 退出码 0 | Release 脚本语法 |
| `python3 -m unittest scripts/release/tests/test_release_scripts.py scripts/ci/tests/test_workflows.py` | 10 项通过 | 版本、dirty、Artifact、Checksum、工作流和帮助文案契约 |
| `scripts/ci/validate-workflows.sh` | 退出码 0 | 7 个 GitHub Actions 工作流、YAML 和完整 Action SHA |
| `go run github.com/rhysd/actionlint/cmd/actionlint@v1.7.7 .github/workflows/*.yml` | 退出码 0 | Action 表达式和工作流结构 |
| `cargo fmt --all -- --check` | 退出码 0 | Rust 格式 |
| `CARGO_INCREMENTAL=0 cargo clippy --workspace --all-targets --all-features --offline -- -D warnings` | 退出码 0 | Rust 严格 Clippy |
| `RUSTDOCFLAGS='-D warnings' CARGO_INCREMENTAL=0 cargo doc --workspace --all-features --no-deps --offline` | 退出码 0 | Rust 文档和 doctest 编译 |
| `CARGO_INCREMENTAL=0 cargo test --workspace --exclude jc-memory-python --all-features --offline` | 退出码 0 | 其余 workspace 测试与 doctest |
| `CARGO_INCREMENTAL=0 cargo test -p jc-memory-python --no-default-features --offline` | 退出码 0 | Python 绑定 crate 的 Rust 测试，不构建不可链接的 `cdylib` 测试目标 |
| `models/.venv/bin/pytest models/tests -q` | 6 项通过 | 训练/导出/ONNX Fixture |
| `scripts/test/sdk-contracts.sh` | 退出码 0 | Rust/Proto、Server 健康、Node/Python/Go/Kotlin/Swift/Java 和五个 Adapter Fixture |
| `scripts/benchmark/smoke.sh --output target/benchmarks/m6-task10-smoke.json` | 退出码 0 | Harness、million-fragments 恢复测试和审计 JSON |
| 本地 `scripts/release/package.sh` + `scripts/release/verify.sh` Fixture | 退出码 0 | Manifest、PRIVATE、SBOM、元数据、SHA-256 和签名接口 |
| `git diff --check` | 退出码 0 | 空白和补丁格式 |

## Recall 性能与可观测增量验收

增量验证分支为 `feat/performance-observability`。提交 `b5b14b3` 消除最近候选对 FTS 虚表的联接，提交 `384ee96` 使压测器能够通过环境变量访问受保护 BFF，且不把 Cookie、Origin 或 API Key 写入报告。

TDD 证据：`recent_candidates_read_authoritative_content_without_fts_dependency` 在修复前因返回 0 条而失败，修复后与既有最近候选测试共 2 项通过；BFF Header 测试在修复前捕获到 `cookie/origin` 均为 `undefined`，修复后 HTTP 压测执行器 4 项测试通过。

正式 `target/benchmarks/recall-after.json` 使用提交 `384ee96`、干净工作树和真实 BFF -> Rust -> SQLite 链路。每个场景预热 20 次，数据库规模为 2032 Fragment/2032 Event，结果如下：

| 场景 | 请求/并发 | 成功 | 429/timeout | 吞吐 | P50/P95/P99 |
| --- | ---: | ---: | ---: | ---: | --- |
| Recall c8 | 400/8 | 400 | 0/0 | 125.94/s | 61.58/85.59/113.54ms |
| Recall c32 | 800/32 | 800 | 0/0 | 123.40/s | 252.58/305.02/313.25ms |
| Recall c64 | 800/64 | 800 | 0/0 | 118.52/s | 523.56/599.77/604.42ms |

压测后 Fragment/Event 仍为 2032/2032。历史基线与当前报告的数据库规模、服务限额、请求数和并发数相同，但旧请求体没有版本化保存，因此文档中的吞吐与延迟百分比只作为参考对比，不作为严格同请求体 A/B 证明。

## 私有 Release

实现提交 `e41d372` 增加 `.github/workflows/release.yml` 和 `scripts/release/`。workflow 通过 `workflow_dispatch` 接收 `version`、`ref` 和 `prerelease`，要求 `ref` 是对应 `v<version>` tag，并在 Native job 固定 `RUSTUP_TOOLCHAIN=1.97.1`。

Native 目标矩阵包含 Linux x64/ARM64、macOS x64/ARM64 和 Windows x64；SDK job 构建 Node、Python Wheel、Go、Kotlin、Java，并可选 Swift；Mobile job 单独生成 Android 与 Swift 未签名输入。每个私有 Release 生成 `release-manifest.json`、`SHA256SUMS`、CycloneDX SBOM、构建元数据和 `detached-signature-v1` 接口。验证器强制 `PRIVATE`、`public_publish: false`、Artifact 全量 checksum 和路径不越界。

当前主机已完成 macOS ARM64 Native 构建和本地私有 Fixture verify；其他矩阵目标只完成 CI 配置，不能视为本机实测。

## 未通过或未完成边界

1. `CARGO_INCREMENTAL=0 cargo test --workspace --all-features --offline` 在当前 Python 3.14 环境链接 `jc-memory-python` 时失败，缺少 `_Py_*` Python C API 符号。该失败发生在 Python 扩展链接阶段；排除该扩展后的 workspace 测试通过。本机未修改 Rust Python 绑定来掩盖环境问题。
2. SDK Python Fixture 有 1 项明确跳过，原因是需要真实嵌入式 native 模块；远程 HTTP Fixture 已通过。
3. Linux、Windows、macOS x64、Android/iOS 设备或模拟器、Docker/Kubernetes 集群、KMS/HSM、外部 LLM、GPU/加速后端、真实 LongMemEval/LoCoMo 全量数据和生产性能未在本次主机验证。

## 安全与一致性检查

- 原始 Fragment、Event、EvidenceRef 仍是权威记录；摘要、Embedding、图关系、Surface 和索引不覆盖原始记录。
- Prediction、Inference、Plan 和 ToolResult 不会自动成为 Verified Fact；Quarantine、ACL、Scope 和 EvidenceRef 失败时保持 fail-closed。
- Release 默认拒绝 dirty tree；Manifest、SBOM、构建元数据和 checksum 可追溯到提交、版本和目标平台。
- 工作流第三方 Action 使用完整 commit SHA；发布工作流没有 crates.io、npm、PyPI 或公开 Release 写入权限。

## 占位符扫描

扫描命令：

```bash
rg -n 'TODO|FIXME|unimplemented!|todo!' crates services adapters benchmarks models sdk scripts
```

扫描命中两个生成 binding 内的英文 TODO 注释：`sdk/swift/Sources/JiangchengMemoryUniffi/jc_memory_uniffi.swift:552` 和 `sdk/java/src/main/kotlin/uniffi/jc_memory_uniffi/jc_memory_uniffi.kt:1232`。它们属于生成代码的可见性/日志提示，不是业务空函数或伪造实现；未发现 Rust 核心、服务、Adapter、模型或 Benchmark 的实现占位符。文档中的“未实现/未验证”仅用于明确范围，不代表伪造完成。`unsafe` 仅保留在 FFI/第三方绑定边界，并不进入核心逻辑。

## 结论

M6 的可重复模型/ONNX Fixture、Benchmark Smoke、跨平台 CI 配置、SDK/Adapter 契约和私有 Release 资产链已完成本机静态与 Fixture 验证。跨平台实机、外部服务、真实数据集和 Python 扩展链接环境仍是明确的后续验收项，不能将本报告解释为全平台生产发布证明。
