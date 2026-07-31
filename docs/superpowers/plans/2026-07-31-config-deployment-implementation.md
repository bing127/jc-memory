# 疆诚记忆配置与部署实施计划

> **面向执行代理：** 必须使用 `superpowers:subagent-driven-development`（推荐）或 `superpowers:executing-plans` 逐 Task 执行本计划；所有步骤使用复选框跟踪。

**目标：** 为 `memory-server` 增加 YAML 配置与环境变量覆盖，并提供不含真实 Secret 的 Docker/Kubernetes 部署资源。

**架构：** 配置加载拆为“可选 YAML 文件 + 环境变量查找器 + 默认值校验”三层，环境变量优先于 YAML，API Key 和私钥内容不进入 YAML。部署资源只引用稳定的 `/healthz`、`/readyz`、HTTP/gRPC 端口和 Secret 注入约定。

**技术栈：** Rust 2024、Serde、`yaml_serde` 0.10、Tokio、Docker、Docker Compose、Kubernetes YAML。

---

### Task 1：建立可测试的配置模型和 YAML 解析

**文件：**
- 修改：`services/memory-server/Cargo.toml`
- 修改：`services/memory-server/src/config.rs`
- 测试：`services/memory-server/src/config.rs` 内单元测试

- [x] **步骤 1：写 YAML 配置解析失败测试**

新增测试使用内存字符串验证：合法 YAML 能解析 `bind`、`grpc_bind`、`data_dir`、`max_in_flight`、`max_requests_per_second` 和 TLS 路径；未知字段与类型错误分别返回带字段上下文的 `ConfigError`。

- [x] **步骤 2：运行测试确认 RED**

运行：

```bash
CARGO_INCREMENTAL=0 cargo test -p jc-memory-server config::tests::yaml
```

预期：因 `ServerConfig::from_yaml_str` 和 YAML 依赖尚不存在而失败。

- [x] **步骤 3：加入 `yaml_serde` 并实现无副作用 YAML 解析**

在 `services/memory-server/Cargo.toml` 增加 `yaml_serde.workspace = true`，根 `Cargo.toml` 增加 `yaml_serde = "0.10"`。在 `config.rs` 增加私有可选字段结构和：

```rust
fn from_yaml_str(input: &str) -> Result<PartialConfig, ConfigError>;
```

YAML 反序列化使用 `#[serde(deny_unknown_fields)]`；解析错误不得输出私钥内容。

- [x] **步骤 4：运行 YAML 测试确认 GREEN**

运行同一聚焦命令，预期合法配置、未知字段和类型错误测试全部通过。

- [x] **步骤 5：提交配置模型**

```bash
git add Cargo.toml Cargo.lock services/memory-server/Cargo.toml services/memory-server/src/config.rs
git commit -m "feat(config): 增加 YAML 配置模型"
```

### Task 2：实现配置文件、环境变量和默认值合并

**文件：**
- 修改：`services/memory-server/src/config.rs`
- 测试：`services/memory-server/src/config.rs` 内单元测试

- [x] **步骤 1：写来源优先级 RED 测试**

测试传入 YAML 部分配置和内存环境变量查找器，验证优先级为环境变量 > YAML > 默认值；空白环境变量不覆盖 YAML；`JC_MEMORY_CONFIG_FILE` 缺失、读取失败、TLS 只配置一半、并发或速率为零均返回稳定错误。

- [x] **步骤 2：运行测试确认 RED**

```bash
CARGO_INCREMENTAL=0 cargo test -p jc-memory-server config::tests::merge
```

- [x] **步骤 3：实现合并入口**

增加：

```rust
pub fn from_env() -> Result<Self, ConfigError>;
pub fn from_sources(path: Option<&Path>, lookup: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError>;
```

`JC_MEMORY_CONFIG_FILE` 只负责指定 YAML 路径；文件字段先生成 `PartialConfig`，再逐字段应用非空环境变量，最后填默认值并统一校验。现有 `from_env` 委托到 `from_sources`，保持旧环境变量调用不变。

- [x] **步骤 4：运行配置全测**

```bash
CARGO_INCREMENTAL=0 cargo test -p jc-memory-server config::tests
```

- [x] **步骤 5：提交合并逻辑**

```bash
git add services/memory-server/src/config.rs
git commit -m "feat(config): 支持 YAML 与环境变量合并"
```

### Task 3：接线 Server 启动和公开配置文档

**文件：**
- 修改：`services/memory-server/src/main.rs`
- 修改：`docs/superpowers/plans/2026-07-29-m4-runtime-services-implementation.md`
- 创建：`docs/configuration.zh-CN.md`

- [x] **步骤 1：写启动入口契约测试**

通过纯配置函数验证 `JC_MEMORY_CONFIG_FILE` 指向的 YAML 能生成与旧环境变量模式相同的 `ServerConfig`；不在 Rust 测试中调用 `std::env::set_var`。

- [x] **步骤 2：实现启动接线**

`main.rs` 使用 `ServerConfig::from_env()`，把解析后的速率、并发、TLS 和数据目录传入现有 ServerState；错误只显示变量名/文件路径，不显示 API Key 或私钥内容。

- [x] **步骤 3：编写中文配置参考**

`docs/configuration.zh-CN.md` 列出 YAML 字段、环境变量映射、优先级、默认值、TLS 文件权限和 Secret 注入示例；示例 API Key 使用明确的占位文本，不可直接作为凭据。

- [x] **步骤 4：运行 Server 测试、文档和差异门禁**

```bash
CARGO_INCREMENTAL=0 cargo test -p jc-memory-server
RUSTDOCFLAGS='-D warnings' cargo doc -p jc-memory-server --no-deps
git diff --check
```

- [x] **步骤 5：提交启动接线和配置文档**

```bash
git add services/memory-server/src/main.rs docs/configuration.zh-CN.md docs/superpowers/plans/2026-07-29-m4-runtime-services-implementation.md
git commit -m "feat(config): 接入 Server 配置文件"
```

### Task 4：提供 Docker 部署资源

**文件：**
- 创建：`deploy/docker/Dockerfile`
- 创建：`deploy/docker/compose.yaml`
- 创建：`deploy/docker/config/server.yaml.example`
- 创建：`docs/deployment.zh-CN.md`

- [ ] **步骤 1：写资源静态契约检查**

使用文本/结构化检查验证 Dockerfile 构建 `memory-server`、运行阶段非 root、Compose 不包含真实 API Key、配置通过 `JC_MEMORY_CONFIG_FILE` 挂载、数据目录为持久化卷。

- [ ] **步骤 2：实现多阶段 Dockerfile**

构建阶段使用 Rust 1.97 工具链编译 `memory-server`；运行阶段使用最小可运行镜像、非 root 用户、`/var/lib/jc-memory` 数据目录和 `/etc/jc-memory/server.yaml` 配置路径。

- [ ] **步骤 3：实现 Compose 模板**

Compose 暴露 `8787` HTTP 和 `8788` gRPC，挂载配置/数据目录，API Key 使用 `${JC_MEMORY_API_KEY:?}`，不在 YAML 中写入默认秘密。

- [ ] **步骤 4：运行可用工具上的静态验证**

可用时运行 `docker build -f deploy/docker/Dockerfile .`；不可用时执行 YAML 结构检查并明确未完成真实镜像构建。

- [ ] **步骤 5：提交 Docker 资源**

```bash
git add deploy/docker docs/deployment.zh-CN.md
git commit -m "feat(deploy): 增加 Docker Compose 部署资源"
```

### Task 5：提供 Kubernetes 部署资源

**文件：**
- 创建：`deploy/kubernetes/configmap.yaml`
- 创建：`deploy/kubernetes/secret.example.yaml`
- 创建：`deploy/kubernetes/deployment.yaml`
- 创建：`deploy/kubernetes/service.yaml`
- 修改：`docs/deployment.zh-CN.md`

- [ ] **步骤 1：写 Kubernetes 结构契约检查**

验证 Deployment 引用 ConfigMap 和 Secret、容器不以 root 运行、HTTP 探针指向 `/healthz`/`/readyz`、Service 同时暴露 HTTP/gRPC，Secret 示例不包含真实值。

- [ ] **步骤 2：实现 ConfigMap 与 Secret 模板**

ConfigMap 只挂载非秘密 `server.yaml`；Secret 示例只保留 `JC_MEMORY_API_KEY`、TLS 证书和私钥字段名称与替换说明。

- [ ] **步骤 3：实现 Deployment 与 Service**

Deployment 设置资源请求/上限、`runAsNonRoot`、只读根文件系统、数据卷和配置卷；Service 使用明确端口名 `http`、`grpc`。

- [ ] **步骤 4：执行本地结构验证**

可用时运行：

```bash
kubectl apply --dry-run=client -f deploy/kubernetes/
```

否则使用 `yaml_serde` 结构检查脚本，并把结果标为 Fixture/静态验证，不宣称真实集群验证。

- [ ] **步骤 5：提交 Kubernetes 资源**

```bash
git add deploy/kubernetes docs/deployment.zh-CN.md
git commit -m "feat(deploy): 增加 Kubernetes 部署资源"
```

### Task 6：完成配置与部署阶段门禁

- [ ] **步骤 1：运行聚焦测试、Clippy、文档和 `git diff --check`**
- [ ] **步骤 2：检查敏感字段和示例凭据**

```bash
rg -n "sk-|BEGIN .*PRIVATE KEY|真实密码|真实 token" deploy docs/configuration.zh-CN.md docs/deployment.zh-CN.md
```

预期只有字段名、明确占位符和测试说明，不出现可用凭据。

- [ ] **步骤 3：更新 M4 计划**

记录配置来源优先级、部署资源、静态验证结果和未完成的请求超时/完整 Proto/Sidecar/MCP 范围。

- [ ] **步骤 4：阶段提交并推送**

```bash
git add docs/superpowers/plans/2026-07-31-config-deployment-implementation.md
git commit -m "docs: 增加配置与部署实施计划"
git push origin feat/m4-runtime-services
```
