# 配置硬编码治理实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 修复部署配置缺陷，并把 Security、Server Runtime/SQLite、Web BFF、Neural Fallback 和 Hermes 的可运营策略改为严格、版本化且可验证的配置。

**Architecture:** 保留代码级协议不变量和资源绝对上限，使用编译内置 YAML 提供默认策略，允许 Server YAML 或环境变量显式覆盖。所有外部配置在启动阶段严格校验且失败关闭，Secret 继续只由环境变量或部署平台 Secret 注入。

**Tech Stack:** Rust 1.97、Serde、yaml_serde、SHA-256、Axum/Tonic、SQLite/SQLx、TypeScript、Fastify、Zod、Vitest、Python、pytest、Docker Compose、Kubernetes YAML。

---

## 文件结构

- `config/security/quarantine.zh-CN.yaml`：版本化 Quarantine 规则默认文件。
- `crates/memory-security/src/policy.rs`：严格解析、规范化、校验和与规则访问接口。
- `crates/memory-security/src/{injection,sensitive,poisoning,quarantine}.rs`：只消费已校验策略，不再维护业务词项。
- `crates/memory-core/src/config.rs`：Embedded Engine 的 SQLite 与 Security 文件配置。
- `crates/memory-core/src/engine.rs`：启动时加载外部或内置安全策略，并注入真实准入链路。
- `services/memory-server/src/config.rs`：YAML、环境变量和默认值的统一合并与校验。
- `services/memory-server/src/main.rs`、`runtime_backend.rs`：消费统一 SQLite、Provider 和 Runtime Recall 配置。
- `deploy/docker/**`、`deploy/kubernetes/**`：容器文件路径和部署身份注入。
- `web/backend/src/config.ts`：BFF 上游超时和 Topic 周期配置。
- `web/backend/src/app.ts`、`instance-stream.ts`、`routes/instances.ts`：注入已校验 BFF 运行策略。
- `config/neural/fallback-routing.zh-CN.yaml`：版本化离线路由词项。
- `crates/memory-neural/src/routing.rs`、`fallback.rs`：严格规则加载与路由消费。
- `integrations/hermes/plugin/jc_memory/config.py`：严格 URL、字段集合和数值范围校验。
- `integrations/hermes/tests/**`：配置拒绝与 Commit 锁一致性门禁。
- `docs/configuration.zh-CN.md`、`web/README.zh-CN.md`、`docs/sidecar.zh-CN.md`、Hermes 文档：中文配置参考。

### Task 1: 版本化 Security Quarantine 策略

**Files:**
- Create: `config/security/quarantine.zh-CN.yaml`
- Create: `crates/memory-security/src/policy.rs`
- Modify: `crates/memory-security/Cargo.toml`
- Modify: `crates/memory-security/src/lib.rs`
- Modify: `crates/memory-security/src/injection.rs`
- Modify: `crates/memory-security/src/sensitive.rs`
- Modify: `crates/memory-security/src/poisoning.rs`
- Modify: `crates/memory-security/src/quarantine.rs`
- Test: `crates/memory-security/tests/quarantine_policy.rs`

- [ ] **Step 1: 写严格解析红灯测试**

新增测试，要求以下 API 存在并拒绝未知字段、空词项、重复词项、非有限/越界分数和零重复阈值：

```rust
let policy = QuarantinePolicy::from_yaml_str(valid_yaml()).unwrap();
assert_eq!(policy.version(), "fixture-1");
assert_eq!(policy.sha256().len(), 64);
assert!(QuarantinePolicy::from_yaml_str(&format!("{}\nunknown: true", valid_yaml())).is_err());
```

- [ ] **Step 2: 运行测试并确认因 API 缺失失败**

Run: `cargo test -p jc-memory-security --test quarantine_policy`
Expected: FAIL，`QuarantinePolicy` 或 `from_yaml_str` 不存在。

- [ ] **Step 3: 实现策略类型和内置 YAML**

实现 `QuarantinePolicy::from_yaml_str`、`builtin`、`version`、`sha256` 和规则访问器。所有集合规范化为小写、去重并受代码级最大数量与字符长度限制；原始 YAML 计算 SHA-256。

- [ ] **Step 4: 写真实检测链红灯测试**

使用不包含默认关键词的 Fixture 规则构造 Pipeline，证明自定义词项能触发、自定义阈值能改变 Admission，且原始 Fragment/Evidence 不被改写。

- [ ] **Step 5: 运行检测测试并确认仍使用硬编码词项失败**

Run: `cargo test -p jc-memory-security --test quarantine_policy custom_policy`
Expected: FAIL，Pipeline 无法接收自定义策略或仍读取代码数组。

- [ ] **Step 6: 注入策略并删除业务词项硬编码**

把检测函数改为接收 `&QuarantinePolicy`。`QuarantinePipeline` 持有 `Arc<QuarantinePolicy>`，提供 `new` 和可失败的 `builtin`；RiskAssessment 记录策略版本和摘要，检测器说明不含规则正文。

- [ ] **Step 7: 运行 Security 全包测试**

Run: `cargo test -p jc-memory-security`
Expected: PASS。

- [ ] **Step 8: 提交 Security 批次**

```bash
git add config/security crates/memory-security
git commit -m "feat(security): 外置版本化隔离规则" \
  -m "使用严格 YAML 加载注入、敏感数据与污染检测策略，并记录版本和校验和。" \
  -m "规则只改变准入决策，不覆盖原始 Fragment、Event 或 EvidenceRef。"
```

### Task 2: Core 与 Server 配置贯通

**Files:**
- Modify: `crates/memory-core/src/config.rs`
- Modify: `crates/memory-core/src/engine.rs`
- Modify: `crates/memory-core/src/lib.rs`
- Modify: `services/memory-server/src/config.rs`
- Modify: `services/memory-server/src/main.rs`
- Modify: `services/memory-server/src/runtime_backend.rs`
- Test: `crates/memory-core/tests/poisoning.rs`
- Test: `services/memory-server/src/config.rs`

- [ ] **Step 1: 写 Core 外部规则红灯测试**

创建临时规则文件并通过期望 API 构造 Engine：

```rust
let config = MemoryConfig::local(data.path())
    .with_security_rules_file(policy_path.clone())
    .with_sqlite_max_connections(2)
    .with_busy_timeout(Duration::from_millis(250));
let engine = MemoryEngine::open(config).await.unwrap();
```

提交只命中 Fixture 词项的 Fragment，断言其被 Quarantine；文件缺失或非法时断言 `MemoryEngine::open` 失败且不静默回退。

- [ ] **Step 2: 运行 Core 红灯测试**

Run: `cargo test -p jc-memory-core --test poisoning external_security_policy`
Expected: FAIL，`with_security_rules_file` 或对应错误类型不存在。

- [ ] **Step 3: 实现 MemoryConfig 与 Engine 加载**

为 `MemoryConfig` 增加 `sqlite_max_connections` 和 `security_rules_file`，保留默认连接数 4 与 Busy Timeout 5 秒。Engine 启动时严格加载策略并把 Pipeline 保存为字段，Observe/Commit 两条准入路径复用同一 Pipeline。

- [ ] **Step 4: 写 Server YAML/环境覆盖红灯测试**

测试以下结构能解析，并且环境变量覆盖 YAML：

```yaml
sqlite:
  max_connections: 2
  busy_timeout_ms: 250
security:
  rules_file: ./security.yaml
runtime:
  provider:
    max_retries: 2
    retry_backoff_ms: 50
  recall:
    max_seeds: 4
    max_neighbors: 6
    max_hops: 2
    max_nodes: 24
    timeout_ms: 900
```

- [ ] **Step 5: 运行 Server 红灯测试**

Run: `cargo test -p jc-memory-server config::tests::operational_sections_merge_and_validate`
Expected: FAIL，YAML 因未知字段或配置字段缺失被拒绝。

- [ ] **Step 6: 实现 Server 统一配置**

新增经过校验的 `SqliteRuntimeConfig`、`RuntimeTuningConfig` 和 `RuntimeRecallConfig`。环境变量名与设计规格一致；零连接、零超时、越界 Recall、Provider 重试退避溢出均在启动阶段失败。

- [ ] **Step 7: 接入真实 Server 构造路径**

`MemoryEngine` 和 Runtime Read Store 使用同一连接池参数。Provider 使用配置化重试和退避；`CommandBusRuntimeBackend` 构造时接收 Runtime Recall Limits，不再在 `recall` 方法内构造固定字典。

- [ ] **Step 8: 运行 Core 与 Server 聚焦测试**

Run: `cargo test -p jc-memory-core --test poisoning && cargo test -p jc-memory-server`
Expected: PASS。

- [ ] **Step 9: 提交 Core/Server 批次**

```bash
git add crates/memory-core services/memory-server
git commit -m "feat(config): 贯通存储与运行策略" \
  -m "为 SQLite、Security、Provider 重试和 Runtime Recall 增加严格 YAML 与环境变量覆盖。" \
  -m "Server 的两个 SQLite 使用点共享同一配置，外部规则加载失败时阻止启动。"
```

### Task 3: Docker 与 Kubernetes 部署契约

**Files:**
- Modify: `deploy/docker/Dockerfile`
- Modify: `deploy/docker/compose.yaml`
- Modify: `deploy/docker/config/server.yaml.example`
- Modify: `deploy/kubernetes/configmap.yaml`
- Modify: `deploy/kubernetes/deployment.yaml`
- Modify: `deploy/kubernetes/secret.example.yaml`
- Test: `services/memory-server/tests/deployment_contract.rs`

- [ ] **Step 1: 写部署契约红灯测试**

测试读取仓库部署文件，断言 Docker Server YAML 的 `/etc/jc-memory/retrieval/aliases.zh-CN.yaml` 与 `/etc/jc-memory/security/quarantine.zh-CN.yaml` 都出现在 Dockerfile COPY 中；ConfigMap 不包含 `namespace_id`；Deployment 的 `JC_MEMORY_NAMESPACE_ID` 与 `JC_MEMORY_API_KEY` 都来自 Secret。

- [ ] **Step 2: 运行红灯测试**

Run: `cargo test -p jc-memory-server --test deployment_contract`
Expected: FAIL，当前 Dockerfile 未复制规则且 ConfigMap 含固定 Namespace。

- [ ] **Step 3: 修复部署文件**

运行镜像创建 `/etc/jc-memory/retrieval` 和 `/etc/jc-memory/security`，复制两份版本化 YAML。Compose 可选只读挂载同路径。Kubernetes 从 Secret 注入 Namespace，示例 Secret 使用 `replace-with-a-namespace-uuid` 占位值。

- [ ] **Step 4: 运行契约和 Server Schema 测试**

Run: `cargo test -p jc-memory-server --test deployment_contract && cargo test -p jc-memory-server config::tests::docker_yaml_example_stays_compatible_with_the_server_schema`
Expected: PASS。

- [ ] **Step 5: 提交部署批次**

```bash
git add deploy services/memory-server/tests/deployment_contract.rs
git commit -m "fix(deploy): 消除容器配置硬编码" \
  -m "把版本化检索与安全规则复制到运行镜像，并改用绝对容器路径。" \
  -m "Kubernetes Namespace 改由 Secret 显式注入，避免不同环境复用固定 Scope。"
```

### Task 4: Web BFF 运行策略配置化

**Files:**
- Modify: `web/backend/src/config.ts`
- Modify: `web/backend/src/app.ts`
- Modify: `web/backend/src/instance-stream.ts`
- Modify: `web/backend/src/routes/instances.ts`
- Modify: `web/backend/tests/config.test.ts`
- Modify: `web/backend/tests/instance-stream.test.ts`
- Modify: `web/.env.example`

- [ ] **Step 1: 写 Config 红灯测试**

加载环境变量并断言 `upstreamTimeoutMs` 与四个 `streamIntervals` 可覆盖默认值；零、负数、非整数和超过上限被 Zod 拒绝。

- [ ] **Step 2: 运行 Config 红灯测试**

Run: `cd web && corepack pnpm --filter @jiangcheng-memory/control-plane-backend test -- config.test.ts`
Expected: FAIL，新配置字段不存在。

- [ ] **Step 3: 实现严格环境配置**

为 `ControlPlaneConfig` 增加运行策略，默认值保持 30 秒、15/1/10/5 秒。上游超时上限 5 分钟，采样周期范围 `250..=300000` 毫秒。

- [ ] **Step 4: 写注入红灯测试**

通过测试配置把 metrics 周期设为 20ms，证明 WebSocket 在测试窗口内收到多次采样；另一个测试把上游超时传入可控 Fetch 并断言 AbortSignal 使用配置值。

- [ ] **Step 5: 运行注入红灯测试**

Run: `cd web && corepack pnpm --filter @jiangcheng-memory/control-plane-backend test -- instance-stream.test.ts`
Expected: FAIL，`buildApp` 仍使用固定 30 秒和模块常量周期。

- [ ] **Step 6: 注入运行策略并删除硬编码字典**

`buildApp` 创建 Upstream Client 时传入超时；Instance Route 把已校验周期传给 `attachInstanceStream`。测试辅助配置显式包含新增字段。

- [ ] **Step 7: 运行 Backend 全测试与类型检查**

Run: `cd web && corepack pnpm --filter @jiangcheng-memory/control-plane-backend test && corepack pnpm --filter @jiangcheng-memory/control-plane-backend typecheck`
Expected: PASS。

- [ ] **Step 8: 提交 Web 批次**

```bash
git add web/backend web/.env.example
git commit -m "feat(web): 配置上游超时与采样周期" \
  -m "BFF 启动时严格校验上游 deadline 和四类 Topic 周期，并注入统一实例订阅流。" \
  -m "浏览器继续使用单条 WebSocket，不恢复前端周期 HTTP 轮询。"
```

### Task 5: Neural Fallback 路由规则外置

**Files:**
- Create: `config/neural/fallback-routing.zh-CN.yaml`
- Create: `crates/memory-neural/src/routing.rs`
- Modify: `crates/memory-neural/Cargo.toml`
- Modify: `crates/memory-neural/src/lib.rs`
- Modify: `crates/memory-neural/src/fallback.rs`
- Test: `crates/memory-neural/tests/fallback.rs`

- [ ] **Step 1: 写自定义路由规则红灯测试**

用 Fixture YAML 把唯一词项 `root-cause-fixture` 映射为 Causal，构造 `DeterministicFallback::from_routing_yaml_str` 并断言路由结果，以及 Router Manifest 的 `model_version`、`version` 和 `checksum_sha256` 随规则版本与摘要变化。

- [ ] **Step 2: 运行红灯测试**

Run: `cargo test -p jc-memory-neural --test fallback custom_routing_policy`
Expected: FAIL，构造入口和路由元数据不存在。

- [ ] **Step 3: 实现严格 RoutingPolicy**

解析 `schema_version`、`policy_version` 和三类非空词项，规范化、去重、限制数量与长度。默认 Fallback 使用编译内置 YAML，路由方法只读取 Policy。只重建 Router 自身的 Manifest：`version` 使用规则版本，`model_version` 包含规则摘要短标识，由现有 Manifest 校验和覆盖完整契约；其余七个 Trait 的 Manifest Schema 和版本保持不变。

- [ ] **Step 4: 运行 Neural 全包测试**

Run: `cargo test -p jc-memory-neural`
Expected: PASS。

- [ ] **Step 5: 提交 Neural 批次**

```bash
git add config/neural crates/memory-neural
git commit -m "feat(neural): 外置离线路由规则" \
  -m "使用严格版本化 YAML 管理多语言 Recall Mode 触发词，并保留内置离线默认值。" \
  -m "规则版本和校验和可查询，当前 Server 零模型检索路径保持不变。"
```

### Task 6: Hermes 配置与来源锁门禁

**Files:**
- Modify: `integrations/hermes/plugin/jc_memory/config.py`
- Modify: `integrations/hermes/tests/test_config.py`
- Modify: `integrations/hermes/tests/test_bootstrap_contract.py`

- [ ] **Step 1: 写非法配置红灯测试**

参数化覆盖远程 HTTP、带凭据 URL、`NaN`/零/负超时、未知 `jc_memory` 字段、缺失/未知 Recall 字段、布尔值和越界限制。

- [ ] **Step 2: 运行红灯测试**

Run: `python3 -m pytest integrations/hermes/tests/test_config.py -q`
Expected: FAIL，当前转换接受至少一个非法输入。

- [ ] **Step 3: 实现启动阶段严格校验**

使用 `urllib.parse.urlsplit` 校验 URL；定义固定允许字段和 Recall 范围；显式排除 `bool`；使用 `math.isfinite` 检查超时。错误只包含字段名，不包含 API Key。

- [ ] **Step 4: 写 Commit 一致性红灯测试**

读取 `hermes.lock.yaml` 与 Provider 模块，断言 `HERMES_COMMIT == lock["commit"]`，并保留 Bootstrap 对真实外部仓库 HEAD 的检查。

- [ ] **Step 5: 运行 Hermes 全测试**

Run: `python3 -m pytest integrations/hermes/tests -q`
Expected: PASS；真实 Server 测试如由 Marker/环境跳过，必须如实记录跳过项。

- [ ] **Step 6: 提交 Hermes 批次**

```bash
git add integrations/hermes
git commit -m "fix(hermes): 严格校验记忆配置" \
  -m "在 Provider 初始化前拒绝非法 URL、非有限超时、未知字段和越界 Recall 预算。" \
  -m "新增锁文件与来源 Commit 一致性门禁，避免权威元数据记录错误宿主版本。"
```

### Task 7: 中文文档与完整验证

**Files:**
- Modify: `docs/configuration.zh-CN.md`
- Modify: `docs/deployment.zh-CN.md`
- Modify: `docs/security/quarantine.md`
- Modify: `docs/sidecar.zh-CN.md`
- Modify: `web/README.zh-CN.md`
- Modify: `integrations/hermes/config.example.yaml`
- Modify: `docs/superpowers/plans/2026-08-03-config-hardcoding-remediation-implementation.md`

- [ ] **Step 1: 更新中文配置参考**

记录全部新增 YAML 字段、环境变量、默认值、范围、Secret 边界、启动失败策略和规则摘要。补充 `JC_MEMORY_SIDECAR_DAEMON_BIN`，不修改协议字段或命令名称。

- [ ] **Step 2: 更新部署与安全边界**

说明 Docker 文件位置、Kubernetes Namespace 注入、规则只在启动时加载，以及 Fixture/静态契约不等同于真实集群验证。

- [ ] **Step 3: 运行格式和静态门禁**

Run: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && git diff --check`
Expected: PASS。

- [ ] **Step 4: 运行 Rust 测试和文档门禁**

Run: `cargo test --workspace --all-features && RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps`
Expected: PASS。

- [ ] **Step 5: 运行 Web 门禁**

Run: `cd web && corepack pnpm test && corepack pnpm typecheck && corepack pnpm build`
Expected: PASS；现有 Vite chunk 大小提示若仍存在，记录为非阻断提示。

- [ ] **Step 6: 运行 Hermes 门禁与配置差异审计**

Run: `python3 -m pytest integrations/hermes/tests -q`
Run: `rg -n "std::env::var|process.env|os.environ" services web/backend integrations/hermes/plugin integrations/hermes/scripts`
Expected: 测试通过；每个生产配置读取点都有中文文档或明确测试用途。

- [ ] **Step 7: 勾选计划并提交文档**

把实际完成步骤改为 `[x]`，记录未执行的 Docker/Kubernetes 实机验证边界。

```bash
git add docs web/README.zh-CN.md integrations/hermes/config.example.yaml
git commit -m "docs(config): 记录配置治理验收" \
  -m "补充 Server、Security、Web、Sidecar、Hermes 与部署配置参考，并勾选已完成实施步骤。" \
  -m "分别记录本机代码门禁和未执行的外部容器、Kubernetes 集群实机验证。"
```
