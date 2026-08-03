# 疆诚记忆配置硬编码治理设计

- **状态：** 已确认
- **日期：** 2026-08-03
- **范围：** Server、Core、Security、Neural Fallback、Web BFF、Hermes 集成与部署清单

## 1. 背景

当前 Server 已支持内置默认值、YAML 和环境变量覆盖，零模型业务别名也已经使用版本化 YAML。不过只读配置审计仍发现以下问题：

1. Docker 配置引用了运行镜像中不存在的外部别名文件，导致容器启动失败；
2. Kubernetes ConfigMap 固定了 Namespace ID；
3. Quarantine 安全检测词项、风险分数和隔离阈值编译在代码中；
4. SQLite、Runtime Provider、Runtime Recall 和 Web BFF 的运行策略分散在代码中；
5. Neural Fallback 的中英文路由词项只能通过重新编译修改；
6. Hermes 配置只做类型转换，没有完整字段和范围校验；
7. Hermes 上游 Commit 同时存在于锁文件和 Provider 常量中，缺少一致性门禁。

这些问题不改变 Fragment、Event 和 EvidenceRef 的权威性，但会影响部署可重复性、安全规则迭代、运行调优和来源元数据可信度。

## 2. 目标与非目标

本次治理目标：

- 修复 Docker 和 Kubernetes 的可部署配置；
- 统一使用“代码级安全上限、内置默认值、严格 YAML、环境变量覆盖”的配置模型；
- 把可运营规则与参数从业务代码移到版本化配置；
- 所有外部规则在启动阶段校验，错误时失败关闭；
- Secret 继续只由环境变量或部署平台 Secret 注入；
- 保持现有无配置启动方式和公开 API 兼容；
- 用 TDD 和契约测试固定配置优先级、边界和部署文件关系。

本次不建设远程配置中心、热更新、管理控制台编辑器、动态租户策略或自动发布系统。规则和运行参数只在进程启动时读取，修改后需要受控重启。

## 3. 配置原则

所有配置分为三类：

| 类型 | 示例 | 处理方式 |
| --- | --- | --- |
| 协议与格式不变量 | API 版本、文件 Magic、Nonce 长度、枚举名称 | 保留在代码中，不允许外部覆盖 |
| 资源绝对上限 | 最大查询字符数、最大词项数、最大 Recall 节点数 | 保留代码级上限，配置只能在范围内收紧或选择 |
| 运行与业务策略 | 连接池、超时、重试、采样周期、检测词项、风险阈值 | 提供内置默认值，并允许严格 YAML 或环境变量覆盖 |

Server 配置优先级保持：

```text
代码内置默认值 < JC_MEMORY_CONFIG_FILE 指向的 YAML < 环境变量
```

空白环境变量继续视为未设置。YAML 使用 `deny_unknown_fields` 等价的严格解析；未知字段、类型错误、非有限浮点数、越界数值、缺失配对字段或外部文件不可读都必须阻止启动。

## 4. Server 配置结构

Server YAML 在现有字段之外增加以下配置段：

```yaml
sqlite:
  max_connections: 4
  busy_timeout_ms: 5000

security:
  rules_file: /etc/jc-memory/security/quarantine.zh-CN.yaml

runtime:
  provider:
    max_retries: 1
    retry_backoff_ms: 100
  recall:
    max_seeds: 8
    max_neighbors: 8
    max_hops: 3
    max_nodes: 64
    timeout_ms: 2000
```

对应环境变量：

- `JC_MEMORY_SQLITE_MAX_CONNECTIONS`；
- `JC_MEMORY_SQLITE_BUSY_TIMEOUT_MS`；
- `JC_MEMORY_SECURITY_RULES_FILE`；
- `JC_MEMORY_RUNTIME_PROVIDER_MAX_RETRIES`；
- `JC_MEMORY_RUNTIME_PROVIDER_RETRY_BACKOFF_MS`；
- `JC_MEMORY_RUNTIME_RECALL_MAX_SEEDS`；
- `JC_MEMORY_RUNTIME_RECALL_MAX_NEIGHBORS`；
- `JC_MEMORY_RUNTIME_RECALL_MAX_HOPS`；
- `JC_MEMORY_RUNTIME_RECALL_MAX_NODES`；
- `JC_MEMORY_RUNTIME_RECALL_TIMEOUT_MS`。

Provider Endpoint、Model 和 API Key 继续使用现有环境变量。API Key 不进入 YAML。`MemoryConfig` 接收 SQLite 与 Security 配置，`MemoryEngine` 和 Runtime Read Store 使用同一组 SQLite 参数，避免同一进程内两个连接池采用不同策略。

Runtime Recall 配置只作用于 Runtime 模型编排内部的 Recall，不改变客户端显式提交的 Recall Limits。所有值必须符合协议和 Core 的代码级上限。

## 5. 版本化安全规则

新增 `config/security/quarantine.zh-CN.yaml`，使用严格 Schema：

```yaml
schema_version: jcmem.security-quarantine.v1
policy_version: 2026-08-03.1
quarantine_threshold: 0.8
repeated_disputed_threshold: 2
detections:
  prompt_injection:
    score: 0.95
    instruction_override: []
    hidden_prompt_access: []
    disclosure_request: []
  cross_scope_reference:
    score: 1.0
  suspicious_tool_result:
    score: 0.9
    self_verified: []
    self_authorized: []
  sensitive_data:
    score: 0.9
    markers: []
```

规则文件需要校验：

- Schema 和策略版本非空且受支持；
- 所有风险分数与隔离阈值是有限的 `(0, 1]`；
- 重复争议阈值大于零且有代码级上限；
- 每组词项非空、去重、规范化，并受数量和字符长度上限约束；
- 未知字段被拒绝；
- 加载后计算原始 YAML 的 SHA-256。

`QuarantinePipeline` 持有不可变策略，风险评估记录策略版本与摘要，但不把完整规则或敏感正文写入日志。没有配置外部文件时使用编译内置的同一版本化 YAML；配置了外部文件而加载失败时不得静默回退。

规则只决定准入状态，不删除、摘要或改写原始 Fragment、Event 和 EvidenceRef。模型来源以及 Prediction、Inference 和 Plan 的强制 Unverified 边界不允许通过配置关闭。

## 6. Neural Fallback 路由规则

新增 `config/neural/fallback-routing.zh-CN.yaml`，包含版本、Causal、CurrentState 和 Procedural 的多语言触发词。`DeterministicFallback` 提供从严格 YAML 构造的公开入口，并保留编译内置默认规则。

该规则只影响 Neural Fallback 的模式选择，不改变当前 Server 零模型检索路径。词项为空、重复、过长、未知 Mode 或 Schema 不兼容时拒绝加载。模型 Manifest 需要记录路由规则版本与 SHA-256，使结果能够定位到实际规则版本。

## 7. Web BFF 配置

Web 环境配置增加：

- `JC_WEB_UPSTREAM_TIMEOUT_MS`，默认 `30000`；
- `JC_WEB_STREAM_OVERVIEW_INTERVAL_MS`，默认 `15000`；
- `JC_WEB_STREAM_METRICS_INTERVAL_MS`，默认 `1000`；
- `JC_WEB_STREAM_RUNTIME_LIST_INTERVAL_MS`，默认 `10000`；
- `JC_WEB_STREAM_RUNTIME_DETAIL_INTERVAL_MS`，默认 `5000`。

所有值必须是正整数，并设置有限上限。BFF 启动时完成校验，将上游超时注入 `UpstreamClient`，将采样周期注入单实例 WebSocket 调度器。浏览器仍只维持一条实例 WebSocket；本次配置化不会恢复前端周期 HTTP 轮询。

## 8. 部署治理

Docker 运行镜像必须包含内置版本化规则文件，并在 Server YAML 使用绝对容器路径。Compose 同时允许用户以只读 Volume 替换规则文件。镜像不得依赖构建上下文路径或进程当前目录。

Kubernetes ConfigMap 不再保存固定 Namespace ID。Deployment 从 `jc-memory-server-secret` 读取 `JC_MEMORY_NAMESPACE_ID` 和 `JC_MEMORY_API_KEY`；示例 Secret 使用明确的替换占位值。Namespace ID 虽不是密码，但属于部署身份，必须由每套环境显式提供。

部署契约测试必须证明：

- Docker Server YAML 引用的每个外部配置文件都被复制或挂载；
- Kubernetes ConfigMap 不包含固定 Namespace；
- Deployment 对必填 Namespace 和 API Key 都使用 Secret 引用；
- 示例配置可以被当前 Server Schema 严格解析。

这些测试只证明仓库部署契约，不描述为真实 Docker、Kubernetes 集群或生产 Ingress 实机验证。

## 9. Hermes 配置治理

Hermes Provider 在初始化阶段严格校验：

- `base_url` 是合法 URL，不包含凭据；远程地址必须使用 HTTPS，Loopback 可使用 HTTP；
- `timeout_seconds` 是有限正数并受上限约束；
- `recall` 只能包含并且必须包含 `max_seeds`、`max_neighbors`、`max_hops`、`max_nodes`、`timeout_ms`；
- 每个 Recall 值满足协议范围，布尔值不能被当作整数；
- 未知 `memory.jc_memory` 字段被拒绝；
- API Key 继续只从环境变量读取且不进入对象 `repr`。

测试读取 `hermes.lock.yaml`，要求 Provider 使用的 Commit 与锁文件一致。Bootstrap 继续核对外部 Hermes 仓库实际 HEAD；三者不一致时验收失败，不允许写入虚假的来源 Commit。

## 10. 文档与兼容性

更新中文配置、部署、Web、Sidecar 和 Hermes 文档，列出新增字段、默认值、覆盖方式和安全边界。补充 `JC_MEMORY_SIDECAR_DAEMON_BIN` 的用途，消除代码读取但文档未声明的配置。

兼容性约束：

- 未提供新配置时保持当前默认运行参数；
- 现有顶层 YAML 字段和环境变量继续有效；
- API、Protobuf、SDK 和持久化 Schema 不因配置治理发生破坏性变化；
- 内置规则保持当前行为，外部规则只在显式配置时生效；
- 配置摘要和版本可以进入 Debug/指标，Secret、完整规则词项和原始输入不得进入日志。

## 11. 测试策略

所有行为变更执行 RED-GREEN-REFACTOR：

1. Security 单元测试先证明内置规则、外部规则、未知字段、越界值和摘要行为缺失；
2. Core 测试先证明外部安全规则没有进入真实 Observe/Commit 准入路径；
3. Server 配置测试先证明 SQLite、Runtime 和 Security 字段无法解析或覆盖；
4. Docker/Kubernetes 契约测试先复现缺失文件和固定 Namespace；
5. Web Backend 测试先证明上游超时和采样周期无法从配置注入；
6. Neural 测试先证明路由词项无法外部替换；
7. Hermes 测试先证明非法数值、未知字段和锁版本漂移未在启动阶段失败。

聚焦测试通过后，运行适用的 Rust fmt、Clippy、Workspace test、rustdoc、Web test/typecheck/build、Hermes pytest 和 `git diff --check`。外部平台不可用时只记录静态部署契约结果。

## 12. 完成标准

- Docker 配置引用的规则文件在运行镜像中真实存在；
- Kubernetes 不再包含可直接使用的固定 Namespace ID；
- Quarantine 与 Neural Fallback 业务词项均来自版本化严格配置，并有内置默认值；
- SQLite、Runtime 和 Web 运行策略可通过受校验配置覆盖；
- Hermes 在发出请求前拒绝无效配置，并验证来源 Commit 一致性；
- 不存在代码读取但中文文档未声明的生产配置；
- Secret 扫描未发现真实凭据或日志泄漏路径；
- 原始 Fragment、Event 和 EvidenceRef 未被任何配置或派生规则覆盖；
- 全部适用门禁有本轮新鲜输出，未执行的外部实机验证明确保留边界。
