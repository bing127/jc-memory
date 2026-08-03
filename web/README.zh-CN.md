# 疆诚记忆 Web 控制面

`web/` 是独立于权威 Memory 数据库的控制面实现：

- `frontend/`：React、Vite、TanStack Query 与 shadcn 风格组件；
- `backend/`：Fastify BFF、登录 Session、Instance Registry 与 `memory-server` 代理；
- 控制面 SQLite 只保存账号会话所需配置、实例元数据和加密 API Key；
- Fragment、Event、EvidenceRef、Memory Packet 和 Explain 响应不会写入控制面数据库。

## 当前真实能力

页面已接入现有 `memory-server` 的 Health、Ready、Metrics readiness、Stats、Observe、Commit、Correct、Forget、Recall、Explain、Runtime Run/Transition/Checkpoint 查询，以及配置化 Provider 驱动的 Runtime FinalResponse Start。Runtime 查询与启动都按实例 API Key 绑定的精确 Scope 执行；浏览器不能提交 Scope 或 Grant，也不会持有实例 API Key、Provider Token。实例没有 Runtime 查询契约时，页面显示 `CORE` 边界，不使用 Fixture 或演示数据填充生产页面。

Memory Lab 支持普通 Text 与显式 State 两种 Observe 模式。State 表单写入 `metadata.state`，Current State 与 Historical State Recall 分别展示当前值、历史版本、原因、有效时间和 Evidence ID。系统不会从普通文本猜测 State，也不会把 LLM 的 Prediction、Inference 或 Plan 提升为 Verified Fact。

Commit 失败后不会自动重试；Correct 和 Forget 要求审计原因，Forget 还要求再次输入目标 Fragment ID。Correct/Forget 只追加带 EvidenceRef 的新权威记录，原始 Fragment/Event/EvidenceRef 不会被覆盖；Forget 不等于 Hard Delete。

Runtime Start 把用户 Prompt 作为 `Observation + Unverified` 追加，经真实 Recall Context 调用 Server 配置的 Provider，并把 FinalResponse 作为 `Inference + Unverified` 追加；不会把模型输出写成 Verified Fact。Approval Decision、Tool Execute、Pause/Resume、模型/Provider 管理、Governance、Recovery 与 Managed Sandbox 只在能力矩阵中显示真实状态，不提供虚假数据或空操作。

## 环境要求

- Node.js 22.22 或更高版本，推荐 Node.js 24 或更高版本；
- pnpm 9.15.9；
- 一个可访问的 `jc-memory-server`，HTTP 仅允许 loopback，远程实例必须使用 HTTPS。

Node.js 22 会把内置 SQLite 标记为 experimental；推荐 Node.js 24+ 运行控制面生产进程。

## 配置

```bash
cd web
cp .env.example .env
openssl rand -base64 32
openssl rand -base64 32
```

把两次随机输出分别写入 `JC_WEB_SESSION_SECRET` 和 `JC_WEB_CREDENTIAL_KEY`，并设置长度至少 12 个字符的 `JC_WEB_ADMIN_PASSWORD`。两个密钥不能复用。

生产环境必须：

- 通过 TLS 暴露控制面；
- 设置 `NODE_ENV=production`，使 Session Cookie 带 `Secure`；
- 把 `JC_WEB_ALLOWED_ORIGINS` 限制为实际前端 Origin；
- 使用独立、受限权限的控制面数据目录；
- 不在日志、命令行历史或配置样例中保存真实 API Key。

控制面 BFF 的上游超时和统一实例 WebSocket 的四类 Topic 周期由环境变量配置：`JC_WEB_UPSTREAM_TIMEOUT_MS` 默认 30 秒，`JC_WEB_STREAM_OVERVIEW_INTERVAL_MS`、`JC_WEB_STREAM_METRICS_INTERVAL_MS`、`JC_WEB_STREAM_RUNTIME_LIST_INTERVAL_MS`、`JC_WEB_STREAM_RUNTIME_DETAIL_INTERVAL_MS` 默认分别为 15 秒、1 秒、10 秒和 5 秒。上游超时范围为 `1..=300000` 毫秒，四类采样周期范围为 `250..=300000` 毫秒；非法、零值、非整数或越界值会阻止 BFF 启动。WebSocket 首轮快照仍由订阅消息立即触发，后续采样复用同一条连接的已校验周期。

## 启动

安装依赖并同时启动前后端：

```bash
cd web
corepack pnpm install
corepack pnpm dev
```

- 前端：`http://127.0.0.1:5173`
- 后端：`http://127.0.0.1:8790`

前端公开入口为 `/`，展示项目定位、当前 `LIVE` 能力和不虚构金额的部署方向；`Get Started` 进入 `/login`，登录后才进入实例、Memory Lab、Runtime 与能力矩阵控制台。公开页面与控制台使用 Tailwind CSS 4 和 shadcn-compatible 组件约定。

前端开发服务器把 `/api` 代理到 BFF。实例 API Key 必须匹配 `^sk-jcmemory-[a-z0-9]{34}$`，只通过登录后的实例连接表单发送给 BFF；之后浏览器只使用实例 ID。

## State 闭环操作

连接实例后，在 Memory Lab 中可以复现以下可信状态链路：

1. 选择 `State`，填写 key `project.deployment_strategy`、JSON value `"blue_green"` 和变更原因，然后执行 `Observe State`；
2. 在 `Correct` 中使用 Observe 返回的 Fragment ID，附带相同 State key、JSON value `"canary"` 和新的原因；
3. 在 `Recall` 中选择 `Current State`，按 key 查询并确认当前值为 `canary`；
4. 切换到 `Historical State`，确认 `blue_green` 为 `superseded`，并核对起止时间、原因和 Evidence ID；
5. 在 `Forget` 中使用 Correct 返回的新 Fragment ID，完成双重确认后再次执行 Current State Recall，当前值应为空，历史中的 `canary` 为 `invalidated`。

Correct 必须操作当前开放版本；跨 Scope、目标不存在或 State key 不匹配会在追加权威记录前被拒绝。Forget 关闭目标 StateVersion，但不删除或覆盖原始 Fragment、Event 和 EvidenceRef。

## 验证

```bash
cd web
corepack pnpm test
corepack pnpm typecheck
corepack pnpm build
corepack pnpm test:e2e
corepack pnpm test:e2e:real
```

`test:e2e` 使用版本化 Fixture 验证 UI/BFF 契约和 Fixture 专用故障路径；`test:e2e:real` 启动临时 SQLite 数据目录中的真实 Rust `jc-memory-server`，且不得访问 `__fixture` 专用端点。两者都在本机执行，不代表外部 Server、Sidecar、MCP、SDK 安装环境或云平台实机验证。
