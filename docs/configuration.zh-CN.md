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
tls:
  cert_file: /etc/jc-memory/tls/server.crt
  key_file: /etc/jc-memory/tls/server.key
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
| `tls.cert_file` | `JC_MEMORY_TLS_CERT_FILE` | 无 | PEM 证书链文件路径 |
| `tls.key_file` | `JC_MEMORY_TLS_KEY_FILE` | 无 | PEM 私钥文件路径 |
| 无 | `JC_MEMORY_API_KEY` | 无 | 服务端 API Key，必须通过 Secret 注入 |
| 无 | `JC_MEMORY_CONFIG_FILE` | 无 | YAML 配置文件路径 |

`namespace_id` 和 `JC_MEMORY_API_KEY` 是必填项。TLS 证书与私钥必须成对配置；两者都未配置时使用本机明文 gRPC 开发模式。

## 本地启动

```bash
export JC_MEMORY_CONFIG_FILE=./server.yaml
export JC_MEMORY_API_KEY='replace-with-a-random-secret'
cargo run -p jc-memory-server
```

环境变量可覆盖 YAML 中的单个字段：

```bash
export JC_MEMORY_CONFIG_FILE=./server.yaml
export JC_MEMORY_API_KEY='replace-with-a-random-secret'
export JC_MEMORY_BIND=127.0.0.1:9787
cargo run -p jc-memory-server
```

## Secret 与文件权限

- 不要把 API Key、证书私钥或可用 Token 提交到 Git。
- 生产环境 API Key 应由 Kubernetes Secret、容器运行时 Secret 或进程管理器注入。
- TLS 私钥文件应只允许服务进程用户读取；Unix 环境建议权限为 `0600`。
- 日志和错误报告只应包含配置字段名或文件路径，不应包含 API Key 或私钥内容。
