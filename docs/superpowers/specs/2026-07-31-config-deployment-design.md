# 疆诚记忆配置与部署设计

## 目标

为 `memory-server` 提供可审查的 YAML 配置、环境变量覆盖和可直接改造的 Docker/Kubernetes 部署资源，同时保持现有环境变量启动方式兼容。

## 配置边界

配置只描述服务运行参数，不改变 Core 的权威记录语义。配置字段包含：

- HTTP 和 gRPC 监听地址；
- Embedded 数据目录；
- API Key 绑定的 Namespace；
- 最大并发数和每 API Key 每秒速率上限；
- gRPC TLS 证书链和私钥文件路径。

API Key 是秘密，不写入 YAML 示例、ConfigMap、Docker 镜像或 Git 仓库。部署时由环境变量或 Kubernetes Secret 注入。

## 来源与优先级

来源优先级从低到高为：

1. 代码默认值；
2. `JC_MEMORY_CONFIG_FILE` 指向的 YAML 文件；
3. 同名环境变量。

没有配置文件时继续使用当前环境变量模式。配置文件不存在、YAML 无法解析、字段类型错误或 TLS 文件只配置一半时，启动失败并指出字段名；私钥内容不进入错误和日志。

环境变量采用现有名称，并新增 `JC_MEMORY_MAX_REQUESTS_PER_SECOND`。YAML 使用 snake_case 字段名，例如 `grpc_bind`、`data_dir`、`max_in_flight` 和 `max_requests_per_second`。

## 合并模型

先将 YAML 解析为可选字段结构，再逐字段应用环境变量覆盖，最后填充默认值并做统一校验。环境变量的空白值视为未设置，不会覆盖有效 YAML 值。合并逻辑接受抽象键值查找器，以便在不修改进程环境的情况下进行单元测试。

## 部署资源

新增以下版本化资源：

- `deploy/docker/Dockerfile`：多阶段构建 Rust 二进制，运行阶段使用非 root 用户和持久化数据目录；
- `deploy/docker/compose.yaml`：暴露 HTTP/gRPC 端口，挂载配置和数据目录，使用环境变量注入 API Key；
- `deploy/kubernetes/configmap.yaml`：非秘密 YAML 配置示例；
- `deploy/kubernetes/secret.example.yaml`：只包含 Secret 字段模板和替换说明，不包含真实值；
- `deploy/kubernetes/deployment.yaml`：ConfigMap/Secret 挂载、资源限制、非 root 安全上下文、HTTP 健康探针；
- `deploy/kubernetes/service.yaml`：分别暴露 HTTP 和 gRPC 服务端口；
- `docs/deployment.zh-CN.md`：本地、Docker Compose、Kubernetes 的配置和验证流程。

Kubernetes 探针使用 `/healthz` 和 `/readyz`；TLS 私钥不放进 ConfigMap。示例资源不宣称已连接真实集群，验证以 YAML 结构检查和本地容器构建为准。

## 测试与验收

- 配置 RED/GREEN 测试覆盖默认值、YAML 解析、环境变量覆盖、空白环境变量、未知字段、类型错误、TLS 成对校验和零值拒绝；
- Server 协议测试确认配置生成的速率限制同时作用于 HTTP 和 gRPC；
- `cargo fmt`、聚焦 `cargo test`、严格 `cargo clippy`、`cargo doc` 和 `git diff --check` 必须通过；
- 使用 `docker build` 或版本化构建 Fixture 验证 Dockerfile 语法；使用 `kubectl apply --dry-run=client`（可用时）或 YAML 结构检查验证 Kubernetes 文件，不把 Fixture 结果描述为实机验证。

## 非目标

本设计不引入命令行参数解析、不实现远程配置中心、不把 Secret 写入 YAML、不改变 Protobuf 命令集合，也不替代后续 Sidecar 的专用配置生命周期。
