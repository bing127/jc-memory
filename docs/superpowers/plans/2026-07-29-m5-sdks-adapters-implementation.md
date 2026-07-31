# 疆诚记忆 M5 多语言 SDK 与适配器实施计划

> **面向执行代理：** 必须使用 `superpowers:subagent-driven-development`（推荐）或 `superpowers:executing-plans` 逐 Task 执行本计划；所有步骤使用复选框跟踪。

**目标：** 让 Node、Python、Go、Kotlin、Swift、Java 通过统一协议使用疆诚记忆，并完成 Hermes、OpenClaw、Mem0、Graphiti、Dify 的来源保真适配与契约测试。

**架构：** Protobuf/OpenAPI 和 canonical JSON Fixture 是跨语言真相源；远程 SDK 必须先完成，原生绑定复用同一 Core Command。Adapter 只负责来源数据映射和生命周期桥接，不得提升来源置信度或丢失 Provenance。

**技术栈：** napi-rs、TypeScript、PyO3/maturin、Go gRPC/C ABI、UniFFI/JNI、Kotlin Coroutines、Swift async/await、Java/Gradle、Buf、JSON Schema。

---

## 文件边界

- `proto/jiangcheng/memory/v1/`、`sdk/contracts/fixtures/`、`scripts/test/sdk-contracts.sh`：统一契约。
- `crates/memory-ffi/`：受控 C ABI；`crates/memory-uniffi/`：Kotlin/Swift 共享绑定。
- `sdk/node/`、`sdk/python/`、`sdk/go/`、`sdk/kotlin/`、`sdk/swift/`、`sdk/java/`：语言 SDK。
- `adapters/{hermes,openclaw,mem0,graphiti,dify}/`：独立版本化 Adapter。
- `examples/{node-electron,python-agent,go-agent}/`：真实调用示例。

## Task 1：冻结 SDK 契约 Fixture 与兼容门禁

**文件：** `sdk/contracts/{manifest.json,fixtures/*.json}`、`proto/buf.yaml`、`proto/buf.lock`、`scripts/test/sdk-contracts.sh`。

- [x] **步骤 1：写 Fixture Validator 失败测试**，覆盖 Observe、十种 Recall、Commit、Correct、Explain、Forget、Stats 和稳定错误；同时覆盖篡改 SHA-256 的拒绝路径。
- [x] **步骤 2：运行 `cargo test -p jc-memory-protocol --test sdk_contract_fixtures`**，先确认缺少 Validator 的红灯，再完成绿灯。
- [x] **步骤 3：每个 Fixture 固定 request、response、canonical_sha256、protocol_version、required_features；敏感字段使用合成值，不提交真实 Token。**
- [x] **步骤 4：加入 `buf lint`、`buf breaking --against` 和 SDK 契约脚本；当前仓库尚无 OpenAPI schema，保留为后续远程 SDK 门禁。生成代码未改动且 `git diff --check` 通过。**
- [x] **步骤 5：提交 `test(sdk): add canonical cross-language contracts`。已完成提交 `8dbcd05 test(sdk): 增加跨语言规范契约`；Rust、Buf 兼容性、Clippy、格式和差异检查均已通过。**

## Task 2：实现 Node/TypeScript SDK 与 napi-rs

**文件：** `sdk/node/{package.json,tsconfig.json,npm/*.json,src/{index,types,remote,embedded,errors}.ts,__tests__/*.test.ts}`、`crates/memory-node/`、`examples/node-electron/`。

- [x] **步骤 1：写 Vitest 失败测试。覆盖 canonical Recall、Embedded 生命周期、HTTP 错误、AbortSignal 和 napi JSON 适配。**

```ts
it("returns the canonical recall packet", async () => {
  const client = new RemoteMemoryClient({ baseUrl: server.url, apiKey: "fixture" });
  expect(await client.recall(fixture.query)).toEqual(fixture.response);
});
```

- [x] **步骤 2：运行 `corepack pnpm --dir sdk/node test`，先确认导出缺失红灯，再完成绿灯。**
- [x] **步骤 3：实现 `RemoteMemoryClient` 的 observe/recall/commit/correct/explain/forget/stats/close；使用 fetch + AbortSignal，错误统一映射为 `JiangchengMemoryError { code, requestId, details }`。当前 REST 服务没有 Node 生成 gRPC Client，因此未伪造 gRPC 实现。**
- [x] **步骤 4：加入 `crates/memory-node` napi-rs Embedded 边界，使用 napi Tokio runtime，只暴露 JSON Rust Core 门面；`EmbeddedMemoryClient` 复用同一方法集合，Electron 示例调用远程 Recall 并执行退出清理；配置私有 npm 平台包但不发布。ThreadsafeFunction 未作为额外 API 暴露，避免引入未使用的回调通道。**
- [x] **步骤 5：运行 TypeScript 类型检查、Vitest、`cargo build -p jc-memory-node`、Rust 单测/Clippy、格式和差异检查；提交 `feat(sdk-node): 增加远程与嵌入式客户端`。**

## Task 3：实现 Python SDK 与 PyO3

**文件：** `sdk/python/{pyproject.toml,src/jiangcheng_memory/{__init__,models,client,async_client,errors}.py,tests/*.py}`、`crates/memory-python/`、`examples/python-agent/`。

- [x] **步骤 1：写 pytest 同步/异步/取消/错误失败测试，并补充 Embedded 命令方法和上下文管理器失败测试。**
- [x] **步骤 2：运行 `python -m pytest sdk/python/tests -q`；宿主最初缺少 pytest，安装隔离 `.venv` 依赖后确认模块缺失红灯。**
- [x] **步骤 3：加入 typed dict JSON 模型；同步/异步 HTTP Client 使用 `httpx`，同步客户端不嵌套事件循环，Async Client 原生 await 并保留 `CancelledError`。当前 REST 服务未提供 Python 生成 gRPC Stub，因此未伪造 gRPC 实现。**
- [x] **步骤 4：`crates/memory-python` 使用 `pyo3-async-runtimes` Tokio Future 调用 Embedded；`extension-module` 仅作为 maturin feature，普通 Rust 测试不链接 Python；pyproject 声明 maturin wheel 构建，运行时不依赖训练包。**
- [x] **步骤 5：运行 pytest（6 passed）、mypy、Rust 单测/Clippy、当前 Python 3.14 的 maturin develop 和真实 `_native` Embedded Smoke；提交 `feat(sdk-python): 增加同步异步与嵌入式客户端`。**

## Task 4：实现 Go SDK 与受控 C ABI

**文件：** `sdk/go/{go.mod,memory/client.go,memory/types.go,memory/errors.go,memory/client_test.go}`、`crates/memory-ffi/{src/lib.rs,include/jcmemory.h}`、`examples/go-agent/`。

- [x] **步骤 1：写 `go test` 的 Context 取消、Fixture 和错误码失败测试。**
- [x] **步骤 2：运行嵌套 module 的 `go test ./...`，先确认 `memory` 包缺失红灯，再完成绿灯。**
- [x] **步骤 3：实现标准库 HTTP Transport；所有远程方法接收 `context.Context`，Envelope 使用 `json.RawMessage` 保留 optional 区别，错误提供 `Code(err)` 和 `RequestID(err)`。当前服务未提供 Go 生成 gRPC Stub，因此未伪造 gRPC 实现。**
- [x] **步骤 4：`memory-ffi` 仅暴露 create/destroy/execute/cancel/free_buffer 及带 token 的执行变体；generation 绑定取消 token，所有 panic 在边界捕获，boxed slice 保证返回缓冲区容量不依赖隐藏值；`unsafe` 仅存在该 crate 并逐块说明指针、长度、所有权和生命周期不变量。**
- [x] **步骤 5：运行 `cargo test/clippy -p jc-memory-ffi`、Go 普通和 race test、示例编译、格式和差异检查；提交 `feat(sdk-go): 增加远程客户端与受控 C ABI`。**

## Task 5：实现 Kotlin/Android SDK

**文件：** `crates/memory-uniffi/`、`sdk/kotlin/{settings.gradle.kts,build.gradle.kts,src/main,src/test,src/androidTest}`。

- [x] **步骤 1：写 JVM Fixture、Coroutine 取消和稳定错误映射失败测试。**
- [x] **步骤 2：运行 `gradle -p sdk/kotlin test`，先确认 API 缺失红灯，再完成绿灯。**
- [x] **步骤 3：实现 Kotlin/JVM 远程 `JiangchengMemoryClient`，方法使用 `suspend`，`recallFlow` 使用 `Flow`，网络调用运行在 `Dispatchers.IO`，客户端实现 `AutoCloseable`。**
- [ ] **步骤 4：Android AAR、ABI filters、consumer ProGuard 和 UniFFI `JcMemoryEngine` 尚未实现；当前仓库没有 `memory-uniffi` crate，不能把 JVM 客户端标记为 Android Embedded。**
- [x] **步骤 5：运行 JVM Gradle 测试（4 个通过）；当前主机未运行 connectedAndroidTest，已在 `docs/sdk/kotlin.md` 记录为 CI/设备待验证；提交 `feat(sdk-kotlin): 增加 JVM Coroutine 客户端`。**

## Task 6：实现 Swift/iOS/macOS SDK

**文件：** `sdk/swift/{Package.swift,Sources/JiangchengMemory/*.swift,Tests/*.swift}`、`scripts/build/build-xcframework.sh`，复用 `memory-uniffi`。

- [x] **步骤 1：写 Swift Fixture、async cancellation、Actor 并发和错误映射失败测试。**
- [x] **步骤 2：运行 `swift test --package-path sdk/swift`，先确认 target 缺失红灯，再完成绿灯。**
- [x] **步骤 3：`JiangchengMemoryClient` 使用 actor 保护 URLSession 状态，公开 async observe/recall/commit/correct/explain/forget/stats；调用方取消保留 Swift `CancellationError`。**
- [ ] **步骤 4：Package.swift 已声明 macOS/iOS 最低版本并加入显式输入的 XCFramework 脚本；但 `memory-uniffi`、device/simulator/macOS 原生 slices 和 dSYM 合并尚未实现，不能标记为完成。**
- [x] **步骤 5：运行当前主机 Swift 测试（3 个通过），提交 `feat(sdk-swift): 增加异步 Apple 客户端`；Apple 原生绑定留待 UniFFI/CI。**

## Task 7：实现 Java/JVM SDK

**文件：** `sdk/java/{settings.gradle.kts,build.gradle.kts,src/main/java,src/test/java}`。

- [x] **步骤 1：写 CompletableFuture、HTTP `Duration` deadline、Fixture、取消和错误码失败测试。当前服务未提供 Java gRPC Stub，因此不伪造 gRPC 实现。**
- [x] **步骤 2：运行 `gradle -p sdk/java test --no-daemon`，先确认超时异常导入和 JUnit Launcher 配置缺失的红灯，再完成绿灯。**
- [x] **步骤 3：实现 `JiangchengMemoryClient` AutoCloseable，异步方法返回 CompletableFuture，`*Blocking` 同步 facade 明确阻塞；使用不可变 `MemoryResponse` wrapper 保留原始 JSON。**
- [x] **步骤 4：嵌入模式复用 UniFFI 0.32 生成的 Kotlin/JVM native loader，不复制第二套 JNI；当前主机已校验平台库 SHA-256、classloader/path 缓存和 Rust handle 生命周期。Android AAR、移动端 native slices 与其他平台 CI 仍待完成。**
- [x] **步骤 5：运行 Gradle 测试并提交 `feat(sdk-java): 增加 JVM CompletableFuture 客户端`；UniFFI/JVM Embedded 继续留待后续绑定任务。**

## Task 8：实现 Hermes Adapter

**文件：** `adapters/hermes/{Cargo.toml,src/{lib,config,mapping}.rs,tests/fixtures/*.json,README.md}`。

- [x] **步骤 1：写 sync_turn、prefetch、shutdown 和配置 Fixture 失败测试。已覆盖来源保真、模型候选不升级、Recall Packet 透传、关闭期间拒绝新请求和 in-flight 等待。**
- [x] **步骤 2：运行 `cargo test -p jc-memory-adapter-hermes`，先确认 workspace 中 crate 缺失，再确认公开契约缺失的编译红灯。**
- [x] **步骤 3：实现 MemoryProvider；sync_turn 通过 Core Commit 映射 Observation/ModelOutput/ToolResult，prefetch 原样返回 MemoryPacket，shutdown 等待 in-flight 后调用 CommandBus flush；source_id 和 metadata 保存 Hermes 版本、observer 接口版本与 turn ID。**
- [x] **步骤 4：Fixture 仅声明 Hermes 0.19.0 / `hermes.observer.v1` 契约；测试已使用真实 EmbeddedCommandBus 验证 Core 边界，但未把 Fixture 描述为 Hermes Python 运行时实机验证。**
- [x] **步骤 5：已提交 `2ce819b feat(adapter-hermes): 增加来源保真的记忆适配器`；聚焦测试 6 个通过，fmt、严格 Clippy、rustdoc 和 `git diff --check` 均通过。**

## Task 9：实现 OpenClaw 与 Mem0 Adapter

**文件：** `adapters/openclaw/`、`adapters/mem0/` 的 config/mapping/import/export/tests/README。

- [ ] **步骤 1：写 Markdown/Memory 导入、Hook/MCP、Mem0 Add/Search、历史迁移和 round-trip 失败测试。**
- [ ] **步骤 2：运行两个 Adapter 测试**，预期 crate 缺失。
- [ ] **步骤 3：OpenClaw 按文件路径、内容哈希、heading 和行范围生成 Provenance；Hook/MCP 只调用 Core API；重复导入幂等但不覆盖历史。**
- [ ] **步骤 4：Mem0 Add 映射 Fragment，Search 映射 Recall；旧数据默认 Unverified 并保存 legacy_confidence；Export 保留 ID mapping manifest。**
- [ ] **步骤 5：运行契约测试并提交 `feat(adapters): add OpenClaw and Mem0 migration bridges`**。

## Task 10：实现 Graphiti 与 Dify Adapter

**文件：** `adapters/graphiti/`、`adapters/dify/` 的协议、mapping、Fixture、README；`examples/mcp-client/`。

- [ ] **步骤 1：写 Graphiti temporal edge、推断边、Dify Scope 和工作流节点失败测试。**
- [ ] **步骤 2：运行测试**，预期 Adapter 缺失。
- [ ] **步骤 3：Graphiti Node -> Point/Entity，Edge -> Candidate Edge，valid_at/invalid_at -> 有效时间；Provenance 和 source version 必须保留，inferred edge 不设置 human/tool verified。**
- [ ] **步骤 4：Dify user/conversation/app/workflow 映射 User/Session/Project/Trace Scope；HTTP/MCP 节点输出结构化 Memory Packet；示例不内置 API Key。**
- [ ] **步骤 5：运行测试并提交 `feat(adapters): add Graphiti and Dify integrations`**。

## Task 11：跨语言契约、文档与门禁

**文件：** `scripts/test/sdk-contracts.sh`、`docs/sdk/*.md`、`docs/deployment/embedded-bindings.md`、各示例、CHANGELOG。

- [ ] **步骤 1：脚本启动一次本地 Server，依次运行 Node、Python、Go、Kotlin JVM、Swift、Java 的同一 Fixture；比较 canonical JSON SHA-256。**
- [ ] **步骤 2：运行各 Adapter Fixture 与 Import/Export round-trip；验证所有来源都能回溯原始外部 ID。**
- [ ] **步骤 3：编写六种 SDK 和五种 Adapter 中文文档、支持矩阵、构建前提和未验证平台。**
- [ ] **步骤 4：运行 Rust 门禁、可用 SDK 测试、`git diff --check` 和生成代码无漂移检查。**
- [ ] **步骤 5：提交 `docs: complete M5 SDK and adapter milestone`**。

## M5 完成定义

- 六种 SDK 的远程模式通过同一契约；
- 当前主机可构建的原生模式通过真实测试；
- C ABI/JNI/UniFFI 的 unsafe 被隔离并有边界测试；
- 五种 Adapter 保留 Provenance、Scope、时间和原置信度；
- Fixture 兼容与实机验证在报告中明确区分；
- 示例可运行且不含密钥；
- 全仓和各 SDK 门禁通过，无占位实现。
