你现在要从零构建一个完整、可运行、可测试、可发布的开源项目，项目名称为：

# 疆诚记忆

英文及代码标识统一使用：

* 产品名称：疆诚记忆
* 英文名称：Jiangcheng Memory
* 仓库名称：`jiangcheng-memory`
* Rust crate 前缀：`jc-memory-*`
* CLI：`jcmem`
* Sidecar/Server：`jcmemd`

## 一、项目定位

疆诚记忆不是聊天记录库、向量数据库封装或普通 RAG Memory，而是一个：

> 独立于大语言模型的 Memory-Native Agent Kernel 与神经认知记忆数据库。

系统应满足：

```text
疆诚记忆
+ 任意大模型
+ Prompt / Skill / Tool / MCP
= 可以长期存在、持续学习、跨模型迁移的 Agent
```

核心理念：

```text
Memory负责保存过去并从过去中学习
LLM负责推演未来、推理和提出决策候选
Runtime负责当前选择、权限、执行与结果提交
工具负责对现实环境产生作用
```

LLM必须是无状态、可替换的外部推理器。更换GPT、Claude、Qwen、DeepSeek、本地模型后，Agent的身份、过去、经验、偏好和项目状态不能丢失。

## 二、重要工作要求

1. 首先检查当前仓库、`AGENTS.md`、现有代码和Git状态。
2. 如果仓库为空，从零初始化完整Monorepo。
3. 如果已有代码，保留用户现有修改，不覆盖无关内容。
4. 先给出分阶段实施计划，然后持续实现，不要只停留在方案。
5. 目标是完成整个仓库，而不是只创建脚手架。
6. 不允许用大量`TODO`、`unimplemented!()`、空函数或假实现冒充完成。
7. 外部服务不可用时，必须：

   * 完成真实接口；
   * 提供可重复的本地测试实现；
   * 提供契约测试；
   * 清楚记录需要外部服务验证的部分。
8. 每完成一个模块都要运行对应测试。
9. 最终运行全仓库质量检查，并报告真实结果。
10. 不要为了“完整”而堆砌没有调用链的文件。每个模块必须被集成、测试或明确作为公共扩展接口。
11. 核心逻辑中避免使用`unsafe`；确实需要时必须隔离、说明不变量并增加测试。
12. 所有派生记忆必须能够回溯原始证据。
13. 摘要、Embedding、图关系和认知面都不能覆盖或替代原始事件。
14. LLM生成的预测和推断绝不能作为已验证事实写入。
15. 项目默认本地优先、离线可运行，不强制依赖外部LLM、云端向量库或Python Runtime。

## 三、语言与技术栈

### 核心技术

* Rust Stable
* Rust 2024 Edition
* Tokio
* Serde
* SQLite WAL
* FTS5
* 内容寻址BlobStore
* Protobuf
* gRPC
* HTTP REST
* OpenAPI
* ONNX Runtime
* tracing
* structured logging

### 各层语言

```text
Memory Core：Rust
Sidecar/Server：Rust
MCP Server：Rust
模型训练与评测：Python + PyTorch
模型部署：ONNX
Node/Electron SDK：TypeScript + napi-rs
Python SDK：Python + PyO3
Go SDK：Go + gRPC/C ABI
Android SDK：Kotlin + UniFFI/JNI
iOS/macOS SDK：Swift + UniFFI
Java SDK：Java/Kotlin JVM绑定或gRPC
管理和示例界面：TypeScript
```

最终用户运行本地版本时，不得强制安装Python。

## 四、完整仓库结构

创建并实现以下完整Monorepo。可以根据编译边界做合理调整，但不能随意删除核心模块。

```text
jiangcheng-memory/
├── Cargo.toml
├── Cargo.lock
├── README.md
├── README.zh-CN.md
├── LICENSE
├── SECURITY.md
├── CONTRIBUTING.md
├── CHANGELOG.md
├── AGENTS.md
├── rust-toolchain.toml
├── deny.toml
├── .editorconfig
├── .gitignore
│
├── crates/
│   ├── memory-core/
│   ├── memory-protocol/
│   ├── memory-events/
│   ├── memory-episodic/
│   ├── memory-semantic/
│   ├── memory-graph/
│   ├── memory-surface/
│   ├── memory-activation/
│   ├── memory-consolidation/
│   ├── memory-storage/
│   ├── memory-neural/
│   ├── memory-security/
│   └── memory-runtime/
│
├── services/
│   ├── memory-sidecar/
│   ├── memory-server/
│   └── memory-mcp/
│
├── sdk/
│   ├── node/
│   ├── python/
│   ├── go/
│   ├── kotlin/
│   ├── swift/
│   └── java/
│
├── adapters/
│   ├── hermes/
│   ├── openclaw/
│   ├── mem0/
│   ├── graphiti/
│   └── dify/
│
├── proto/
│   └── jiangcheng/
│
├── models/
│   ├── training/
│   ├── evaluation/
│   ├── export/
│   ├── manifests/
│   └── onnx/
│
├── benchmarks/
│   ├── longmemeval/
│   ├── locomo/
│   ├── cross-year/
│   ├── model-swap/
│   ├── memory-poisoning/
│   └── million-fragments/
│
├── examples/
│   ├── rust-agent/
│   ├── node-electron/
│   ├── python-agent/
│   ├── go-agent/
│   ├── mcp-client/
│   └── model-swap/
│
├── docs/
│   ├── architecture/
│   ├── concepts/
│   ├── protocol/
│   ├── sdk/
│   ├── deployment/
│   ├── security/
│   ├── benchmarks/
│   └── adr/
│
├── scripts/
│   ├── build/
│   ├── test/
│   ├── release/
│   └── benchmark/
│
└── .github/
    ├── workflows/
    ├── ISSUE_TEMPLATE/
    └── pull_request_template.md
```

## 五、核心时间认知模型

系统时间闭环定义为：

[
M_t=(E_t,G_t,\Theta_t)
]

其中：

* (E_t)：不可变认知片段与原始证据；
* (G_t)：点、线、状态版本和显式关系；
* (\Theta_t)：连接权重、认知面和慢速学习状态。

当前观察：

[
o_t=Observe(Environment_t)
]

召回过去：

[
r_t=Recall(o_t,M_t)
]

LLM推演未来：

[
H_t=LLM(o_t,r_t)
]

Runtime选择行动：

[
a_t=Select(H_t,Policy,Approval)
]

环境返回结果：

[
y_t=Environment(a_t)
]

当前事件进入过去：

[
e_t=(o_t,r_t,H_t,a_t,y_t)
]

[
M_{t+1}=Consolidate(M_t,e_t)
]

必须在代码和文档中严格区分：

```text
过去的可验证证据
当前观察
LLM未来假设
Runtime决策
工具行动
实际结果
结果验证
```

## 六、认知片段模型

不要只提供`add_message()`。

系统最小输入单位为`CognitiveFragment`：

```rust
pub struct CognitiveFragment {
    pub id: FragmentId,
    pub namespace_id: NamespaceId,
    pub tenant_id: Option<TenantId>,
    pub user_id: Option<UserId>,
    pub agent_id: Option<AgentId>,
    pub project_id: Option<ProjectId>,
    pub session_id: Option<SessionId>,
    pub trace_id: Option<TraceId>,

    pub kind: FragmentKind,
    pub content: FragmentContent,
    pub source: FragmentSource,
    pub perspective: Perspective,

    pub occurred_at: DateTime<Utc>,
    pub observed_at: DateTime<Utc>,

    pub confidence: f32,
    pub importance: f32,
    pub validity: ValidityStatus,
    pub security_label: SecurityLabel,

    pub evidence: Vec<EvidenceRef>,
    pub metadata: Metadata,
}
```

`FragmentKind`至少包括：

```text
Observation
Claim
Inference
Prediction
Decision
Action
ToolCall
ToolResult
Outcome
Correction
Constraint
Preference
Fact
Failure
Success
Plan
Approval
Rejection
StateTransition
```

`ValidityStatus`至少包括：

```text
Unverified
Verified
Disputed
Superseded
Invalidated
Quarantined
```

必须保存：

* 事件发生时间；
* 系统观察时间；
* 谁说的；
* 谁观察的；
* 来自哪个工具；
* 是否被验证；
* 被什么证据验证；
* 当前是否仍然有效。

## 七、点、线、面模型

### 1. 点 Point

点不是普通Chunk，而是原子认知变化：

```text
事实
决定
约束
状态变化
行为
结果
失败
偏好
纠正
```

每个点必须引用一个或多个原始Fragment。

### 2. 线 Edge

至少支持：

```text
TemporalBefore
TemporalAfter
Causes
CausedBy
Supports
Contradicts
Supersedes
DependsOn
PartOf
SameEntity
CoActivated
PredictedBy
ValidatedBy
DisprovedBy
RecalledBy
UsedInDecision
ExecutedAs
ResultedIn
```

每条线包含：

* 类型；
* 方向；
* 权重；
* 置信度；
* 来源；
* 有效时间；
* 模型版本；
* 是否人工确认；
* 是否工具验证。

实现在线连接更新：

[
w_{ij}^{t+1}
============

clip(w_{ij}^{t}
+\eta a_i a_j
+\alpha Success
-\beta Failure)
]

不得因为共同出现就自动升级为确定因果。

### 3. 面 Surface

认知面不是摘要或标签，而是：

> 一组重叠的点和线形成的、能够描述当前状态、预测后续结果并支持模式补全的动态认知空间。

Surface至少包含：

* 语义中心；
* 成员点；
* 成员边；
* 激活阈值；
* 时间范围；
* 当前状态；
* 历史状态；
* 置信度；
* 边界；
* 来源证据；
* 模型版本；
* 更新版本。

一个点允许同时属于多个Surface。

实现：

* 显式面；
* 动态面；
* 临时任务面；
* 面合并；
* 面分裂；
* 面冻结；
* 面失活；
* 面版本历史；
* 面内冲突检测；
* 增量中心更新；
* 成员权重更新。

## 八、快慢双系统

### 快速情景记忆

`memory-episodic`负责：

* 单次事件快速写入；
* 稀疏索引；
* 时间、人物、地点、项目绑定；
* 原始证据引用；
* 模式分离；
* 相似事件去重但不覆盖；
* 状态版本链；
* 模糊线索召回。

### 慢速语义记忆

`memory-semantic`和`memory-surface`负责：

* 从多次情景中学习稳定规律；
* 从成功和失败中学习程序经验；
* 形成概念、偏好、约束和认知面；
* 保留每个规律对应的证据；
* 避免单次错误污染长期知识；
* 支持撤销、纠正和重新巩固。

不得直接微调外部LLM。

慢速学习参数必须属于疆诚记忆自身。

## 九、模式分离与模式补全

### 模式分离

写入时必须避免高度相似事件互相覆盖：

* 内容哈希；
* 实体差异；
* 时间差异；
* 项目差异；
* 语义距离；
* 来源差异；
* 状态差异；
* 稀疏编码或正交化策略。

### 模式补全

召回时：

1. 通过精确、全文和向量索引定位种子点；
2. 加载局部邻接子图；
3. 根据查询意图选择边类型；
4. 进行有限跳数的扩散激活；
5. 激活相关Surface；
6. 检查问题所需证据是否完整；
7. 缺失时继续局部扩展；
8. 形成最小证据子图；
9. 返回原始证据引用。

禁止在百万节点上执行无界全图传播。

## 十、核心API

完整实现以下接口：

```text
observe
recall
commit
correct
reinforce
weaken
supersede
contradict
forget
explain
snapshot
restore
rebuild
consolidate
health
stats
```

### Observe

接收原始认知片段并返回：

* 创建了哪些点；
* 创建了哪些候选边；
* 激活了哪些面；
* 是否存在冲突；
* 是否进入隔离区。

### Recall

支持模式：

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

### Commit

提交一次完整状态转移：

```text
当前观察
召回了什么
LLM提出哪些假设
Runtime选择什么
执行了什么
结果是什么
哪些预测被验证或否定
```

### Explain

解释：

* 为什么召回；
* 从哪些种子开始；
* 经过哪些边；
* 激活哪些面；
* 最终证据是什么；
* 哪些关系是推断；
* 哪些事实被验证；
* 是否存在缺失证据。

## 十一、Memory Packet

所有SDK统一返回：

```json
{
  "query_id": "query-id",
  "intent": "causal_recall",
  "current_states": [],
  "historical_states": [],
  "activated_surfaces": [],
  "episodes": [],
  "points": [],
  "paths": [],
  "procedures": [],
  "constraints": [],
  "preferences": [],
  "past_failures": [],
  "evidence": [],
  "conflicts": [],
  "missing_evidence": [],
  "coverage": {},
  "confidence": 0.0,
  "recommended_context": ""
}
```

Memory Packet必须是模型无关协议，不得包含某个模型专用Prompt格式。

## 十二、Agent Runtime

`memory-runtime`必须实现轻量但完整的Agent循环：

```text
Observe
Recall
Build Memory Packet
Resolve Capabilities
Invoke Model
Parse Hypotheses
Apply Policy
Request Approval
Execute Tool
Capture Result
Commit Transition
Check Completion
Retry / Pause / Resume
```

提供：

* 模型接口Trait；
* OpenAI兼容接口；
* 通用HTTP模型适配接口；
* 自定义模型Provider接口；
* Tool Registry；
* Skill Registry；
* Prompt Registry；
* MCP Client；
* Policy Engine；
* Approval Manager；
* Pause/Resume；
* WaitingUser；
* 超时、取消、重试；
* Trace；
* Completion Judge。

LLM输出的内容必须区分：

```text
Hypothesis
Prediction
Plan
DecisionCandidate
ToolRequest
FinalResponse
```

Runtime不得把LLM的预测直接写成事实。

## 十三、能力系统

统一定义`Capability`：

```text
Prompt
Skill
Tool
MCPTool
Workflow
Policy
```

每个能力包含：

* 唯一ID；
* 版本；
* 来源；
* 权限；
* 输入输出Schema；
* 风险等级；
* 是否需要审批；
* 执行记录；
* 成功率；
* 失败模式；
* 适用Surface。

系统必须学习：

* 什么任务适合什么Skill；
* 哪个工具组合成功率更高；
* 哪些参数曾经失败；
* 哪些工具需要提前批准；
* 哪些MCP服务不稳定。

这些属于程序记忆，但统计关系不能冒充事实。

## 十四、存储引擎

第一版使用成熟SQLite作为物理存储，不从零实现B-Tree和WAL。

### SQLite

启用：

* WAL；
* 外键；
* Busy Timeout；
* Schema Migration；
* 事务；
* FTS5；
* 完整索引；
* 数据完整性检查。

至少设计以下逻辑表：

```text
fragments
fragment_contents
events
points
edges
surfaces
surface_members
entities
entity_aliases
states
state_versions
evidence_refs
recall_sessions
recall_activations
transitions
hypotheses
decisions
actions
outcomes
capabilities
capability_runs
approvals
model_versions
memory_models
audit_logs
snapshots
quarantine
schema_migrations
```

### BlobStore

实现：

* 内容寻址；
* SHA-256校验；
* 原子写；
* 分片目录；
* 可选压缩；
* 可选AES-GCM加密；
* 损坏检测；
* 垃圾回收；
* 引用计数；
* 恢复工具。

### 派生数据

以下内容必须可从原始事件重建：

```text
Embedding
FTS索引
向量索引
边
Surface成员
摘要
缓存
```

## 十五、神经模型

`memory-neural`定义可插拔接口：

```text
FragmentEncoder
EventClassifier
ImportancePredictor
EdgePredictor
QueryRouter
Reranker
SurfaceAssigner
ContradictionDetector
```

提供：

1. ONNX Runtime实现；
2. 确定性本地Fallback；
3. 模型Manifest；
4. 模型版本；
5. 校验和；
6. 输入输出Schema；
7. 批处理；
8. 超时；
9. 熔断；
10. 设备选择。

Python侧实现：

* PyTorch训练代码；
* 数据加载；
* 训练配置；
* Checkpoint；
* ONNX导出；
* ONNX一致性验证；
* 评测脚本；
* 小型可重复测试模型。

不要提交虚构的“大模型权重”。测试可以使用体积小、许可清晰的模型或确定性Fixture。

## 十六、离线重放与记忆巩固

`memory-consolidation`实现后台Job：

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

必须满足：

* 巩固是增加结构，不是覆盖原始事件；
* 默认采用保守巩固；
* 可暂停；
* 可恢复；
* 可重试；
* 有Checkpoint；
* 有审计；
* 有资源限制；
* 支持增量局部维护；
* 避免每次全库重建。

## 十七、安全与治理

`memory-security`实现：

* Namespace隔离；
* Tenant隔离；
* User/Agent/Project Scope；
* ACL；
* Security Label；
* 加密接口；
* Key Provider接口；
* 审计日志；
* Prompt Injection内容隔离；
* Memory Poisoning检测；
* 敏感数据标记；
* 删除策略；
* Retention Policy；
* Quarantine；
* 导出；
* 彻底删除；
* 派生索引清理；
* 模型反学习或重新巩固任务。

任何召回都必须在检索前和结果返回前执行权限过滤，不能只依靠上层Agent过滤。

## 十八、服务模式

### Embedded

提供Rust原生库和各语言绑定。

### Sidecar

实现：

* Unix Domain Socket；
* Windows Loopback/Named Pipe可扩展接口；
* 本地鉴权Token；
* 单用户本地运行；
* 自动启动；
* 健康检查；
* 优雅关闭；
* 文件锁；
* 崩溃恢复。

### Server

实现：

* HTTP REST；
* gRPC；
* OpenAPI；
* 多租户；
* API Key；
* 请求ID；
* Trace ID；
* 限流；
* 健康检查；
* Metrics；
* 结构化日志；
* 优雅关闭；
* 数据目录配置；
* TLS配置接口。

## 十九、MCP Server

实现标准MCP能力，至少暴露：

```text
memory_observe
memory_recall
memory_commit
memory_correct
memory_explain
memory_forget
memory_stats
```

工具Schema必须完整。

MCP返回值应是结构化Memory Packet，而不是一段无法验证的自然语言。

## 二十、多语言SDK

所有SDK使用统一协议和契约测试。

### Node/TypeScript

* napi-rs嵌入式模式；
* HTTP/gRPC远程模式；
* 完整TypeScript类型；
* Promise异步接口；
* Electron示例；
* npm构建配置；
* 平台包发布配置。

### Python

* PyO3嵌入式模式；
* HTTP/gRPC客户端；
* 类型注解；
* 同步/异步接口；
* pytest；
* wheel构建配置。

### Go

* gRPC/HTTP客户端；
* C ABI嵌入接口；
* Context取消；
* Go类型；
* 示例和测试。

### Kotlin/Android

* UniFFI/JNI绑定；
* Android AAR构建；
* 协程接口；
* 本地数据库初始化；
* 基本Instrumented Test配置。

### Swift/iOS/macOS

* UniFFI绑定；
* XCFramework构建脚本；
* async/await封装；
* Swift Package配置；
* 基本测试。

### Java

* JVM API；
* gRPC客户端；
* Kotlin绑定可复用时避免重复实现；
* Gradle配置；
* 测试。

## 二十一、兼容适配器

完整实现适配器接口和数据映射：

### Hermes

* 实现MemoryProvider；
* 映射`sync_turn`；
* 映射`prefetch`；
* 映射`shutdown`；
* 支持配置；
* 提供安装文档。

### OpenClaw

* Memory插件适配；
* 导入Markdown/Memory文件；
* Hook/MCP接入；
* 保留来源信息。

### Mem0

* 导入导出；
* 映射Add/Search；
* 迁移历史数据；
* 标记旧数据置信度。

### Graphiti

* 导入节点、边、时间有效性；
* 保留Provenance；
* 不把推断边升级为已验证事实。

### Dify

* 提供HTTP/MCP接入；
* 工作流节点示例；
* 用户、会话和项目Scope映射。

外部项目API无法在本地验证时，使用版本化接口、Fixture和契约测试，不能伪造已验证结果。

## 二十二、Benchmark

完整实现Benchmark Harness，不要只放空目录。

### LongMemEval

测试：

* 信息提取；
* 跨会话推理；
* 时间推理；
* 知识更新；
* 拒答。

### LoCoMo

测试：

* 单跳；
* 多跳；
* 时间；
* 因果；
* 长期对话。

### Cross-Year

自行生成可重复数据：

```text
跨1天
跨1月
跨1年
多次状态变更
模糊线索
同名实体
相似事件
稀有关键事实
```

### Model Swap

同一个MemoryDB接入多个模型或确定性模型Fixture，验证：

* 记忆一致；
* 证据一致；
* 当前状态一致；
* 只允许最终语言表达不同。

### Memory Poisoning

测试：

* 恶意提示长期污染；
* 跨租户串联；
* 未验证LLM推断；
* 重复错误信息强化；
* 恶意工具结果；
* 权限绕过。

### Million Fragments

生成至少百万片段的压力测试，测量：

* 写入吞吐；
* 索引耗时；
* 数据大小；
* Recall P50/P95/P99；
* 局部图加载；
* 巩固耗时；
* 内存占用；
* 崩溃恢复。

## 二十三、必须统计的指标

```text
Critical Memory Recall@K
Evidence Coverage
Current State Accuracy
Historical State Accuracy
Temporal Accuracy
Causal Path Accuracy
Conflict Detection Rate
Stale Memory Rate
False Memory Rate
Cross-Tenant Leakage Rate
Source Traceability
Abstention Accuracy
Model Swap Consistency
Write Throughput
Recall P50/P95/P99
Storage Amplification
Consolidation Cost
```

## 二十四、测试体系

必须包括：

* Rust单元测试；
* Rust集成测试；
* 端到端测试；
* 属性测试；
* 并发测试；
* 事务回滚测试；
* 崩溃恢复测试；
* Schema Migration测试；
* Blob损坏测试；
* 权限测试；
* 跨租户隔离测试；
* Prompt Injection测试；
* SDK契约测试；
* gRPC/HTTP一致性测试；
* MCP测试；
* 模型Fallback测试；
* Benchmark Smoke Test；
* Fuzz入口。

质量门禁：

```bash
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo doc --workspace --no-deps
```

并执行各SDK测试。

## 二十五、CI/CD

GitHub Actions至少实现：

* Rust fmt；
* Rust clippy；
* Rust test；
* 文档构建；
* Linux/macOS/Windows矩阵；
* Node SDK测试；
* Python SDK测试；
* Go SDK测试；
* Protobuf兼容检查；
* 安全审计；
* 依赖许可证检查；
* Release构建；
* Artifact上传；
* Benchmark Smoke Test。

发布产物至少规划：

```text
Windows x64
macOS x64
macOS ARM64
Linux x64
Linux ARM64
Node原生包
Python Wheels
Go SDK
Server二进制
MCP二进制
```

移动端产物在CI环境允许时构建；不允许时必须提供完整构建脚本和独立工作流。

## 二十六、文档

必须编写：

```text
项目介绍
五分钟快速开始
完整架构
时间认知模型
点线面模型
快慢双记忆
模式分离与补全
Memory Packet协议
数据模型
安全模型
权限模型
存储格式
嵌入式部署
Sidecar部署
Server部署
Node/Python/Go/Kotlin/Swift/Java SDK
Hermes/OpenClaw适配
模型训练与ONNX导出
Benchmark方法
故障恢复
数据迁移
版本升级
威胁模型
性能调优
```

创建ADR，至少包括：

```text
为什么选择Rust
为什么底层先使用SQLite
为什么LLM不属于记忆主体
为什么原始事件不可变
为什么预测不能成为事实
为什么使用快慢双系统
为什么选择局部传播
为什么派生索引必须可重建
```

## 二十七、实现顺序

虽然最终目标是完整仓库，但仍然按照依赖顺序实施：

1. Monorepo、协议和领域类型；
2. EventLog、SQLite、BlobStore和Migration；
3. Fragment、Point、Edge、Surface；
4. 精确、FTS和向量种子检索；
5. 局部图激活和模式补全；
6. Recall/Explain/Memory Packet；
7. 巩固和快慢双系统；
8. Neural接口、ONNX和Fallback；
9. Security、ACL、审计和隔离；
10. Runtime、模型接口和能力系统；
11. HTTP/gRPC/Sidecar；
12. MCP；
13. Node、Python、Go SDK；
14. Kotlin、Swift、Java SDK；
15. Hermes、OpenClaw、Mem0、Graphiti、Dify适配；
16. Benchmark；
17. CI/CD；
18. 文档、示例和发布检查。

每个阶段必须保持仓库可编译、可测试。

## 二十八、验收场景

最终必须能够运行以下场景：

### 场景一：跨年召回

2018年写入一个项目失败事件，2026年使用不同措辞查询，系统通过局部线索、关联路径和模式补全找回原始证据。

### 场景二：事实更新

先写入：

```text
项目使用PostgreSQL
```

后写入：

```text
项目改用MySQL
```

系统必须分别回答：

```text
当前使用什么？
以前使用什么？
什么时候改变？
为什么改变？
```

### 场景三：预测与事实隔离

LLM预测：

```text
修改后测试应该通过
```

实际工具结果：

```text
测试失败
```

系统必须将预测标记为被否定，不能保存成成功事实。

### 场景四：经验学习

多次记录：

```text
大规模修改失败
小范围兼容改造成功
```

慢速系统形成带证据的经验规律，但不得把统计相关升级为绝对规则。

### 场景五：模型更换

使用同一个MemoryDB依次接入不同模型，召回证据、用户身份、项目状态和过去经历保持一致。

### 场景六：权限隔离

两个租户拥有相似项目和相似问题，任何召回都不得发生跨租户数据泄漏。

### 场景七：崩溃恢复

在写入、Blob提交、索引更新和巩固过程中模拟崩溃，重启后必须恢复一致状态，派生索引允许重建，原始事件不得损坏。

## 二十九、最终交付要求

完成后必须提供：

1. 实际创建的仓库结构；
2. 核心设计说明；
3. 数据库Schema；
4. API和协议说明；
5. 所有可运行命令；
6. 各SDK使用示例；
7. 测试结果；
8. Benchmark结果；
9. 尚未在真实外部平台验证的事项；
10. 已知风险；
11. 下一版本建议。

最终不要只回复“已搭建框架”。

必须确认：

* 哪些模块已经真实实现；
* 哪些测试已经运行；
* 哪些平台已经构建；
* 哪些能力依赖外部环境；
* 是否存在任何占位实现；
* 是否存在未解决的安全或一致性问题。

现在开始：

1. 检查仓库和约束；
2. 给出完整实施计划；
3. 创建并实现疆诚记忆完整仓库；
4. 持续测试和修复；
5. 直到形成可以本地运行、接入任意LLM、注册工具并完成记忆闭环的完整项目。
