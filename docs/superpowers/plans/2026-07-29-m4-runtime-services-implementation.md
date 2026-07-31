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

- [ ] **步骤 1：写 Health、Observe、Recall、错误码、API Key、限流和优雅关闭失败测试。**
- [ ] **步骤 2：运行测试**，预期 Server crate 缺失。
- [ ] **步骤 3：Axum 路由 `/v1/memory/{observe,recall,commit,correct,forget,explain,stats}` 和 `/healthz`、`/readyz`、`/metrics`；Extractor 构造 AccessContext，禁止客户端伪造授权 Grant。**
- [ ] **步骤 4：Tonic 实现同一 CommandBus；REST/gRPC 共享 ErrorCode、request_id、trace_id；TLS 仅从文件/KeyProvider 加载，不记录密钥；Tower 加入超时、并发和速率限制。**
- [ ] **步骤 5：运行协议测试并提交 `feat(server): expose secured HTTP and gRPC APIs`**。

## Task 8：实现本地 Sidecar

**文件：** `services/memory-sidecar/src/{main,config,transport,lock,token,lifecycle}.rs`、`src/bin/{jcmem,jcmemd}.rs`、集成测试。

- [ ] **步骤 1：写第二实例被锁拒绝、Token 错误、崩溃重启和 shutdown 失败测试。**
- [ ] **步骤 2：运行测试**，预期 Sidecar crate 缺失。
- [ ] **步骤 3：实现 Transport Trait；Unix 使用权限 0600 的 UDS，Windows 使用随机 Loopback 端口；Named Pipe 只保留 Trait 扩展点，不宣称已实现。`jcmemd` 运行守护进程；`jcmem` 提供 start/status/stop/doctor/observe/recall 子命令并使用同一协议 Client。**
- [ ] **步骤 4：Token 使用 256-bit 随机值、权限受限文件和常量时间比较；文件锁记录 PID/启动标识并检测陈旧锁；信号触发停止接收、等待 in-flight、checkpoint、关闭 DB。**
- [ ] **步骤 5：运行测试并提交 `feat(sidecar): add authenticated local daemon`**。

## Task 9：实现 MCP Server

**文件：** `services/memory-mcp/src/{main,server,tools,schema}.rs`、`tests/mcp_contract.rs`。

- [ ] **步骤 1：写 initialize、tools/list 和七个 tools/call 的 Schema/结果失败测试。**
- [ ] **步骤 2：运行测试**，预期 MCP Server 缺失。
- [ ] **步骤 3：实现 memory_observe、memory_recall、memory_commit、memory_correct、memory_explain、memory_forget、memory_stats；每个 inputSchema 禁止额外未知字段并要求 Access Scope。**
- [ ] **步骤 4：结果使用 MCP structuredContent 包含协议对象；错误返回稳定 code/data，不把自然语言当唯一结果。**
- [ ] **步骤 5：运行测试并提交 `feat(mcp): expose structured memory tools`**。

## Task 10：端到端一致性、文档与门禁

**文件：** `tests/e2e/runtime_loop.rs`、`tests/e2e/protocol_parity.rs`、部署与协议文档、README/CHANGELOG。

- [ ] **步骤 1：确定性 Model/Tool 完成完整循环，实际失败否定“测试会通过”的 Prediction。**
- [ ] **步骤 2：同一 Fixture 经 Embedded、REST、gRPC、MCP 调用，canonical Memory Packet 和 Evidence 集合一致。**
- [ ] **步骤 3：编写 Runtime、Capability、Server、Sidecar、MCP、故障恢复和配置文档。**
- [ ] **步骤 4：运行全仓 Rust 门禁、服务 E2E 和 `git diff --check`。**
- [ ] **步骤 5：提交 `docs: complete M4 runtime and services milestone`**。

## M4 完成定义

- Agent 循环、审批、工具、提交和恢复真实运行；
- 非幂等工具不会被自动重复；
- Embedded/REST/gRPC/MCP 语义一致；
- Sidecar 本地认证、单实例和优雅关闭通过测试；
- 服务无法绕过 Core ACL；
- Prediction/Fact 隔离验收通过；
- 全仓门禁通过，无占位实现。
