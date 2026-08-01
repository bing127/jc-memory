# 疆诚记忆官网与混合控制面设计规格

- **状态：** 已确认
- **日期：** 2026-07-31
- **产品名称：** 疆诚记忆
- **英文名称：** Jiangcheng Memory
- **设计范围：** 公开官网、登录与 Get Started、Memory 工具控制台、Agent Runtime 全链路控制台、治理与运维入口
- **前端建议：** React、Next.js、TypeScript、shadcn/ui、Tailwind CSS

## 1. 目标

本设计为疆诚记忆增加一个可登录的 Web 产品入口。它同时承担两类产品职责：

1. 作为独立 Memory 系统，为开发者和平台提供 Observe、Recall、Explain、Memory Packet、状态、认知结构、巩固、安全治理与恢复工具；
2. 作为 Agent Runtime 控制面，展示并管理从观察、召回、模型推演、策略、审批、工具执行到结果提交和恢复的完整链路。

产品结构采用：

```text
Memory Infrastructure 入口
        +
Agent Runtime 入口
        |
共享 Evidence / Scope / ACL / Audit / Recovery
        |
同一 Rust CommandBus、MemoryEngine 与权威存储
```

Web 层不得重写记忆语义、绕过 Core ACL、直接写 SQLite，或把控制面缓存变成权威记录。

## 2. 产品定位

疆诚记忆不是聊天记录浏览器、向量数据库面板或普通 RAG Playground。官网与控制台必须持续表达以下差异：

- 模型可以替换，权威记忆、身份、证据和项目状态不能随模型丢失；
- 原始 Fragment、Event 和 EvidenceRef 始终高于摘要、Embedding、Point、Edge、Surface 和 UI 展示；
- Prediction、Inference 和 Plan 不得在界面或后端中冒充 Verified Fact；
- Recall 不只返回文本，还返回模型无关、证据相连、受硬预算约束的 Memory Packet；
- Runtime 不只展示最终答案，还展示 Policy、Approval、ToolResult、Outcome 和 Commit；
- 本地、Sidecar、Server、MCP 与未来 SDK 使用同一核心语义。

官网主标题采用产品名作为首屏第一信号：

> 疆诚记忆

支持文案：

> 让 Agent 保留过去，并对每次行动负责。

描述文案：

> 独立于模型的认知记忆系统与 Agent Runtime。原始证据始终可追溯，预测不会冒充事实。

## 3. 事实来源与状态标记

后台功能不能只由页面想象产生。每项功能必须标记为以下四种状态之一：

| 标记 | 含义 | 是否可以进入生产导航 |
| --- | --- | --- |
| `LIVE` | 当前已有可调用的 Server、MCP 或 Sidecar 契约 | 可以，但只显示真实返回字段 |
| `CORE` | Rust 领域能力和测试已存在，但缺少受保护的 Web/Server 查询或管理契约 | 完成适配层、授权和 E2E 后可以 |
| `ROADMAP` | 现有产品文档和实施计划已经定义，但实现或真实验证尚未完成 | 不可以，只进入路线图或状态矩阵 |
| `PLATFORM` | 为满足登录、托管、团队和商业化而新增的 Web 产品基础能力 | 完成独立设计、实现和验证后可以 |

状态标记是产品契约，不是装饰标签。官网的“立即可用”、定价权益和控制台导航必须使用同一份能力状态注册表。

### 3.1 当前可直接接入的服务能力

`memory-server` 当前公开：

- `/healthz`；
- `/readyz`；
- `/metrics`，当前只返回 `jc_memory_server_ready 1`；
- `/v1/memory/command`；
- `/v1/memory/observe`；
- `/v1/memory/commit`；
- `/v1/memory/recall`；
- `/v1/memory/stats`；
- `/v1/memory/correct`；
- `/v1/memory/forget`；
- `/v1/memory/explain`；
- 同一 `CommandBus` 的 Tonic gRPC `Execute`；
- API Key、请求超时、最大并发、按 API Key 速率限制和稳定错误分类。

MCP 当前公开七个结构化工具：

```text
memory_observe
memory_recall
memory_commit
memory_correct
memory_explain
memory_forget
memory_stats
```

Sidecar 当前提供 `status`、`stop`、`doctor`，并能转发上述七类业务命令。

### 3.2 当前必须如实展示的限制

- `/healthz` 与 `/readyz` 当前是静态服务状态，不等同于 `MemoryEngine::health` 的数据库、Blob 和 Schema 完整性报告；
- `/metrics` 当前没有请求量、错误率、吞吐或延迟分位数；
- Server 二进制当前只从启动配置加载一个 API Key 和一个 Namespace；
- 远程 Server 当前授予 Observe、Recall、Mutate 和 Explain，不包含 Admin、Export、Delete 或 Consolidate；
- HTTP 没有内建 TLS，生产访问必须位于 HTTPS Ingress、Service Mesh、反向代理或受保护网络后；gRPC 已支持文件型 TLS；
- Runtime、Approval、Consolidation、Quarantine 审查、Snapshot、Restore、Rebuild、Retention、Export 和 Hard Delete 尚未形成完整 Web 服务契约；
- Memory Packet 的类型定义完整，但状态、语义知识和 Surface 尚未全部汇入当前 Core 返回路径；空数组不能被界面解释为系统不存在该类记忆；
- M4 已有同一真实 Fixture 贯穿 Embedded、REST、gRPC、Sidecar 和 MCP 的 Recall 一致性测试；Web 控制面另有 Fixture 和真实本地 Rust Server 两类 Playwright 主路径；
- M5 SDK/Adapter 与 M6 ONNX/Benchmark/Release 的仓库实现和契约测试已经存在；未经过当前环境验证的外部平台、第三方服务和私有发布产物仍不得标记为实机验证或已经发布。

## 4. 参考站点的取舍

设计参考以下站点，但不复制其品牌或组件：

| 参考 | 吸收的原则 | 不照搬的部分 |
| --- | --- | --- |
| Ailoooook | 深色品牌叙事、真实产品画面、治理与组织级价值表达 | 过多装饰、与疆诚记忆无关的组织对象和数字员工概念 |
| Spline Pricing | 清晰导航、明确 Get Started、套餐层级与功能比较 | 具体价格、席位计费、AI Credits 和不属于本项目的协作功能 |
| Portainer | 运维控制面心智、实例选择、密集但清晰的后台导航 | 容器、Cluster、Node、GitOps 和当前项目不存在的 Agent 穿透能力 |

## 5. 用户与角色

产品采用多角色控制台，但多角色不是前端隐藏按钮。

| 角色 | 默认工作区 | 允许的产品操作 | 默认禁止 |
| --- | --- | --- | --- |
| Owner / Admin | 总览、实例、治理、恢复 | 管理实例连接、Scope、治理策略和恢复任务 | 未重新授权的 Restore、Hard Delete 和 Retention Execute |
| Developer | Memory、Runtime、Connect | Observe、Recall、Explain、受授权的 Mutate、Run 调试、Capability 接入 | 无授权 Scope、治理删除、跨租户访问 |
| Auditor | Evidence、Explain、Governance | 查看证据、状态、Explain、审计和 Quarantine 记录 | Observe、Mutate、Tool Execute |
| Approver | Approvals、Run Detail | 查看请求证据并批准或拒绝被分配的请求 | 修改其他 Scope 或未分配审批 |

当前 `Operation` 和单 API Key 启动模型不能完整表达上述角色。多角色上线前必须：

1. 扩展服务端凭据模型，使多个凭据可以映射到精确 Scope 与 Operation；
2. 必要时细化只读审计、审批和治理 Operation；
3. BFF RBAC 与 Rust ACL 同时通过；
4. 证明前端、BFF 或直接协议调用都不能绕过 Core 授权。

## 6. 公开官网

### 6.1 顶部导航

```text
疆诚记忆
产品
Runtime
安全治理
部署
文档
定价
登录
Get Started
```

### 6.2 首页内容顺序

1. 首屏：产品名、价值主张、真实 Run 状态与 Evidence 摘要；
2. 为什么普通聊天记录或向量检索不足；
3. Memory Engine：Observe、Recall、Explain、Memory Packet；
4. Agent Runtime：完整状态机、Policy、Approval、Tool、Commit；
5. 可信边界：Evidence、Validity、Scope、ACL、Quarantine；
6. 本地优先与部署模式：Embedded、Sidecar、Server、MCP；
7. 当前支持状态矩阵，不把路线图写成已交付；
8. 套餐与 Get Started；
9. 文档、部署和安全入口。

首屏使用真实产品状态或可运行 Fixture 的画面，不使用与产品无关的抽象渐变插画。首屏在常见桌面和移动视口中保留下一区块提示，避免整屏只剩营销文案。

### 6.3 关键产品演示

官网只演示版本化 Fixture 可以证明的链路：

- 跨年召回；
- PostgreSQL 到 MySQL 的当前/历史状态；
- “测试应该通过”的 Prediction 被实际失败 ToolResult 否定；
- 跨 Tenant 隔离；
- Snapshot/Restore 后权威哈希一致。

Fixture 演示必须标为演示或契约验证，不能描述为真实外部平台验证。

## 7. 登录与 Get Started

### 7.1 登录边界

登录、账号、组织角色和实例登记属于 `PLATFORM`，不属于当前 Memory Core。

- Managed Cloud 采用标准身份提供方和服务端 HttpOnly Session；
- 自部署控制面支持一次性引导的本地管理员账号；
- 未完成真实身份提供方、邮件、MFA 或 SSO 集成前，官网不宣称支持对应能力；
- 浏览器不保存实例 API Key；
- Session 过期后停止代理新请求，不继续显示旧数据为当前状态。

### 7.2 双路径向导

用户登录后选择：

#### 路径 A：创建试运行实例

1. 创建隔离 Sandbox；
2. 分配独立数据目录、Namespace 和 Secret；
3. 加载版本化验收 Fixture；
4. 完成 Observe -> Recall -> Explain；
5. 进入同一个 Recall Lab 和 Run Detail。

现有 Docker/Kubernetes 文件只证明部署资源存在。自动 Provision、实例生命周期、过期清理和配额属于新增平台能力，完成前不能宣称 Sandbox 可用。

#### 路径 B：接入现有实例

1. 填写可达的 Server 地址、Namespace 和 API Key；
2. 校验证书或 HTTPS 边界；
3. 调用 Health/Ready 和 Stats；
4. 执行版本兼容检查；
5. 运行同一条 Recall/Explain 验证链路；
6. 保存加密凭据引用，不把密钥返回浏览器。

当前没有 Portainer Agent 式反向通道。私网实例需要 VPN、专线、受控反向代理或可达的 gRPC TLS。控制面不得暗示具有内网穿透能力。

## 8. 控制台信息架构

```text
总览
Memory
  试运行
  Recall 与 Explain
  证据与状态
  认知图与 Surface
Runtime
  Runs
  Approvals
  Models
  Capabilities
Operations
  Consolidation
  Recovery
Governance
  Scopes
  Quarantine
  Audit
  Retention
Connect
  Instances
  Server / Sidecar / MCP
  SDK / Adapters
Evaluation
  Acceptance Fixtures
  Benchmarks
Settings
```

无权限模块不出现在导航中。没有真实查询契约的模块在生产环境整体隐藏，不能用虚构的 0、空图表或禁用按钮假装已经接入。

## 9. 总览

默认首页以 Operator 视角呈现，但按角色裁剪。

### 9.1 可以显示的现有数据

- 服务 Health/Ready；
- 当前 `server_ready`；
- 精确 Scope 的 Fragment/Event 数量；
- 最近一次控制面请求的状态和 Request ID；
- 当前连接方式、协议版本和 Namespace。

### 9.2 完成适配后可以显示的数据

- `MemoryEngine::health` 的 database_ok、blob_store_ok、schema_version；
- Runtime Run 当前状态和最近 Transition；
- Consolidation Job 与 Checkpoint；
- Quarantine 待审数量；
- Snapshot Manifest 和最近恢复结果。

### 9.3 不得提前显示的数据

- Recall P50/P95/P99；
- 请求吞吐和错误率曲线；
- CPU、内存和存储趋势；
- Token、模型成本或云账单；
- 尚未实现的告警数量。

## 10. Memory 工作区

### 10.1 试运行

提供类型化命令向导：

- Observe；
- Commit；
- Correct；
- Forget；
- Stats。

界面必须展示 Request Envelope、Access Scope、命令输入、稳定响应、Request ID、错误码和 Canonical JSON。写操作显示实际产生的 Fragment ID、Event ID、Sequence、Content Hash 和 Quarantine 状态。

写操作默认不自动重试。

### 10.2 Recall Lab

输入项直接映射 `RecallQuery`：

- Scope；
- Recall Mode；
- Text；
- Entities；
- At Time；
- Max Seeds；
- Max Neighbors；
- Max Hops；
- Max Nodes；
- Timeout Ms。

支持十种模式：

```text
Exact
Semantic
Temporal
Causal
Entity
Associative
Procedural
CurrentState
HistoricalState
DeepReconstruction
```

结果区严格映射 Memory Packet：

- Current States；
- Historical States；
- Activated Surfaces；
- Episodes；
- Points；
- Paths；
- Procedures；
- Constraints；
- Preferences；
- Past Failures；
- Evidence；
- Conflicts；
- Missing Evidence；
- Coverage；
- Confidence；
- Recommended Context。

当前 Core 没有填充的 Packet 字段显示“当前调用链未提供”，不能显示“没有此类记忆”。

### 10.3 Explain

Explain 直接呈现：

- Seeds 与初始分数；
- Traversals、Edge Kind、Depth 和 Score；
- Surface ID、Version 和 Activation；
- Inferred Edges；
- Verified Fragments；
- Recall Limits；
- Truncated。

图形只是结构化 Trace 的视图。用户必须能切回表格和 Canonical JSON，不得通过布局省略推断边或截断状态。

### 10.4 Evidence Inspector

从 Run、Packet、Point、Path 或 Explain 中的 Evidence ID 进入详情：

- Fragment ID、Event ID、EvidenceRef ID；
- Kind、Validity、Security Label；
- Source、Perspective；
- Occurred At、Observed At；
- Confidence、Importance；
- Verification Method；
- 原始内容或 Blob 引用；
- Content Hash；
- 关联状态、Point、Edge、Surface 和 Run。

当前远程协议只能返回证据 ID，完整详情需要新增受 Scope 保护的读取契约。新增列表 API 必须分页、有硬上限、按 Scope 过滤，禁止全库扫描后在 Web 层筛选。

## 11. Runtime 工作区

### 11.1 完整链路

控制台使用文档已经定义的状态闭环：

```text
Observe
-> Recall
-> Build Memory Packet
-> Resolve Capabilities
-> Invoke Model
-> Parse Hypotheses
-> Apply Policy
-> Request Approval
-> Execute Tool
-> Capture Result
-> Commit Transition
-> Check Completion
-> Retry / Pause / Resume
```

### 11.2 Run 列表

列表按精确 Scope 查询，默认字段为：

- Run ID；
- State；
- Next Step；
- Version；
- Trace ID；
- Updated At；
- Control State；
- Attempt；
- 是否等待用户或审批。

当前 RuntimeRunner 已持久化 Run 和 Transition，但没有完整列表服务契约。列表上线前必须提供分页、状态过滤、时间范围和 Scope 过滤。

### 11.3 Run Detail

Run Detail 是整个产品的核心页面。它包含：

- 状态时间线；
- 每个 Transition 的 From/To、Step、Attempt、Deadline 和 Trace ID；
- Input Refs 与 Output Refs；
- Payload；
- Recall Packet 与 Explain；
- ModelOutput 的真实类型：Hypothesis、Prediction、Plan、DecisionCandidate、ToolRequest 或 FinalResponse；
- Policy Decision；
- Approval Request 与 Decision；
- Tool Invocation、ToolResult 和 Outcome；
- Commit 产生的权威 Fragment/Event；
- Completion、Retry、Pause、Resume、WaitingUser 与 Cancel。

界面必须把 Prediction、ToolResult 和 Verified Outcome 以不同颜色和标签呈现，不能只显示一段合并后的自然语言。

### 11.4 高风险动作

- Pause/Resume/Cancel 需要服务端 Runtime 控制契约；
- Approval 只能处理分配给当前主体的请求；
- 非幂等 Tool Execute 不自动重放；
- 恢复时必须显示上次已提交状态和明确的恢复决策；
- Commit 失败时不能把 ToolResult 标成已经进入权威记忆。

## 12. Capability、Model 与 Tool

Capability 页面覆盖文档中定义的：

```text
Prompt
Skill
Tool
MCPTool
Workflow
Policy
```

详情展示 ID、Version、Source、Permission、Input/Output Schema、Risk Level、Approval Requirement、执行记录、成功率、失败模式和适用 Surface。

Capability 统计属于程序记忆，界面必须显示样本数和 Evidence，不能把统计关系渲染成绝对规则。

Model 页面展示 Provider 类型、Descriptor、版本、超时、Fallback 和 Circuit Breaker 状态。API Key 和认证 Header 只显示是否配置，不显示明文。

当前 Capability Registry、统计、Model Provider、Policy、Approval、Tool 和 MCP Client 已有 Rust 实现；它们仍需统一的服务查询与管理契约，以及完整 Agent 循环 E2E。

## 13. Consolidation 与 Operations

Consolidation 页面只能映射已经定义的十二类 Job：

```text
Replay
Deduplication
EdgeReinforcement
EdgeWeakening
SurfaceDiscovery
SurfaceMerge
SurfaceSplit
SemanticPromotion
ConflictDetection
StaleStateDetection
ColdTierMovement
Rebuild
```

每个 Job 展示 Kind、Scope、State、Cursor、Checkpoint、Attempt、Lease、Limits、Started/Updated At 和错误。支持 Pause、Resume、Retry 和 Cancel，但必须先把 `Operation::Consolidate` 接入受 `MemoryEngine` 或服务层保护的提交入口。

界面不得提供任意 SQL、任意 Handler 名称或跳过审计的“立即全库重建”。

## 14. Governance 与 Recovery

### 14.1 Quarantine

- 待审列表；
- Risk Label、Score、Detector Version 和 Evidence；
- 目标 Fragment；
- Release 或 Reject；
- 审查主体、理由和追加式状态事件。

当前已有风险检测和 `review_quarantine`，但缺远程待审列表与管理契约。

### 14.2 Audit

审计列表显示 Principal、Operation、Target Digest、Request ID、Result 和 Time，不显示正文或密钥。审计查询必须分页并受 Scope 约束。

### 14.3 Retention、Export 与 Hard Delete

- Retention 先生成计划，再单独执行；
- Export 显示 Manifest、Canonical JSONL 和 Blob 清单；
- Hard Delete 显示精确 Scope、影响对象和不可逆说明；
- 成功后显示不含正文的审计凭证。

Restore、Hard Delete、Retention Execute 和 Quarantine Reject 要求重新认证或二次确认，并在服务端重新授权。

### 14.4 Snapshot、Restore 与 Rebuild

- Snapshot 展示 Scope、Schema Version、SQLite Checksum、Blob 清单和创建时间；
- Restore 在隔离目录校验完成前不能改变当前数据目录；
- Rebuild 展示 FTS、Embedding、Edge Candidate、Surface Member、Summary/Cache 阶段和 Checkpoint；
- UI 不允许在仍有活跃 Engine 连接时直接 Restore。

## 15. Connect 与 Evaluation

### 15.1 Connect

按真实交付状态展示：

- Embedded Rust；
- Server REST/gRPC；
- Sidecar；
- MCP；
- Node、Python、Go、Kotlin、Swift、Java SDK；
- Hermes、OpenClaw、Mem0、Graphiti、Dify Adapter。

SDK 和 Adapter 未完成时只显示路线图状态、契约目标和 Fixture 范围，不显示安装命令或“已验证”徽标。

### 15.2 Evaluation

分开呈现：

- 产品验收 Fixture；
- Benchmark Smoke；
- 完整数据集结果；
- Million Fragments 长测；
- 当前主机真实验证；
- CI 配置但未运行的平台；
- 外部平台 Fixture 兼容。

Benchmark 页面不能充当实时监控页面。所有报告必须包含 Git Commit、Dirty 标志、平台、数据 Digest、配置、时间和原始计数。

## 16. 混合控制面架构

### 16.1 组件

```text
Browser
  -> Next.js React UI
  -> Web BFF
  -> gRPC TLS 或 HTTPS Ingress 后的 REST
  -> memory-server
  -> CommandBus
  -> MemoryEngine / Runtime / Governance
  -> SQLite WAL + BlobStore
```

### 16.2 Web Control Plane

建议新增 `apps/control-plane-web/`，职责仅包括：

- 官网、登录、Get Started 和控制台；
- Account、Organization、Role 和 Session；
- Instance Registry；
- 加密的实例凭据引用；
- BFF 请求代理与稳定错误映射；
- UI 偏好；
- 不含正文的控制面操作审计。

首版控制面元数据使用独立 SQLite WAL，不能与任何实例的权威 Memory 数据库共用文件或事务。

### 16.3 数据驻留

控制面可以持久化：

- 账号和组织角色；
- 实例 URL、Namespace 和协议版本；
- 加密凭据；
- UI 偏好；
- 脱敏的 Request ID、操作类型、状态和耗时；
- 套餐和实例生命周期元数据。

控制面默认不持久化：

- Fragment 正文；
- Event 内容；
- EvidenceRef 内容；
- Memory Packet；
- ModelOutput；
- ToolResult；
- Snapshot；
- API Key 明文。

正文与结构化响应只在当前代理请求和浏览器会话中短暂存在。未来如增加请求历史持久化，必须单独设计 Retention、加密、Scope、导出和删除语义。

### 16.4 凭据与传输

- 浏览器不接触实例 API Key；
- BFF 通过 Secret Store 或加密字段读取凭据；
- 日志不得记录 Authorization、API Key、私钥或正文；
- 生产自部署优先使用 gRPC TLS；
- REST 必须位于 HTTPS 反向代理或受保护网络后；
- `http://` 只允许本机或明确隔离的开发模式；
- 删除实例连接只删除控制面元数据和凭据，不删除实例记忆。

## 17. 前端技术与组件

### 17.1 技术栈

- Next.js + React + TypeScript；
- shadcn/ui + Tailwind CSS；
- TanStack Query 管理服务端状态；
- React Hook Form + Zod 管理强类型表单；
- Monaco Editor 编辑和比较 JSON；
- 成熟图组件展示 Explain Path 与认知图；
- 成熟图表库展示真实遥测；
- Vitest、Testing Library 和 Playwright；
- OpenAPI/Protobuf 生成类型或共享协议包，不手写重复 DTO。

### 17.2 组件原则

- 命令使用按钮，模式使用 Select 或 Segmented Control；
- 二元设置使用 Toggle 或 Checkbox；
- Recall 数值预算使用 Input/Stepper，不用纯文本链接；
- 工具按钮优先使用现有图标库；
- 卡片圆角不超过 6px；
- 不嵌套装饰性卡片；
- 表格、时间线、JSON 和证据详情保持稳定尺寸；
- 无权限操作不显示；不可达操作显示原因，不只禁用；
- 所有长 ID 支持复制并保留完整值；
- 移动端允许查看、审批和告警处理，复杂图与 JSON 编辑主要面向桌面。

## 18. 视觉语言

### 18.1 官网

- 深色石墨背景、白色主文本；
- 青色表示信息路径，绿色表示 Verified/Healthy，琥珀表示待审批或 Unverified，红色表示 Conflict/Unhealthy；
- 首屏使用真实 Run、Memory Packet 或 Explain 画面；
- 只使用状态演进和数据流动效，不使用无意义装饰动画；
- 产品名必须在首屏明显出现；
- 首页不使用大面积紫色渐变或抽象光球。

### 18.2 控制台

- 默认浅色、支持深色；
- 左侧一级导航、顶部实例/Scope/角色选择、右侧主工作区；
- 信息密度接近 Portainer，页面用于扫描、比较和重复操作；
- 小标题、状态标签和表格优先，避免营销式大字；
- Inter 与 Noto Sans SC，字间距为 0；
- 字号不随视口宽度缩放。

## 19. 套餐与转化

采用混合模式，但本设计不虚构货币金额、配额或 SLA。

### 19.1 免费试运行

- 版本化验收 Fixture；
- Observe、Recall、Explain；
- 完整 Run 演示；
- 不用于生产持久化；
- 无生产 SLA；
- CTA：开始试运行。

### 19.2 Managed Cloud

- 托管隔离实例；
- Memory 与 Runtime 控制面；
- 团队角色与实例管理；
- 在对应能力完成后提供备份、Retention、升级、配额和 SLA；
- CTA：联系团队。

### 19.3 Enterprise

- 本地、Docker 或 Kubernetes 私有部署；
- Server、Sidecar 和 MCP；
- 治理与恢复；
- 部署支持和契约验证；
- CTA：申请技术评估。

定价页在计费系统和运营能力完成前只展示交付边界和联系入口，不显示虚构价格。

## 20. 错误、空状态与降级

### 20.1 稳定错误映射

| 服务错误 | UI 行为 |
| --- | --- |
| `invalid_argument` | 定位具体输入字段并保留用户内容 |
| `permission_denied` | 显示 Scope、Operation 和 Request ID，不建议扩大权限 |
| `not_found` | 区分目标不存在与无权读取，不泄漏其他 Scope 信息 |
| `conflict` | 显示并发或状态冲突，要求重新加载 |
| `resource_exhausted` | 显示限流或并发耗尽，不自动重放写操作 |
| `deadline_exceeded` | 显示请求超时和 Request ID，允许只读请求显式重试 |
| `internal` | 显示脱敏错误和 Request ID，不展示密钥、正文或内部堆栈 |

### 20.2 连接状态

- `Healthy`：最新请求成功且真实 Health 通过；
- `Ready`：当前只证明服务就绪；
- `Degraded`：Core Health 指出派生层或清理需要处理；
- `Unhealthy`：数据库或 Blob 权威证据不完整；
- `Unreachable`：连接失败，不能用缓存冒充当前状态；
- `Incompatible`：协议版本不兼容。

### 20.3 重试

- Recall、Explain、Stats 和 Health 可由用户显式重试；
- Observe、Commit、Correct、Forget、Approval、Tool Execute、Restore 和 Delete 不自动重试；
- 非幂等 Tool 恢复必须由 Runtime 产生明确决策；
- UI 重复提交必须使用 Request ID 和幂等边界防止误操作。

## 21. 监控设计

### 21.1 当前可用

- Health/Ready；
- `server_ready`；
- Scope Fragment/Event 数量；
- 结构化错误与 Request ID。

### 21.2 领域数据待暴露

- DB/Blob/Schema Health；
- Run、Transition 与 Control Checkpoint；
- Consolidation Job、Lease 与 Checkpoint；
- Audit；
- Snapshot Manifest。

### 21.3 必须新增的生产遥测

- 请求量、成功率和错误分类；
- Recall/Explain/Commit P50/P95/P99；
- 写入吞吐；
- 队列和 Job 延迟；
- SQLite/Blob 容量和增长；
- Runtime 各状态停留时间；
- Approval 等待时间；
- Provider 超时、重试、Fallback 和 Circuit Breaker；
- 告警规则和 Retention。

M6 Benchmark 指标用于可重复评测，不作为实时生产遥测数据源。

## 22. 分阶段交付

### Phase 1：Memory Console

- 官网、登录和 Instance Registry；
- 双路径 Get Started；
- Health/Ready、Stats；
- Observe、Commit、Correct、Forget；
- Recall Lab、Memory Packet、Explain、Canonical JSON；
- 当前能力状态矩阵、接入和部署文档。

### Phase 2：Runtime Chain

- Run 列表与详情查询契约；
- Transition 和 Evidence 展开；
- Model、Capability、Policy 和 Tool 查看；
- Approval 决策；
- Pause/Resume/Retry/WaitingUser；
- 确定性完整 Agent 循环；
- Embedded/REST/gRPC/Sidecar/MCP 真实 E2E。

### Phase 3：Governance & Operations

- 多凭据、角色和精细 Operation；
- Quarantine 列表与审查；
- Consolidation 提交和控制；
- Snapshot、Restore、Rebuild；
- Retention、Export、Hard Delete；
- 生产遥测、历史查询和告警。

### Phase 4：Platform

- Managed Sandbox Provision 和生命周期；
- 团队托管实例；
- SDK/Adapter 状态和真实安装入口；
- Benchmark 报告；
- 商业配额、套餐和 SLA；
- 跨平台与外部环境验证。

## 23. 页面上线门禁

每个页面或操作必须同时满足：

1. Rust 领域能力已经存在，或正式产品需求已批准；
2. HTTP/gRPC/MCP 契约版本化；
3. 服务端使用精确 Scope 与 Operation 授权；
4. 行为变更按 TDD 实现；
5. 聚焦单元、契约、错误路径和租户隔离测试通过；
6. React 页面覆盖 Loading、Empty、Error、Unauthorized、Stale 和 Success；
7. Playwright 覆盖真实本地 Server 的主路径；
8. 关键桌面和移动视口无文本溢出、遮挡和不可操作控件；
9. 官网和能力状态注册表没有把 Fixture、计划或静态配置写成已验证功能；
10. `git diff --check`、适用的 fmt、clippy、test 和 doc 门禁通过。

任一门禁缺失，该能力只能出现在路线图或状态矩阵中，不能进入生产导航。

## 24. 测试策略

### 24.1 前端

- 组件与状态渲染单元测试；
- Zod 与协议 Fixture 往返测试；
- 角色导航和操作可见性测试；
- 错误码、超时、断线和过期 Session 测试；
- Memory Packet、Explain 和 Evidence 的字段完整性测试；
- 无障碍和键盘操作测试。

### 24.2 BFF

- 登录 Session、CSRF、实例凭据脱敏；
- API Key 不进入浏览器、日志或错误；
- Scope 和角色映射；
- REST/gRPC 契约测试；
- 非幂等写入不自动重试；
- 控制面不持久化 Memory 正文。

### 24.3 Rust 服务适配层

- 先写缺失行为的失败测试；
- 列表 API 分页、硬上限和 Scope 过滤；
- Runtime、Approval、Consolidation、Governance 和 Recovery 的权限矩阵；
- 跨 Tenant 泄漏、Prompt Injection、Memory Poisoning；
- Snapshot/Restore、Hard Delete 和崩溃恢复；
- 同一 Fixture 经 Embedded、REST、gRPC、Sidecar 和 MCP 的 Canonical JSON/Evidence 一致性。

### 24.4 端到端

最低覆盖：

1. 注册或登录；
2. 创建 Sandbox 或接入本地实例；
3. Observe 一个 Fragment；
4. Recall 并查看 Memory Packet；
5. Explain 并展开 Evidence；
6. 完整 Runtime Run 进入 Approval；
7. ToolResult 否定 Prediction；
8. Commit 后权威记录可重新 Recall；
9. 越权 Scope 被拒绝且不泄漏部分 Packet；
10. 断线、超时和限流状态正确；
11. 高风险治理操作需要二次确认；
12. 删除实例连接不删除实例数据。

## 25. 非目标

- 不在 Web 层实现第二套记忆数据库；
- 不直接编辑 SQLite、Blob、Point、Edge 或 Surface 权威关系；
- 不增加未经文档和代码支持的容器、Cluster、Node 或 GitOps 功能；
- 不假设存在 Portainer Agent 式内网穿透；
- 不把 Benchmark 结果当作实时监控；
- 不把空 Packet 字段解释成“没有此类记忆”；
- 不把 LLM 文本、摘要或 UI 推断写成 Verified Fact；
- 不在尚未实现计费、配额和 SLA 时展示虚构价格；
- 不用前端角色代替服务端 Scope 与 Operation 授权；
- 不把 Fixture 兼容写成外部平台实机验证。

## 26. 验收标准

本设计完成后的产品必须满足：

1. 用户可以通过登录进入控制台，并选择 Sandbox 或接入现有实例；
2. Memory 能独立作为工具使用，不要求先创建 Agent；
3. Runtime 能展示从 Observe 到 Commit 的完整状态和证据链；
4. 所有可见事实、状态、关系和运行结果都能回到权威 Fragment/Event/EvidenceRef；
5. Prediction、Inference、Plan、Decision、ToolResult 和 Verified Outcome 在 UI 中明确区分；
6. Control Plane 默认不持久化实例记忆正文；
7. 多角色权限同时由 BFF 和 Rust 服务端约束；
8. 自部署数据保持在实例边界，删除连接不删除记忆；
9. 当前能力、适配中能力和路线图能力在官网、定价和控制台中保持一致；
10. 没有后台页面或监控指标建立在项目未定义、未实现且未标记的臆想能力上。
