# Hermes Desktop 全量记忆接入实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在官方 Hermes Desktop 的真实 `hermes serve` 后端中启用疆诚记忆 Provider，使完整 Turn、工具调用、工具结果、压缩边界和跨 Session Recall 进入真实 `jc-memory-server -> SQLite` 链路，并启动桌面程序供用户配置模型 Key 后手动验收。

**Architecture:** Hermes Desktop 保持 Electron/React 展示层不变，通过已有 JSON-RPC 连接 `hermes serve`。独立用户插件 `jc_memory` 实现 Hermes `MemoryProvider`，复用 `sdk/python` 的 `RemoteMemoryClient` 调用疆诚记忆；Hermes 内建语义记忆关闭，`state.db` 仅保留 Desktop 会话导航和当前聊天记录缓存，不参与跨 Session Recall。

**Tech Stack:** Rust 1.97、Axum、SQLite、Python 3.11-3.13、httpx、pytest、Hermes MemoryProvider、Electron 40、React 19、Vite 8。

---

## 文件结构

```text
.gitignore
integrations/hermes/
  README.zh-CN.md
  config.example.yaml
  hermes.lock.yaml
  plugin/jc_memory/
    __init__.py
    config.py
    mapping.py
    provider.py
    plugin.yaml
  scripts/
    bootstrap.sh
    install-plugin.sh
    run-desktop.sh
    stop.sh
    verify.sh
  tests/
    conftest.py
    test_bootstrap_contract.py
    test_config.py
    test_install_plugin.py
    test_mapping.py
    test_provider.py
    test_real_server.py
```

`external/hermes-agent/` 是被忽略的官方独立克隆；所有可维护集成代码必须位于 `integrations/hermes/`。

### Task 1：锁定 Hermes Desktop 测试宿主

**Files:**
- Modify: `.gitignore`
- Create: `integrations/hermes/hermes.lock.yaml`
- Create: `integrations/hermes/scripts/bootstrap.sh`
- Create: `integrations/hermes/tests/test_bootstrap_contract.py`

- [ ] **Step 1：写失败的宿主锁定测试**

测试加载 `hermes.lock.yaml`，要求仓库为 `https://github.com/NousResearch/hermes-agent.git`、Commit 为 40 位 SHA，并验证 `external/hermes-agent/apps/desktop/package.json`、`agent/memory_provider.py` 和 `plugins/memory/__init__.py` 存在。

- [ ] **Step 2：运行 RED**

Run: `python3 -m pytest integrations/hermes/tests/test_bootstrap_contract.py -q`

Expected: FAIL，因为 lock 与 bootstrap 尚不存在。

- [ ] **Step 3：实现最小锁定与 bootstrap**

`hermes.lock.yaml` 固定当前真实 Commit：

```yaml
schema_version: hermes-integration-lock.v1
repository: https://github.com/NousResearch/hermes-agent.git
branch: main
commit: ebf967ff2cecdc040cd9701b0cf52059c7e8da4b
desktop_path: apps/desktop
```

`bootstrap.sh` 幂等执行浅克隆、校验 origin 与 Commit，不对已有脏 Hermes 工作树执行 reset。

- [ ] **Step 4：运行 GREEN**

Run: `python3 -m pytest integrations/hermes/tests/test_bootstrap_contract.py -q`

Expected: PASS。

### Task 2：实现配置与 Scope 映射

**Files:**
- Create: `integrations/hermes/config.example.yaml`
- Create: `integrations/hermes/plugin/jc_memory/config.py`
- Create: `integrations/hermes/tests/test_config.py`

- [ ] **Step 1：写配置 RED 测试**

覆盖：从 Hermes `memory.jc_memory` 读取非 Secret 配置；API Key 只从 `JC_MEMORY_API_KEY` 读取；缺少 URL、Namespace 或 Key 时 `is_available` 为 false；Tenant/User/Agent/Project 必须为 UUID 或 null。

- [ ] **Step 2：运行 RED**

Run: `python3 -m pytest integrations/hermes/tests/test_config.py -q`

Expected: FAIL，因为 `JcMemoryConfig` 不存在。

- [ ] **Step 3：实现配置对象**

核心接口：

```python
@dataclass(frozen=True)
class JcMemoryConfig:
    base_url: str
    api_key: str
    access_scope: dict[str, str | None]
    timeout_seconds: float
    recall_limits: dict[str, int]

    @classmethod
    def load(cls) -> "JcMemoryConfig": ...
```

示例 YAML 设置 `memory_enabled: false`、`user_profile_enabled: false`、`provider: jc_memory`，从而关闭 Hermes 内建语义记忆，只保留外部 Provider。

- [ ] **Step 4：运行 GREEN**

Run: `python3 -m pytest integrations/hermes/tests/test_config.py -q`

Expected: PASS。

### Task 3：映射完整 Hermes 消息

**Files:**
- Create: `integrations/hermes/plugin/jc_memory/mapping.py`
- Create: `integrations/hermes/tests/test_mapping.py`

- [ ] **Step 1：写消息映射 RED 测试**

构造一个包含 `user -> assistant(tool_calls) -> tool -> assistant` 的真实 OpenAI 消息列表，断言分别映射为 Observation、ToolCall、ToolResult、Claim；全部初始为 `unverified`；每条记录保存 Hermes Commit、Session ID、消息序号、Tool Call ID；确定性 Fragment UUID 重跑不变。

- [ ] **Step 2：运行 RED**

Run: `python3 -m pytest integrations/hermes/tests/test_mapping.py -q`

Expected: FAIL，因为映射函数不存在。

- [ ] **Step 3：实现最小映射**

公开接口：

```python
def map_messages(
    messages: list[dict[str, object]],
    *,
    session_id: str,
    scope: dict[str, str | None],
    hermes_commit: str,
) -> list[dict[str, object]]: ...
```

Fragment ID 使用 UUIDv5，输入包含 Hermes Commit、Session ID、消息序号、role、tool_call_id 和规范化内容哈希，保证重启重试幂等。不得把 assistant/tool 内容映射为 Fact。

- [ ] **Step 4：运行 GREEN**

Run: `python3 -m pytest integrations/hermes/tests/test_mapping.py -q`

Expected: PASS。

### Task 4：实现真实 JcMemory MemoryProvider

**Files:**
- Create: `integrations/hermes/plugin/jc_memory/__init__.py`
- Create: `integrations/hermes/plugin/jc_memory/provider.py`
- Create: `integrations/hermes/plugin/jc_memory/plugin.yaml`
- Create: `integrations/hermes/tests/test_provider.py`

- [ ] **Step 1：写 Provider RED 测试**

覆盖以下行为：

- `register(ctx)` 注册且只注册一个 `JcMemoryProvider`；
- `initialize` 保存 Session 与 Hermes 运行上下文；
- `sync_turn(..., messages=...)` Commit 完整消息且重复同步不产生新 Fragment ID；
- `prefetch` 调用 Associative Recall，并返回带来源边界的 `recommended_context` 与 EvidenceRef；
- `on_pre_compress` 在压缩丢弃前提交原始消息；
- `on_session_end` 刷新最后消息；
- `shutdown` 关闭 HTTP Client；
- 日志和异常不包含 API Key。

- [ ] **Step 2：运行 RED**

Run: `PYTHONPATH=external/hermes-agent:sdk/python/src python3 -m pytest integrations/hermes/tests/test_provider.py -q`

Expected: FAIL，因为 Provider 尚不存在。

- [ ] **Step 3：实现 Provider**

核心契约：

```python
class JcMemoryProvider(MemoryProvider):
    @property
    def name(self) -> str:
        return "jc_memory"

    def is_available(self) -> bool: ...
    def initialize(self, session_id: str, **kwargs) -> None: ...
    def prefetch(self, query: str, *, session_id: str = "") -> str: ...
    def sync_turn(self, user_content: str, assistant_content: str, *, session_id: str = "", messages=None) -> None: ...
    def on_pre_compress(self, messages) -> str: ...
    def on_session_end(self, messages) -> None: ...
    def on_session_switch(self, new_session_id: str, **kwargs) -> None: ...
    def get_tool_schemas(self) -> list[dict]:
        return []
    def shutdown(self) -> None: ...
```

Provider 不增加模型 Tool schema；Recall 仅通过 Hermes 已有 prefetch 注入层进入 Prompt，保持 Prompt 缓存规则。

- [ ] **Step 4：运行 GREEN**

Run: `PYTHONPATH=external/hermes-agent:sdk/python/src python3 -m pytest integrations/hermes/tests/test_provider.py -q`

Expected: PASS。

### Task 5：安装独立 Provider 并配置隔离 Desktop Home

**Files:**
- Create: `integrations/hermes/scripts/install-plugin.sh`
- Create: `integrations/hermes/tests/test_install_plugin.py`
- Modify: `integrations/hermes/config.example.yaml`

- [ ] **Step 1：写安装 RED 测试**

在临时 `HERMES_HOME` 执行安装脚本，断言 `$HERMES_HOME/plugins/jc_memory` 指向版本控制插件、配置合并后模型配置未被覆盖、`memory.provider` 为 `jc_memory`、内建语义记忆关闭。

- [ ] **Step 2：运行 RED**

Run: `python3 -m pytest integrations/hermes/tests/test_install_plugin.py -q`

Expected: FAIL，因为安装脚本不存在。

- [ ] **Step 3：实现安全安装**

脚本使用 YAML 结构化合并；已有模型和 Secret 配置原样保留。插件使用符号链接安装，便于代码更新立即生效。运行时目录为 `.cache/hermes-integration/`，已被仓库忽略。

- [ ] **Step 4：运行 GREEN**

Run: `python3 -m pytest integrations/hermes/tests/test_install_plugin.py -q`

Expected: PASS。

### Task 6：真实 Server 契约闭环

**Files:**
- Create: `integrations/hermes/tests/test_real_server.py`
- Create: `integrations/hermes/scripts/verify.sh`

- [ ] **Step 1：写真实服务 RED 测试**

测试启动真实 `jc-memory-server`、使用临时 SQLite 和隔离端口，通过真实 Provider 写入唯一用户消息、assistant tool call、ToolResult 和最终回复，然后新建 Provider/Session 执行 Recall，断言命中唯一内容并包含 EvidenceRef；最后断言 Fragment/Event 计数未因幂等重试增加。

- [ ] **Step 2：运行 RED**

Run: `PYTHONPATH=external/hermes-agent:sdk/python/src python3 -m pytest integrations/hermes/tests/test_real_server.py -q`

Expected: FAIL，直到真实 Provider 链路完整。

- [ ] **Step 3：补齐最小真实链路**

只修复由真实测试暴露的协议、映射或生命周期问题，不使用 Mock 替代 Server。

- [ ] **Step 4：运行 GREEN**

Run: `PYTHONPATH=external/hermes-agent:sdk/python/src python3 -m pytest integrations/hermes/tests/test_real_server.py -q`

Expected: PASS，报告 `real_hermes_provider=true`、`real_rust_sqlite=true`。

### Task 7：一条命令启动 Hermes Desktop

**Files:**
- Create: `integrations/hermes/scripts/run-desktop.sh`
- Create: `integrations/hermes/scripts/stop.sh`
- Create: `integrations/hermes/README.zh-CN.md`

- [ ] **Step 1：实现受控启动器**

`run-desktop.sh` 完成以下动作：

1. 校验 Hermes Commit；
2. 创建隔离 `.cache/hermes-integration/home` 与数据目录；
3. 安装 Provider 并生成本地测试 API Key；
4. 启动一个真实 `jc-memory-server` 并等待 `/healthz`；
5. 设置 `HERMES_DESKTOP_HERMES_ROOT`、`HERMES_HOME`、`PYTHONPATH`；
6. 在官方 `apps/desktop` 执行 `npm run dev`；
7. 保存自己启动的 PID，退出时只清理这些 PID。

- [ ] **Step 2：安装 Hermes Desktop 依赖**

Run: `npm install`

Workdir: `external/hermes-agent`

Expected: 根 workspace 安装成功。

- [ ] **Step 3：运行静态验证**

Run: `npm run typecheck --workspace apps/desktop`

Expected: PASS。

- [ ] **Step 4：启动 Desktop**

Run: `integrations/hermes/scripts/run-desktop.sh`

Expected: Hermes Desktop Electron 窗口打开，`jc_memory` Provider 已激活，用户可在 Desktop 设置中配置模型 Key。

### Task 8：手动跨 Session 验收与记录

**Files:**
- Create: `docs/benchmarks/hermes-desktop-memory-acceptance.zh-CN.md`

- [ ] **Step 1：Session A 写入**

用户在 Desktop 输入唯一事实和项目偏好，并要求执行一次无破坏性的工具调用。

- [ ] **Step 2：Session B 召回**

新建 Session B，不恢复 Session A 历史，询问唯一事实。记录回答、Recall request ID、Fragment ID 和 EvidenceRef。

- [ ] **Step 3：重启验收**

完全退出 Desktop 后重新启动，再创建 Session C 重复 Recall。

- [ ] **Step 4：结果分级**

只有真实模型回答使用 Recall 内容且证据可追溯时标记 `PASS_RUNTIME`；未配置模型 Key 时最多标记 `PASS_REAL_BACKEND`；Fixture 或 Mock 只能标记 `PASS_CONTRACT`。

### Task 9：提交前门禁

- [ ] **Step 1：Python 聚焦测试**

Run: `PYTHONPATH=external/hermes-agent:sdk/python/src python3 -m pytest integrations/hermes/tests -q`

- [ ] **Step 2：Rust 服务门禁**

Run: `cargo fmt --all -- --check`

Run: `cargo clippy -p jc-memory-server --all-targets --all-features -- -D warnings`

Run: `cargo test -p jc-memory-server`

- [ ] **Step 3：Hermes Desktop 门禁**

Run: `npm run typecheck --workspace apps/desktop`

Run: `npm run test:desktop:platforms --workspace apps/desktop`

- [ ] **Step 4：仓库检查**

Run: `git diff --check`

检查提交只包含 `integrations/hermes/`、设计/计划/验收文档与 `.gitignore`，不得包含 `external/hermes-agent/`、模型 Key、运行数据库、PID 或用户现有 Recall 修改。
