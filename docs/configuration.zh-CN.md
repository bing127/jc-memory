# 疆诚记忆 Server 配置参考

`memory-server` 支持内置默认值、YAML 文件和环境变量三种配置来源。优先级从低到高为：

1. 内置默认值；
2. `JC_MEMORY_CONFIG_FILE` 指向的 YAML；
3. 环境变量。

环境变量为空或只包含空白字符时视为未设置，不会覆盖 YAML 值。

## YAML 示例

```yaml
bind: 0.0.0.0:8787
grpc_bind: 0.0.0.0:8788
data_dir: /var/lib/jc-memory
namespace_id: 018f4c8f-5f7b-7d31-9a6c-b7a0f4f37001
max_in_flight: 64
max_requests_per_second: 128
request_timeout_ms: 30000
tls:
  cert_file: /etc/jc-memory/tls/server.crt
  key_file: /etc/jc-memory/tls/server.key
retrieval:
  aliases_file: /etc/jc-memory/retrieval/aliases.zh-CN.yaml
  lexical_limit_factor: 4
  trigram_limit_factor: 4
  entity_limit_factor: 2
  recent_limit: 32
  rrf_k: 60
  max_expanded_terms: 24
  min_trigram_chars: 3
  min_candidate_score: 0.2
security:
  rules_file: /etc/jc-memory/security/quarantine.zh-CN.yaml
sqlite:
  max_connections: 4
  busy_timeout_ms: 5000
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

YAML 禁止未知字段。API Key 不允许写入 YAML，必须由 `JC_MEMORY_API_KEY` 环境变量或部署平台的 Secret 注入。

## 字段映射

| YAML 字段 | 环境变量 | 默认值 | 说明 |
| --- | --- | --- | --- |
| `bind` | `JC_MEMORY_BIND` | `127.0.0.1:8787` | HTTP 监听地址 |
| `grpc_bind` | `JC_MEMORY_GRPC_BIND` | `127.0.0.1:8788` | gRPC 监听地址 |
| `data_dir` | `JC_MEMORY_DATA_DIR` | `./data/jc-memory` | Embedded 数据目录 |
| `namespace_id` | `JC_MEMORY_NAMESPACE_ID` | 无 | API Key 绑定的 Namespace UUID |
| `max_in_flight` | `JC_MEMORY_MAX_IN_FLIGHT` | `64` | HTTP/gRPC 共享的最大并发数，必须大于 0 |
| `max_requests_per_second` | `JC_MEMORY_MAX_REQUESTS_PER_SECOND` | `128` | 每 API Key 每秒请求上限，必须大于 0 |
| `request_timeout_ms` | `JC_MEMORY_REQUEST_TIMEOUT_MS` | `30000` | 单个请求执行 deadline，单位毫秒，必须大于 0 |
| `tls.cert_file` | `JC_MEMORY_TLS_CERT_FILE` | 无 | PEM 证书链文件路径 |
| `tls.key_file` | `JC_MEMORY_TLS_KEY_FILE` | 无 | PEM 私钥文件路径 |
| `retrieval.aliases_file` | `JC_MEMORY_RETRIEVAL_ALIASES_FILE` | 编译内置词典 | 版本化业务别名词典路径；空白环境变量不覆盖 YAML |
| `retrieval.lexical_limit_factor` | 无 | `4` | unicode61 候选预算倍数，范围 1-16 |
| `retrieval.trigram_limit_factor` | 无 | `4` | trigram 候选预算倍数，范围 1-16 |
| `retrieval.entity_limit_factor` | 无 | `2` | 显式实体候选预算倍数，范围 1-8 |
| `retrieval.recent_limit` | 无 | `32` | 近期安全网绝对上限，范围 1-100，仍受 `max_nodes` 派生预算限制 |
| `retrieval.rrf_k` | 无 | `60` | Reciprocal Rank Fusion 的固定 k，范围 1-10000 |
| `retrieval.max_expanded_terms` | 无 | `24` | 单次查询最大业务别名扩展数，范围 1-128 |
| `retrieval.min_trigram_chars` | 无 | `3` | 与 SQLite trigram Tokenizer 一致，当前版本必须为 3 |
| `retrieval.min_candidate_score` | 无 | `0.2` | 近期候选进入融合前的确定性相关性阈值，范围 `(0, 1]` |
| `security.rules_file` | `JC_MEMORY_SECURITY_RULES_FILE` | 编译内置策略 | 版本化 Quarantine YAML；读取或校验失败直接阻止启动 |
| `sqlite.max_connections` | `JC_MEMORY_SQLITE_MAX_CONNECTIONS` | `4` | 两个 SQLite 使用点共享的连接池上限，范围 `1..=64` |
| `sqlite.busy_timeout_ms` | `JC_MEMORY_SQLITE_BUSY_TIMEOUT_MS` | `5000` | SQLite 忙等待时间，范围 `1..=60000` |
| `runtime.provider.max_retries` | `JC_MEMORY_RUNTIME_PROVIDER_MAX_RETRIES` | `1` | Provider 额外重试次数，范围 `0..=10` |
| `runtime.provider.retry_backoff_ms` | `JC_MEMORY_RUNTIME_PROVIDER_RETRY_BACKOFF_MS` | `100` | Provider 固定退避时间，范围 `0..=60000` |
| `runtime.recall.max_seeds` | `JC_MEMORY_RUNTIME_RECALL_MAX_SEEDS` | `8` | Runtime Recall Seed 上限，范围 `1..=64` |
| `runtime.recall.max_neighbors` | `JC_MEMORY_RUNTIME_RECALL_MAX_NEIGHBORS` | `8` | Runtime Recall 邻居上限，范围 `1..=128` |
| `runtime.recall.max_hops` | `JC_MEMORY_RUNTIME_RECALL_MAX_HOPS` | `3` | Runtime Recall 深度上限，范围 `1..=16` |
| `runtime.recall.max_nodes` | `JC_MEMORY_RUNTIME_RECALL_MAX_NODES` | `64` | Runtime Recall 节点上限，范围 `1..=1024` |
| `runtime.recall.timeout_ms` | `JC_MEMORY_RUNTIME_RECALL_TIMEOUT_MS` | `2000` | Runtime Recall deadline，范围 `1..=60000` |
| 无 | `JC_MEMORY_API_KEY` | 无 | 服务端 API Key，格式为 `sk-jcmemory-` 加 34 位小写字母或数字，必须通过 Secret 注入 |
| 无 | `JC_MEMORY_RUNTIME_PROVIDER_ENDPOINT` | 无 | OpenAI Compatible Chat Completions 地址，与 MODEL 成对配置 |
| 无 | `JC_MEMORY_RUNTIME_PROVIDER_MODEL` | 无 | Runtime 模型标识，与 ENDPOINT 成对配置 |
| 无 | `JC_MEMORY_RUNTIME_PROVIDER_API_KEY` | 无 | 可选 Bearer Token，只通过 Secret 注入 |
| 无 | `JC_MEMORY_CONFIG_FILE` | 无 | YAML 配置文件路径 |

`namespace_id` 和 `JC_MEMORY_API_KEY` 是必填项。`JC_MEMORY_API_KEY` 必须匹配 `^sk-jcmemory-[a-z0-9]{34}$`。TLS 证书与私钥必须成对配置；两者都未配置时使用本机明文 gRPC 开发模式。

检索词典在进程启动时一次性读取，严格校验 `schema_version`、词典版本、concept、term 数量和未知字段，并计算 SHA-256。文件不存在或 Schema 不兼容会直接阻止启动，不会静默退回另一套词典。业务别名只扩展查询概念，不写入或修改 Fragment、Event、EvidenceRef。

Quarantine 规则从 `config/security/quarantine.zh-CN.yaml` 编译内置，也可以通过 `security.rules_file` 或环境变量显式指定。规则词项会规范化并拒绝空值、重复值、超长值、未知字段、非有限分数和越界阈值；风险评估会记录策略版本与 SHA-256 摘要，但不会记录完整规则正文。外部文件读取或校验失败时 Engine 直接启动失败，不会静默回退。

Docker 镜像中的规则路径固定为 `/etc/jc-memory/retrieval/aliases.zh-CN.yaml` 和 `/etc/jc-memory/security/quarantine.zh-CN.yaml`；Kubernetes 由同一个 ConfigMap 挂载非 Secret 规则文件，由 Secret 注入 `JC_MEMORY_NAMESPACE_ID` 与 `JC_MEMORY_API_KEY`。`JC_MEMORY_SIDECAR_DAEMON_BIN` 仅用于 Sidecar 控制端选择守护进程可执行文件，默认使用 `jcmemd`，不改变协议字段或命令名称。

## 本地启动

```bash
export JC_MEMORY_CONFIG_FILE=./server.yaml
export JC_MEMORY_API_KEY="sk-jcmemory-$(openssl rand -hex 17)"
cargo run -p jc-memory-server
```

环境变量可覆盖 YAML 中的单个字段：

```bash
export JC_MEMORY_CONFIG_FILE=./server.yaml
export JC_MEMORY_API_KEY="sk-jcmemory-$(openssl rand -hex 17)"
export JC_MEMORY_BIND=127.0.0.1:9787
cargo run -p jc-memory-server
```

## Secret 与文件权限

- 不要把 API Key、证书私钥或可用 Token 提交到 Git。
- Runtime Provider Token 只从环境变量读取，不写入 YAML、HTTP 响应或 Web 控制面数据库。
- 生产环境 API Key 应由 Kubernetes Secret、容器运行时 Secret 或进程管理器注入。
- TLS 私钥文件应只允许服务进程用户读取；Unix 环境建议权限为 `0600`。
- 日志和错误报告只应包含配置字段名或文件路径，不应包含 API Key 或私钥内容。
