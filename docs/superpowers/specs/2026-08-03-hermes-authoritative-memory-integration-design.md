# Hermes 全量记忆权威接入设计规格

- **状态：** 已确认
- **日期：** 2026-08-03
- **目标宿主：** `NousResearch/hermes-agent` 默认分支
- **记忆系统：** 疆诚记忆（Jiangcheng Memory）
- **接入方式：** 独立 `jc-memory-server` + Hermes Python HTTP Provider

## 1. 目标

本设计通过真实 Hermes 运行时验证疆诚记忆能否成为 Agent 的唯一持久化记忆与召回权威。测试不以 Hermes Fixture、独立 Recall 脚本、模拟模型或只验证 HTTP 200 作为完成条件。

接入完成后：

1. Hermes 产生和使用的所有可持久化记忆均写入疆诚记忆；
2. Hermes 原有持久化记忆、长期召回和本地记忆数据库不再参与运行；
3. Hermes 重启、新建 Session 或清除 Hermes 本地记忆后，仍能从疆诚记忆恢复并召回；
4. 每条召回内容都能追溯到原始 Fragment、Event 和 EvidenceRef；
5. 模型输出、Prediction、Inference、Plan 和 ToolResult 不得自动提升为 Verified Fact。

## 2. 权威边界

疆诚记忆管理以下全部可持久化内容：

- 用户消息与助手回复；
- Tool Call、Tool Result 和工具错误；
- Observation、环境信息与文件相关观察；
- Session 摘要、上下文压缩结果与恢复检查点；
- 用户偏好、事实、经验、Claim、Prediction、Inference 和 Plan；
- Hermes 实例、用户、Agent、Workspace、Session 和 Turn 的关联；
- 纠错、遗忘、时间有效性、可信状态和来源证据；
- 跨 Session 召回与历史会话恢复所需的顺序和引用关系。

Hermes 只保留当前正在执行的 Turn 所必需的进程内对象，包括流式输出缓冲、尚未结束的工具句柄和未完成的消息对象。这些对象不得落入另一套持久化记忆数据库；Turn 完成后必须提交到疆诚记忆。

以下内容不属于记忆，不迁入疆诚记忆：

- 模型 API Key 和其他 Secret；
- Hermes 程序配置、模型目录与可执行文件；
- Python 包缓存、日志轮转文件和构建产物；
- 仅用于运行的网络连接与进程句柄。

## 3. 仓库与所有权

官方 Hermes 克隆到：

```text
external/hermes-agent/
```

该目录加入 `jc-memory/.gitignore`，并保持独立 Git 仓库。克隆后记录远端 URL、默认分支和精确 Commit SHA，真实验收报告必须引用该 SHA。

可维护的集成资产保存在 `jc-memory`：

```text
integrations/hermes/
  README.zh-CN.md
  config.example.yaml
  python/
  patches/
  scripts/
  tests/
```

Python Provider、配置解析、契约测试、启动脚本和对 Hermes 的最小 Patch 都由 `jc-memory` 版本控制。不得只修改被忽略的 Hermes 工作树而不保留可重复应用的 Patch。

## 4. 总体架构

```text
Hermes CLI / Gateway
        |
        | 1. 创建或恢复 Session
        v
JcMemorySessionStore -------> jc-memory-server -------> SQLite WAL + BlobStore
        |
        | 2. 模型调用前 Recall
        v
MemoryPacket 注入层
        |
        | 3. 模型与工具执行
        v
Turn Recorder
        |
        | 4. 原子 Commit 消息、输出、工具结果和关系
        v
jc-memory-server
```

浏览器、Hermes 和集成脚本不得直接访问 SQLite。所有读写通过受 API Key、Scope 和 Operation 约束的远程协议进入同一个 `CommandBus`。

## 5. Hermes 接入点

克隆后必须从当前 Hermes Commit 的真实调用链确认并覆盖以下接入点：

1. **Session 创建：** 为 Hermes Session 建立稳定 Scope 和外部标识；
2. **Session 恢复：** 从疆诚记忆按 Session Scope、时间和序号恢复原始消息及必要派生物；
3. **用户消息接收：** 在进入模型链路前保存原始用户输入；
4. **Prompt 构造：** 在模型调用前执行 Recall，并将 MemoryPacket 作为非指令、带来源的上下文注入；
5. **模型输出完成：** 保存原始响应、结构化输出和流式完成状态；
6. **工具执行完成：** 保存 Tool Call、Tool Result、成功状态和错误，不把结果自动认定为事实；
7. **上下文压缩：** 保存原始输入引用、摘要结果和算法版本，摘要不得覆盖原始记录；
8. **Turn 完成：** 原子提交本 Turn 的 Fragment/Event 及顺序关系；
9. **进程关闭：** 拒绝新 Turn，等待在途提交完成，但不由单个 Hermes 进程关闭共享 `jc-memory-server`。

若 Hermes 当前版本提供稳定 Provider、Hook 或 Plugin 接口，优先使用公开扩展点；否则维护最小源代码 Patch。不得通过会话结束后扫描日志的方式冒充实时接入。

## 6. 数据映射

| Hermes 内容 | 疆诚记忆类型 | 初始有效性 | 来源要求 |
| --- | --- | --- | --- |
| 用户消息 | Observation 或 Claim | Unverified | 保留 user/session/turn/message ID |
| 助手最终回复 | Claim | Unverified | 保留 model、session、turn 和原始文本 |
| 模型推断 | Inference | Unverified | 保留模型与推断来源 |
| 模型预测 | Prediction | Unverified | 自报置信度仅存 metadata |
| 模型计划 | Plan | Unverified | 保留目标和步骤，不升级为事实 |
| Tool Call | ToolCall | Unverified | 保留工具名、参数摘要和调用 ID |
| Tool Result | ToolResult | Unverified | 保留调用 ID、成功状态和原始结果引用 |
| 会话摘要 | Summary 派生物 | Unverified | 必须引用全部原始输入 EvidenceRef |
| 用户确认或外部证据 | 对原记录的验证 Event | 按验证规则处理 | 必须保留确认者与 EvidenceRef |

大文本、二进制工具结果或附件使用 BlobStore，Fragment 保存内容摘要、媒体类型、哈希和 Blob 引用。任何 Embedding、Point、Edge、Surface 或摘要都只能作为可重建派生物。

## 7. Scope 与身份映射

Scope 至少使用以下稳定维度：

```text
namespace = Hermes 部署实例
tenant    = Hermes 用户或组织
agent     = Hermes Agent 标识
project   = Workspace 或项目稳定标识
session   = Hermes Session 标识（保存于 Fragment metadata）
turn      = Hermes Turn 标识（保存于 Fragment metadata）
```

不存在稳定用户身份时，不得把不同本机用户自动合并到同一 Tenant。Workspace 路径必须规范化后映射为稳定 Project ID，避免目录别名造成跨项目串记忆。

## 8. Recall 与 Prompt 注入

模型调用前使用当前用户输入、Session、Agent 和 Workspace Scope 执行有界 Recall。返回的 MemoryPacket：

- 使用独立的 memory context 区域注入；
- 明确标记为历史证据而非 System 指令；
- 保留 EvidenceRef、发生时间、有效时间和可信状态；
- 受 Token、节点数、边数、时间窗口和权限硬限制约束；
- 不得把 Unverified 内容渲染成已确认事实；
- 不得因摘要命中而丢失回查原始 Fragment 的能力。

Hermes 原有长期记忆检索和注入必须关闭。验收期间若发现第二条长期记忆注入链路，测试立即判定无效，修复后重新开始。

## 9. 一致性与失败策略

疆诚记忆作为唯一持久化权威时，禁止静默降级到 Hermes 本地持久化。

- Session 创建或恢复失败：拒绝启动该 Session，并显示稳定错误；
- 用户消息持久化失败：不得调用模型；
- Recall 失败：默认不得继续生成可能缺失关键历史的回答；手动测试模式允许显式选择“无记忆继续”，并记录降级 Event；
- Turn Commit 失败：保留有界内存重试队列，禁止把该 Turn 标记为已持久化；进程退出前仍失败则返回明确失败；
- 服务恢复：使用稳定 idempotency key 重试，不产生重复 Fragment；
- Scope 或授权失败：禁止回退到更宽 Scope；
- Hermes 进程崩溃：只恢复已经由服务确认的记录，未确认的内存缓冲不得伪装成已保存。

集成配置提供连接地址、API Key 环境变量名、Namespace、超时、Recall 限制、失败模式和 Hermes 原生长期记忆禁用开关。示例 YAML 只保存变量引用，不保存真实 Secret。

## 10. 可观测性

每次真实 Turn 至少产生以下可关联标识：

- Hermes Session ID；
- Hermes Turn ID；
- Hermes Message 或 Tool Call ID；
- `jc-memory` request ID；
- Fragment ID、Event ID 和 EvidenceRef；
- Commit 状态与 Recall 延迟。

日志不得输出 API Key、完整敏感 Prompt 或未脱敏 Tool Result。Web 控制台应能按 Session/Turn 定位写入、Recall、失败和重试，但控制台展示不得成为权威数据。

## 11. TDD 与测试层次

行为变更严格执行 RED-GREEN-REFACTOR：先让真实目标行为测试因缺少 Provider 或 Hook 失败，再实现最小代码。

测试分为四层：

1. **Python 单元测试：** 配置、Scope、消息映射、Prompt 注入、错误映射和 Secret 脱敏；
2. **协议契约测试：** 使用真实 `jc-memory-server` 和临时 SQLite，验证 Observe/Commit/Recall/Correct/Forget；
3. **Hermes 运行时 E2E：** 运行克隆的真实 Hermes 入口，经过真实 Session、模型调用前 Hook、工具结果回调和关闭生命周期；
4. **手动验收：** 用户自行配置模型 Key，通过 Hermes CLI 或官方 UI 完成跨 Session 对话。

Fixture 只验证协议兼容，不得标记为 Hermes 实机通过。没有用户模型 Key 时，自动化测试可以验证真实 Hermes 调用链到模型边界，但不能描述为真实模型对话完成。

## 12. 手动验收场景

### 12.1 全量写入

在 Session A 中输入一个唯一事实和一个项目偏好，要求 Hermes 调用至少一个工具。控制台必须看到用户消息、助手回复、Tool Call、Tool Result、Session/Turn 关系和原始 EvidenceRef。

### 12.2 删除本地状态后恢复

完全退出 Hermes，备份后移走 Hermes 原有记忆数据库和会话持久化目录，再启动 Session B。Hermes 必须仅通过疆诚记忆恢复 Session A 的历史并回答唯一事实。

### 12.3 跨 Session Recall

Session B 不携带 Session A 的 Prompt 或本地聊天记录。回答必须实际使用 Recall 返回的 MemoryPacket，并能从回答关联到 Session A 的原始 EvidenceRef。

### 12.4 纠错与时间

在新 Turn 中纠正唯一事实。后续当前状态 Recall 必须优先返回有效新值，历史查询仍能看到旧值及纠错 Event。

### 12.5 隔离

另一个 Tenant 或 Workspace 使用相同查询不得召回 Session A 内容。测试同时覆盖直接 HTTP 请求，证明不能绕过 Hermes 前端限制。

### 12.6 故障与恢复

在 Recall 和 Commit 阶段分别停止 `jc-memory-server`，确认 Hermes 不写本地替代记忆、不静默继续，并在服务恢复后使用同一 idempotency key 正确重试。

### 12.7 性能

记录真实 Hermes 端到端首 Token 延迟、Recall P50/P95/P99、Commit P50/P95/P99、失败率和重复写入数。性能测试后再次执行功能 Recall 和 EvidenceRef 校验，防止只看吞吐而忽略记忆正确性。

## 13. 完成门槛

只有同时满足以下条件，才能说明疆诚记忆对 Hermes 的全量记忆接入可用：

- 官方 Hermes 精确 Commit SHA 已记录且真实入口可运行；
- Hermes 原有持久化记忆和长期 Recall 已确认关闭；
- 所有可持久化记忆类型均通过疆诚记忆读写；
- 删除 Hermes 本地记忆后仍能恢复和跨 Session 召回；
- Recall 内容可追溯到原始 Fragment/Event/EvidenceRef；
- Prediction、Inference、Plan 和 ToolResult 未被错误升级为 Verified Fact；
- Tenant、Agent 和 Workspace 隔离通过；
- Correct、Forget、时间有效性和幂等重试通过；
- 服务故障不会产生隐蔽的第二份本地权威；
- 自动化测试、真实 Hermes E2E 和用户手动模型对话分别记录结果；
- 压测后功能正确性、证据完整性和数据计数仍保持一致。

如果只完成 Adapter Fixture、独立服务压测、Mock 模型或 HTTP 契约测试，状态必须标记为部分验证，不得描述为 Hermes 全量记忆接入完成。
