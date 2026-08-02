# 统一实例 WebSocket 设计

- **状态：** 已实施并验收
- **日期：** 2026-08-02
- **产品名称：** 疆诚记忆
- **英文名称：** Jiangcheng Memory
- **设计范围：** Web 控制面总览、Runtime 与性能观测的实时读取链路

## 1. 背景与问题

当前控制面存在三类周期读取：

- 总览页分别每 15 秒读取实例状态和权威记录统计；
- Runtime 页每 10 秒读取 Run 列表，每 5 秒读取选中 Run 详情；
- 性能页已使用独立 WebSocket，但连接由页面持有，切换页面会销毁并重建。

性能页还把 WebSocket 传输状态与指标采样状态混为一体。BFF 推送采样 `gap` 时，前端把连接标记为失败并显示“等待重连”；下一次成功快照又恢复为“实时流已连接”。错误横幅随状态反复挂载和卸载，导致内容上下位移。连续失败还会不断追加相同缺口并重绘空图表。

## 2. 目标

本设计交付以下行为：

1. 每个当前选中实例最多保持一条控制面 WebSocket；
2. 总览、Runtime 和性能页面不再使用前端周期轮询；
3. 页面通过同一连接订阅所需频道，切换页面不重建实例连接；
4. BFF 仅为已订阅频道采样，并在取消订阅或断开时清理任务；
5. WebSocket 传输错误与单频道采样错误分别建模；
6. 重复采样错误只通知一次，恢复后继续推送真实快照；
7. 性能图表保留真实缺口，但不会因连续相同错误每秒重绘空图；
8. 原始 Fragment、Event 和 EvidenceRef 的权威性及现有写入协议不变。

## 3. 非目标

本次不改变 Rust Server 的 Prometheus pull 模型，不把 BFF 采样描述为 Rust 原生事件推送。以下写操作继续使用现有 HTTP 接口：

- 创建或删除实例；
- Memory Observe、Commit、Recall、Explain、Correct 和 Forget；
- Runtime Start；
- 登录和退出登录。

本次不引入 Redis、消息队列、跨 BFF 节点广播、持久化浏览器事件或新的外部可观测平台。

## 4. 总体架构

```text
AuthenticatedApp
  └─ AppShell
      └─ InstanceStreamProvider（当前实例唯一连接）
          ├─ OverviewPage      订阅 overview
          ├─ RuntimePage       订阅 runtime.list / runtime.detail
          └─ PerformancePage   订阅 metrics

浏览器  ── WebSocket ──>  BFF  ── HTTP pull ──> Rust Server
          订阅/取消订阅       按频道采样
```

连接地址统一为：

```text
GET /api/v1/instances/:instanceId/stream
```

WebSocket 使用现有 Session Cookie，并校验握手 `Origin`。实例不存在、未登录或 Origin 不允许时使用策略违规关闭码 `1008`，且不得启动任何采样任务。

## 5. 客户端订阅协议

客户端只发送版本化 JSON 消息：

```json
{
  "version": 1,
  "type": "subscribe",
  "subscriptions": [
    { "topic": "overview" },
    { "topic": "metrics" },
    { "topic": "runtime.list" },
    { "topic": "runtime.detail", "runId": "00000000-0000-7000-8000-000000000701" }
  ]
}
```

每次 `subscribe` 消息表达该连接的完整期望集合，而不是增量补丁。BFF 比较新旧集合后启动新增频道、停止移除频道。重复发送相同集合必须幂等，不得创建重复定时器。

HTTP 写操作成功后，客户端可要求当前已订阅频道立即采样一次：

```json
{
  "version": 1,
  "type": "refresh",
  "topics": ["runtime.list"]
}
```

`refresh` 不改变订阅集合，不创建新定时器；未订阅 Topic 会返回协议错误。频道已有采样正在执行时跳过该次刷新，避免请求重叠。

频道定义：

| Topic | 参数 | 立即采样 | 后续周期 |
| --- | --- | ---: | ---: |
| `overview` | 无 | 是 | 15 秒 |
| `metrics` | 无 | 是 | 1 秒 |
| `runtime.list` | 无 | 是 | 10 秒 |
| `runtime.detail` | `runId` | 是 | 5 秒 |

未知版本、未知 Topic、非法 UUID、重复冲突参数或刷新未订阅频道不关闭整个连接。BFF 返回协议错误消息并保留上一份合法订阅集合。

## 6. 服务端消息协议

所有服务端消息包含 `version: 1`。成功快照和频道错误还必须包含 `topic` 与 `sampledAt`；协议错误不属于具体频道，不携带这两个字段。

成功快照：

```json
{
  "version": 1,
  "type": "snapshot",
  "topic": "metrics",
  "sampledAt": "2026-08-02T00:00:00.000Z",
  "data": {}
}
```

频道错误：

```json
{
  "version": 1,
  "type": "channel_error",
  "topic": "metrics",
  "sampledAt": "2026-08-02T00:00:00.000Z",
  "code": "upstream_unavailable",
  "message": "实例指标采样失败"
}
```

协议错误：

```json
{
  "version": 1,
  "type": "protocol_error",
  "code": "invalid_subscription",
  "message": "订阅消息格式无效"
}
```

各成功频道的 `data` 沿用现有 HTTP 响应契约：

- `overview`：`{ status, stats }`；
- `metrics`：`PerformanceSnapshot`；
- `runtime.list`：`RuntimeRunPage`；
- `runtime.detail`：`RuntimeRunDetail`。

BFF 不向浏览器发送实例 API Key、内部连接信息或上游原始错误。

## 7. BFF 采样生命周期

每个频道任务包含独立的定时器、执行锁和错误状态：

1. 新订阅建立后立即采样一次；
2. 上一次采样未完成时跳过本周期，禁止请求重叠；
3. 首次失败发送一次 `channel_error`；
4. 同一频道连续失败期间不重复发送相同错误；
5. 失败后首次成功必须发送快照并清除错误状态；
6. 频道取消订阅时清理其定时器；
7. WebSocket `close` 或 `error` 时清理全部频道任务；
8. BFF 关闭时由 Fastify 连接生命周期完成同样清理。

`overview` 的一次采样并行读取 health、ready、metrics 和 Stats。Stats 使用实例 Namespace 与空的可选子 Scope，继续通过现有 CommandBus 契约获得权威统计，不直接读取控制面数据库。

## 8. 前端连接生命周期

`InstanceStreamProvider` 位于 `AppShell` 内，按当前实例 ID 持有连接。页面挂载时注册订阅需求，卸载时移除；Provider 合并所有消费者的需求并发送完整订阅集合。

Provider 维护以下独立状态：

- `transportState`：`connecting | connected | disconnected`；
- 各 Topic 的 `data`、`sampledAt` 和 `error`；
- 指数退避重连次数和唯一重连定时器。

只有真实 WebSocket `close` 或 `error` 才进入传输断开状态。`channel_error` 不得把连接显示为“等待重连”。重连成功后 Provider 自动重发当前订阅集合。

前端使用按实例 ID 建立的连接注册表和引用计数。最后一个 Provider 释放连接时不立即关闭，而是延迟到下一轮事件循环；React StrictMode 紧接着重新获取同一实例时取消释放并复用原生 WebSocket，因此开发期挂载检查也只构造一条连接。真实实例切换、退出实例区域或延迟释放到期后关闭旧连接。

同一轮路由切换中的订阅移除和新增合并后再发送，避免先发送空集合、随后又发送新集合造成无意义的频道启停。BFF 的幂等订阅和任务清理继续作为服务端防线。

## 9. 页面行为

### 9.1 总览

总览页订阅 `overview`，删除两个 React Query `refetchInterval`。首次快照前显示原有 pending 状态；频道失败时保留最后成功的状态与统计，并显示固定位置的错误信息。

### 9.2 Runtime

Runtime 页始终订阅 `runtime.list`；选中 Run 后额外订阅一个带 `runId` 的 `runtime.detail`。删除 10 秒和 5 秒轮询文案及配置。

Runtime Start 仍使用 HTTP mutation。成功后页面选中返回的 `run_id`，Provider 更新详情订阅并对 `runtime.list` 发送 `refresh`；BFF 随即采样新详情和列表，不等待下一个周期。

### 9.3 性能观测

只有打开实时监控 Tab 时订阅 `metrics`。静态报告 Tab 不触发指标采样。

性能页分别显示传输状态和指标频道状态。连续指标失败只追加一个 gap；恢复快照到达后继续序列。错误区域保留稳定布局，不因 `connecting`、`channel_error` 和恢复快照反复插入或移除页面区块。

## 10. 测试与验收

行为变更继续采用 TDD。

后端测试必须证明：

1. 一条已认证连接可订阅多个 Topic 并收到对应初始快照；
2. 重复订阅不会创建重复采样；
3. 切换 `runtime.detail` 的 `runId` 会停止旧任务并立即采样新 Run；
4. 连续上游失败只发送一次频道错误，恢复后发送快照；
5. 取消订阅和关闭连接后不再调用上游；
6. 未登录、非法 Origin 和不存在实例不会启动采样；
7. 非法订阅返回协议错误且保留上一份合法订阅。

前端测试必须证明：

1. AppShell 下同一实例的页面切换复用一个 WebSocket；
2. 总览和 Runtime 不再创建周期 HTTP 请求；
3. 页面订阅集合随路由、实时 Tab 和 Run ID 正确变化；
4. `channel_error` 不改变 transport connected 状态；
5. 性能页连续错误只形成一个 gap，恢复后继续绘图；
6. 传输重连期间布局高度保持稳定；
7. HTTP mutation 成功后仍可立即看到对应 Runtime Run。

Playwright 在桌面和移动端验证：

- 登录后只建立一条实例 WebSocket；
- 依次切换总览、Runtime 和性能页，连接不重新创建；
- 每个页面收到真实 Fixture 快照；
- 性能频道连续失败与恢复时页面不发生纵向跳动；
- SVG 图表非空且时间仍从左向右推进；
- 浏览器请求中不再出现总览和 Runtime 的周期 GET 轮询。

## 11. 发布与兼容性

旧的 `GET /api/v1/instances/:id/metrics/stream` 在前后端同一版本发布后删除，不保留双 WebSocket 实现。现有 HTTP 读取接口暂时保留，供诊断和兼容测试使用，但控制台页面不再周期调用。

部署层必须继续转发 `/api` 的 WebSocket Upgrade。开发环境 Vite 代理已经启用 `ws: true`；生产反向代理和 Kubernetes Ingress 的发布检查必须包含 Upgrade 握手验证。

## 12. 完成标准

- 当前实例只有一条活跃控制面 WebSocket；
- 前端源码不存在总览、Runtime 和性能指标的 `refetchInterval`；
- 页面切换和频道失败不会导致内容区持续闪动或上下位移；
- 所有频道均保留最后成功数据，失败不补零、不伪造权威记录；
- 桌面与移动端测试、类型检查、生产构建和 `git diff --check` 全部通过；
- 中文实施文档、提交记录和远端分支同步完成。

## 13. 实施与验收记录

2026-08-02 已按本设计完成实施，阶段提交如下：

- `7c8ba19 feat(web): 增加统一实例订阅流`；
- `deda963 feat(web): 共享实例 WebSocket 连接`；
- `1712c8a refactor(web): 总览改用实例订阅流`；
- `25e86f5 refactor(web): Runtime 改用实例订阅流`；
- `267806d fix(web): 消除实时性能页面闪动`；
- `e132573 test(web): 验证实例单连接与无轮询`。

最终验收证据：

- 后端 Vitest 单 worker：6 个测试文件、35 项测试通过；
- 前端 Vitest 单 worker：9 个测试文件、28 项测试通过；
- Web 工作区 TypeScript 类型检查通过；
- BFF 与前端生产构建通过，Vite 仍报告既有主 chunk 超过 500 kB 的非阻断提示；
- Playwright 使用单 worker 串行完成 `desktop-chromium` 与 `mobile-chromium`，2 项完整链路测试通过；
- 浏览器跨总览、Runtime 和性能页只构造一个实例 `/stream`，未构造旧 `/metrics/stream`；
- Fixture 请求计数证明 BFF 只采样当前订阅 Topic，浏览器未发起旧总览和 Runtime 周期读取；
- 指标失败与恢复前后 `.live-monitor` 和图表顶部坐标保持不变，SVG 折线非空且从左向右推进；
- 源码轮询残留检索与 `git diff --check` 均无输出。

上述 Playwright 验收使用真实 Chromium、真实 BFF WebSocket 和版本化 Memory Server Fixture。它证明本仓库控制面链路与协议契约，不等同于外部真实 Rust Server、生产反向代理或 Kubernetes Ingress 的实机验证；发布到这些环境时仍必须验证 WebSocket Upgrade、Session Cookie、Origin 和超时配置。
