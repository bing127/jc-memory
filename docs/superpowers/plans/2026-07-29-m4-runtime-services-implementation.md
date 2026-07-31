# 疆诚记忆 M4 Agent Runtime 与服务实施计划

> **面向执行代理：** 必须使用 `superpowers:subagent-driven-development`（推荐）或 `superpowers:executing-plans` 逐 Task 执行本计划；所有步骤使用复选框跟踪。

**目标：** 实现可暂停恢复的 Agent 循环、模型/工具/能力/策略/审批系统，并让同一核心语义经 Embedded、REST、gRPC、Sidecar 和 MCP 对外提供。

**架构：** Runtime 是持久化显式状态机，模型输出先解析为候选，Policy/Approval 决定工具执行，实际 ToolResult 经过验证后才 Commit。所有服务把请求转换为同一个 Core CommandBus，协议层不复制业务逻辑。

**技术栈：** Rust 2024、Tokio、Axum、Tonic/Prost、Utoipa OpenAPI、reqwest、tower、MCP JSON-RPC、tracing/opentelemetry、Prometheus。

---

## 文件边界

- `proto/jiangcheng/memory/v1/memory.proto`：稳定 gRPC/SDK 契约。
- `crates/memory-protocol/src/{command,runtime,capability,service}.rs`：共享命令与状态。
- `crates/memory-runtime/src/{state,runner,model,parser,capability,policy,approval,tool,mcp_client,completion,recovery}.rs`。
- `services/memory-server/src/{main,config,http,grpc,auth,limits,telemetry,shutdown}.rs`。
- `services/memory-sidecar/src/{main,config,transport,lock,token,lifecycle}.rs` 与 `src/bin/{jcmem,jcmemd}.rs`。
- `services/memory-mcp/src/{main,server,tools,schema}.rs`。
- `crates/memory-core/src/command_bus.rs`：唯一业务命令入口。

## Task 1：建立 Protobuf 与统一 CommandBus

**文件：** 创建 Proto、`memory-protocol/src/command.rs`、`memory-core/src/command_bus.rs`、契约测试。

- [x] **步骤 1：写 Embedded Command 失败测试**，同一 ObserveCommand 经 `MemoryEngine` 和 `CommandBus` 返回相同 canonical JSON。
- [x] **步骤 2：运行测试**，预期 CommandBus 缺失。
- [x] **步骤 3：实现：**

```rust
pub struct MemoryRequest { pub api_version: String, pub request_id: String, pub access_scope: AccessScope, pub command: MemoryCommand }
pub enum MemoryCommand { Observe(ObserveCommand) }
pub enum MemoryResponse { Observe(ObserveResult) }
#[async_trait]
pub trait CommandBus: Send + Sync { async fn execute(&self, access: &AccessContext, request: MemoryRequest) -> Result<ResponseEnvelope, CommandError>; }
```

- [x] **步骤 4：Proto 每个请求包含 AccessScope、request_id 和版本，Observe 使用完整强类型且无损的 CognitiveFragment；运行 Buf/Prost 编译与 JSON/Proto 往返测试。**
- [x] **步骤 5：提交 `feat(protocol): add unified service command contract`**。

## Task 2：实现 Runtime 持久化状态机

**文件：** `memory-runtime/src/{state,runner,recovery}.rs`、`tests/state_machine.rs`。

- [x] **步骤 1：写非法跳转和重开恢复失败测试**

```rust
#[test]
fn executing_cannot_skip_commit_to_completed() {
    assert_eq!(RuntimeState::Executing.transition(RuntimeEvent::Complete), Err(TransitionError::Invalid));
}
```

- [x] **步骤 2：运行测试**，预期 RuntimeState 缺失。
- [x] **步骤 3：实现 Observing、Recalling、Reasoning、AwaitingApproval、Executing、Committing、WaitingUser、Paused、Completed、Failed、Cancelled；转换表使用穷尽 match。**
- [x] **步骤 4：每次转换在 `transitions` 表保存 run_id、step、from/to、输入输出引用、attempt、deadline、trace；重开从最后已提交状态恢复。**
- [x] **步骤 5：运行测试并提交 `feat(runtime): add recoverable agent state machine`**。

## Task 3：实现 ModelProvider 与输出解析

**文件：** `memory-runtime/src/{model,providers/openai_compatible,providers/http,parser}.rs`、`tests/model_contract.rs`。

- [x] **步骤 1：写确定性 Provider、超时、SSE 分片和非法输出失败测试。**
- [x] **步骤 2：运行测试**，预期 ModelProvider 缺失。
- [x] **步骤 3：实现：**

```rust
#[async_trait]
pub trait ModelProvider: Send + Sync { async fn invoke(&self, request: ModelRequest, cancel: CancellationToken) -> Result<ModelResponse, ModelError>; fn descriptor(&self) -> ModelDescriptor; }
pub enum ModelOutput { Hypothesis(Hypothesis), Prediction(Prediction), Plan(Plan), DecisionCandidate(DecisionCandidate), ToolRequest(ToolRequest), FinalResponse(FinalResponse) }
```

- [x] **步骤 4：OpenAICompatibleProvider 支持非流式和 SSE、Bearer/Header 配置、超时、429/5xx 有界重试；GenericHttpProvider 使用配置化 JSON Pointer 映射；Parser 拒绝未知种类和 Schema 不匹配。**
- [x] **步骤 5：运行测试并提交 `feat(runtime): add replaceable model providers`**。

## Task 4：实现 Capability Registry

**文件：** `memory-runtime/src/capability/{model,registry,stats}.rs`、`tests/capabilities.rs`。

- [x] **步骤 1：写版本解析、权限、风险和成功率失败测试。**
- [x] **步骤 2：运行测试**，预期 CapabilityRegistry 缺失。
- [x] **步骤 3：实现：**

```rust
pub enum CapabilityKind { Prompt, Skill, Tool, McpTool, Workflow, Policy }
pub struct CapabilityDescriptor { pub id: String, pub version: Version, pub kind: CapabilityKind, pub source: String, pub permissions: BTreeSet<Permission>, pub input_schema: Value, pub output_schema: Value, pub risk: RiskLevel, pub requires_approval: bool, pub surfaces: Vec<SurfaceId> }
pub trait CapabilityRegistry { fn register(&self, descriptor: CapabilityDescriptor, executor: Arc<dyn CapabilityExecutor>) -> Result<(), RegistryError>; fn resolve(&self, request: &CapabilityRequest, access: &AccessContext) -> Result<ResolvedCapability, RegistryError>; }
```

- [x] **步骤 4：统计只保存 runs/successes/failures/failure_modes/evidence；排序使用有最小样本数的贝叶斯平滑，不生成 Fact。**
- [x] **步骤 5：运行测试并提交 `feat(runtime): add versioned capability registry`**。

## Task 5：实现 Policy、Approval、Tool 与 Commit 闭环

**文件：** `memory-runtime/src/{policy,approval,tool,runner}.rs`、`tests/tool_loop.rs`。

- [x] **步骤 1：写高风险工具需要审批、拒绝不执行、失败结果否定 Prediction 的失败测试。**
- [x] **步骤 2：运行测试**，预期 PolicyEngine/ToolRegistry 缺失。
- [x] **步骤 3：实现 `PolicyDecision::{Allow,Deny,RequireApproval}`、持久化 Approval、ToolDescriptor 的 JSON Schema/权限/风险/idempotency、ToolExecutor CancellationToken。**
- [x] **步骤 4：Runner 顺序固定为 Observe -> Recall -> Model -> Parse -> Policy -> Approval -> Execute -> Capture -> Commit；Commit 写 Prediction、Action、ToolResult、Outcome 和 ValidatedBy/DisprovedBy，不把 Prediction 改成 Fact。**
- [x] **步骤 5：运行测试并提交 `feat(runtime): execute approved tools and commit outcomes`**。

## Task 6：实现 WaitingUser、Pause/Resume、Retry、MCP Client 与 Completion

**文件：** `memory-runtime/src/{mcp_client,completion,recovery}.rs`、`tests/resume.rs`。

- [x] **步骤 1：写 WaitingUser 恢复、取消、重试上限、非幂等工具不重放、MCP 超时失败测试。**
- [x] **步骤 2：运行测试**，预期控制 API 缺失。
- [x] **步骤 3：实现 RunControl 的 pause/resume/cancel/provide_user_input；RetryPolicy 按错误分类且持久化 next_attempt_at。**
- [x] **步骤 4：MCP Client 支持 initialize、tools/list、tools/call、协议版本和超时；CompletionJudge 同时检查目标、待审批、待工具、缺失证据和最大步数。**
- [x] **步骤 5：运行测试并提交 `feat(runtime): add pause resume MCP and completion control`**。


## Task 7：实现 HTTP/gRPC Server

**文件：** `services/memory-server/` 全部模块、`tests/protocol_parity.rs`。

- [x] **步骤 1：写 Health、Observe、Recall、错误码、API Key、限流和优雅关闭失败测试。**
- [x] **步骤 2：运行测试并确认 Server crate 与共享 CommandBus 已接通。**
- [x] **步骤 3：实现 Axum 路由 `/v1/memory/{observe,commit,recall,correct,forget,explain,stats}` 和 `/healthz`、`/readyz`、`/metrics`；Extractor 构造 AccessContext，禁止客户端伪造授权 Grant。**
- [x] **步骤 4：Tonic 实现同一 CommandBus；REST/gRPC 共享错误分类和 request_id；TLS 从文件加载且不记录密钥；HTTP/gRPC 统一请求超时、并发和按 API Key 速率限制。**
- [x] **步骤 5：运行协议测试、TCP gRPC 测试和全仓门禁；按命令能力独立提交并推送。**

> 当前已落地 Server 基础边界：`/healthz`、`/readyz`、Observe/Recall/Stats/Correct/Forget/Explain REST 路由、API Key 到服务端 `AccessContext` 的映射，以及共享 `CommandBus` 的适配器。真实 Tonic `Execute` 网络门面已接入；gRPC 文件型 TLS 已接入 `JC_MEMORY_TLS_CERT_FILE` 与 `JC_MEMORY_TLS_KEY_FILE`，并要求证书和私钥成对配置。未配置 TLS 时仍保留本机明文开发模式。当前七类 Memory 命令、请求超时、并发门禁和速率限制已有协议实现；KeyProvider 生产实现和 M4 总体验收仍待完成。

### 2026-07-31 进度记录

- 已增加真实 `memory-server` 二进制入口：从环境变量读取监听地址、数据目录、API Key、Namespace 和并发上限。
- 已将 `MemoryEngine`、`EmbeddedCommandBus`、精确 `Observe` Scope Grant 和 HTTP Listener 接通。
- 已接入 Ctrl-C 优雅关闭，并完成本机临时目录启动探针：`/healthz` 返回成功，进程收到 `SIGINT` 后以 0 退出。
- 已增加真实 Server 二进制重启 E2E：启动后经 HTTP Observe 写入权威记录，`SIGINT` 干净退出，以同一数据目录重启后 Stats 返回 1 个 Fragment 和 1 个 Event。
- 已补齐共享 `RecallCommand`、硬预算和 `MemoryPacket` 响应，并接入 `/v1/memory/recall`；REST 路由与共享 `CommandBus` Envelope 契约测试通过。
- 已补齐 `StatsCommand`、Scope 统计响应并接入 `/v1/memory/stats`；统计只读取授权 Scope 的权威 Fragment/Event 数量。
- 已将服务端 API Key 授权集中到精确 Scope 构造器，入口现在授予已公开的 `Observe`、`Recall` 与 `Mutate` 操作；真实二进制 `/v1/memory/stats` 探针通过 Core ACL 并返回零计数。
- 已补齐 `MutationCommand`、`Correct`/`Forget` 动作和追加式 `MutationResult`，接入 `/v1/memory/correct` 与 `/v1/memory/forget`；Embedded CommandBus 临时 SQLite 闭环验证通过，入口同时授予 `Mutate`。
- 已补齐结构化 `ExplainResult`（种子、遍历、Surface、验证 Fragment、预算和截断状态），复用 Recall 查询输入并接入 `/v1/memory/explain`；入口授予 `Explain`，Embedded CommandBus 映射测试通过。
- 已补齐 Commit 领域命令、`/v1/memory/commit` REST 路由和 MCP/Sidecar 转发；Commit 只允许同一 Scope 的 Fragment 批次，并按输入顺序返回权威追加结果。
- 已将 Commit 权威 Fragment 批次接入 SQLite 单事务追加；补充晚期写入失败时整批回滚的故障契约测试。
- 已在稳定 Proto 契约中加入 `MemoryService.Execute`，用 Tonic 0.14 生成真实 TCP gRPC Server/Client；`x-api-key` 元数据映射到同一 `GrpcAdapter`，网络契约测试和双端口启动探针通过。
- 已将 Tonic/Prost 依赖升级到 0.14，并切换 `tonic_prost_build` 生成代码；服务端在新依赖下保持可编译。
- 已接入文件型 gRPC TLS：启动入口只读取证书链和私钥文件内容构造 `ServerTlsConfig`，不记录私钥；配置解析使用纯查找器测试，避免修改进程环境。
- 已修正 gRPC 适配器绕过并发门禁的问题；HTTP 与 gRPC 现在共同使用 `max_in_flight` permit，超限统一返回资源耗尽语义。
- 已接入按 API Key 隔离的固定窗口速率限制；`JC_MEMORY_MAX_REQUESTS_PER_SECOND` 默认 128，HTTP 与 gRPC 共享配额，配置为零时拒绝启动。
- 已增加 YAML 配置文件加载；`JC_MEMORY_CONFIG_FILE` 指定文件，环境变量逐字段覆盖 YAML，API Key 仍只允许从环境变量或部署 Secret 注入。
- 已补充 Dockerfile、Docker Compose、Kubernetes ConfigMap/Secret/Deployment/Service/PVC 和中文部署指南；当前仅完成本地 YAML 结构与契约验证，未宣称真实容器或集群验证。
- 已接入可配置请求 deadline；`JC_MEMORY_REQUEST_TIMEOUT_MS` 默认 30000，HTTP 超时返回 504，gRPC 超时返回 `DeadlineExceeded`，并保留请求标识。
- 已完成 Recall、Stats、Correct、Forget、Explain 的强类型 Protobuf 请求/响应扩展，所有领域字段通过编解码往返测试保持无损；真实 TCP gRPC 已覆盖 Recall Execute。
- 已完成按文件加载的 gRPC TLS、YAML/环境变量配置、请求超时、共享并发门禁和按 API Key 速率限制；Docker/Kubernetes 仅完成静态资源与契约验证。
- Task 7 仍不代表 M4 完成：KeyProvider 生产实现、Sidecar 到 MCP 的身份转发链和完整 Embedded/REST/gRPC/Sidecar/MCP 跨协议 E2E 仍待完成。

## Task 8：实现本地 Sidecar

**文件：** `services/memory-sidecar/src/{main,config,transport,lock,token,lifecycle}.rs`、`src/bin/{jcmem,jcmemd}.rs`、集成测试。

- [x] **步骤 1：写第二实例被锁拒绝、Token 错误、崩溃重启和 shutdown 失败测试。** 已覆盖锁拒绝、错误 Token、重启、在途 Execute、未认证空闲连接、畸形连接隔离、shutdown 成功与失败。
- [x] **步骤 2：运行聚焦 Sidecar 契约测试。**
- [x] **步骤 3a：实现 Transport Trait；Unix 使用权限 0600 的 UDS，非 Unix 使用随机 Loopback 端点文件；Named Pipe 只保留 Trait 扩展点，不宣称已实现。`jcmemd` 运行守护进程；`jcmem` 提供 status/stop/doctor/observe/recall/commit/correct/forget/explain/stats 子命令并使用同一协议 Client。**
- [x] **步骤 3b-1：补充 `jcmem start`，并通过本机启动、状态和停止探针。**
- [ ] **步骤 3b-2：补充 Windows 实机传输验证。** 当前环境为 macOS，未宣称 Windows 实机验证。
- [x] **步骤 4a：Token 使用 256-bit 随机值、权限受限文件和常量时间比较；停止流程等待已认证在途请求。**
- [x] **步骤 4b-1：文件锁记录 PID/启动标识，并在 OS 锁可取得但存在旧元数据时标记陈旧锁。**
- [x] **步骤 4b-2：生命周期 checkpoint 识别非干净重启并原子发布；停止时取消未认证空闲连接、等待已认证在途请求、显式关闭 CommandBus/SQLite 并记录干净退出。**
- [x] **步骤 5a：运行 Sidecar 测试、Clippy、rustdoc、fmt 和 `git diff --check`。**
- [ ] **步骤 5b：完成 Sidecar 全部边界后创建阶段总提交。**

### 2026-07-31 进度记录

- 已建立 `jc-memory-sidecar` crate，完成 32 字节随机 Token、Unix `0600` 文件权限、恒时比较和文件排他锁。
- 已完成 Unix Domain Socket 控制行协议、`status`/`stop`/`doctor` 控制命令，以及错误 Token、第二实例、停止和重启契约测试。
- 已增加带共享 `CommandBus` 的 `execute` 控制消息和 Commit 转发契约测试；请求仍由本地 Token、精确 Scope 和 Core ACL 共同保护。
- 已增加 `jcmem observe/commit/recall/correct/forget/explain/stats` 专用子命令；每个子命令接收 `--request-file` 或 `--request-json`，并在发送前校验命令类型与 Envelope。
- 已将 Sidecar 连接改为并发任务处理；`stop` 会停止接收新连接并等待已认证的在途连接完成后再退出，新增阻塞 CommandBus 契约测试。
- 非 Unix Loopback 已通过端点文件保存随机端口并由客户端读取连接；Named Pipe 和 Windows 实机验证尚未完成。因此 Task 8 不标记为完成。
- 已增加 `jcmemd` 守护进程别名和 `jcmem start`；本机临时目录启动、状态、停止探针通过。Windows 实机传输仍待专用环境验证。
- 锁文件现在记录 PID/启动标识，并在旧元数据残留但 OS 锁已释放时报告陈旧恢复。
- 生命周期检查点记录 `running`/`stopped`、PID 和启动时间；非干净重启可被识别，干净停止会在等待在途请求后调用 CommandBus shutdown，Embedded Bus 显式关闭 SQLite 连接池。
- 检查点通过同目录临时文件、文件同步和原子替换发布；Unix 同步父目录，Windows 使用目录句柄刷新目录项。取得实例锁后才读取旧状态。单连接解析错误不会终止守护进程，未认证空闲连接也不会阻塞干净停止；shutdown 失败会保留 `running`。

## Task 9：实现 MCP Server

**文件：** `services/memory-mcp/src/{main,server,tools,schema}.rs`、`tests/mcp_contract.rs`。

- [x] **步骤 1：写 initialize、tools/list 和七个 tools/call 的 Schema/结果失败测试。**
- [x] **步骤 2：运行 MCP 聚焦契约测试。**
- [x] **步骤 3：实现 memory_observe、memory_recall、memory_commit、memory_correct、memory_explain、memory_forget、memory_stats；每个 inputSchema 禁止额外未知字段并要求 Access Scope。**
- [x] **步骤 4：结果使用 MCP structuredContent 包含协议对象；错误返回稳定 code/data，不把自然语言当唯一结果。**
- [x] **步骤 5：运行 MCP 测试、Clippy、rustdoc、fmt 和差异门禁，并按中文提交规范提交。**

### 2026-07-31 进度记录

- 已建立 `jc-memory-mcp` stdio JSON-RPC Server，完成 `initialize`、`tools/list` 和七个稳定工具名的契约测试。
- Observe、Recall、Commit、Correct、Forget、Explain、Stats 通过共享 `CommandBus` 执行并返回 MCP `structuredContent`；Commit 批次要求所有 Fragment 属于请求 Scope，并保留各自原始 Kind。
- 已增加可选 `JC_MEMORY_MCP_TOKEN` 身份校验；配置后 `tools/call` 必须携带 `authToken`，缺失或错误统一返回 `-32001`，并通过固定长度比较避免 Token 提前退出比较。
- 已收紧 `tools/list` 的请求 Schema，并在执行前校验工具名与 `MemoryCommand` 一致，避免通过错误工具路由获得不同操作 Grant。
- MCP 到 Sidecar 的 `ExecuteAs` 主体转发链已有真实临时 SQLite 契约测试；完整部署级跨协议 E2E 仍待实现，因此 Task 9 只表示 MCP 协议边界完成，不表示 M4 完成。

## Task 10：端到端一致性、文档与门禁

**文件：** `tests/e2e/runtime_loop.rs`、`tests/e2e/protocol_parity.rs`、部署与协议文档、README/CHANGELOG。

- [x] **步骤 1：确定性 Model/Tool 完成完整循环，实际失败否定“测试会通过”的 Prediction。** 已增加 Runtime 集成测试，覆盖 ModelProvider、Parser、ToolLoop、DisprovedBy 隔离和 RuntimeRunner 完成状态。
- [x] **步骤 2：同一 Fixture 经 Embedded、REST、gRPC、Sidecar、MCP 调用，canonical Memory Packet 和 Evidence 集合一致。**
- [x] **步骤 3：编写 Runtime、Capability、Server、Sidecar、MCP、故障恢复和配置文档。**
- [x] **步骤 4：运行全仓 Rust 门禁、服务 E2E 和 `git diff --check`。**
- [ ] **步骤 5：提交 `docs: complete M4 runtime and services milestone`**。

### 2026-07-31 进度记录

- 已完成协议包、Server、Sidecar、MCP 和 Workspace 的 fmt、严格 Clippy、测试、rustdoc 与 `git diff --check` 门禁。
- 已有 REST/gRPC Adapter 一致性、TCP gRPC Recall、MCP structuredContent 和 Runtime Fixture 契约测试。
- 已补充真实临时 SQLite Fixture：同一 Fragment 的 Recall 经 Embedded、REST、TCP gRPC、Sidecar 转发 MCP 后，规范化去除每次请求随机的 `query_id` 后，完整 Memory Packet 保持一致。
- 已补充本地真实 Server 二进制启动、Observe、`SIGINT`、同目录重启和 Stats 持久化验收；这不等同于五协议独立部署进程组合 E2E。
- 已增加确定性 Runtime 循环验收：模型返回 ToolRequest，失败工具只执行一次并生成 `DisprovedBy`，不写入 Fact，Runtime 状态从 Observe 路径提交到 Completed。
- 已增加 MCP 到 Sidecar 的真实身份转发：`JC_MEMORY_MCP_SIDECAR_DATA_DIR` 选择 Sidecar 后端，`mcp-client` 通过 `ExecuteAs` 传到 Core；仍未完成五协议同一 Fixture 的部署级 E2E。
- 已补齐 Runtime、Capability、Server、Sidecar、MCP、故障恢复、配置和部署中文文档，并在 README 中文入口中列出。
- Sidecar 完整身份转发、Runtime 完整闭环和部署级跨协议 E2E 仍待实现，因此不创建 M4 总 Tag。

## M4 完成定义

- Agent 循环、审批、工具、提交和恢复真实运行；
- 非幂等工具不会被自动重复；
- Embedded/REST/gRPC/MCP 语义一致；
- Sidecar 本地认证、单实例和优雅关闭通过测试；
- 服务无法绕过 Core ACL；
- Prediction/Fact 隔离验收通过；
- 全仓门禁通过，无占位实现。
