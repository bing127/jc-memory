# Recall 性能优化与可视化观测设计

- **状态：** 已确认
- **日期：** 2026-08-01
- **产品名称：** 疆诚记忆
- **英文名称：** Jiangcheng Memory
- **设计范围：** Recall 查询性能、端到端超时、可复现 HTTP 压测、静态压测图表、实时服务指标

## 1. 目标

本设计同时交付两类能力：

1. 优化当前 Recall 在约 2,000 条 Fragment 时约 13 req/s 饱和的问题，优先消除候选 Fragment 的串行 N+1 读取，并让超时覆盖完整 Recall 链路；
2. 提供一目了然的性能页面，先导入版本化压测 JSON 展示静态结果，再接入 Rust Server 的实时请求、错误、吞吐和延迟指标。

优化不得改变以下可信边界：

- 原始 Fragment、Event 和 EvidenceRef 仍是权威记录；
- 批量读取必须继续受精确 Scope 约束；
- Recall 查询前和返回前的 ACL 校验不得省略；
- Prediction、Inference 和 Plan 不得因性能处理变为 Verified Fact；
- 图表只展示实际采集数据，缺失字段显示“未采集”，不得使用估算值冒充实测值。

## 2. 已验证问题

本机隔离压测使用 `max_in_flight=64`、高限流阈值和约 2,000 条正式写入数据，得到以下稳定现象：

| 场景 | 吞吐 | P95 | P99 | 结果 |
| --- | ---: | ---: | ---: | --- |
| Observe，并发 32 | 296.77 req/s | 169.63 ms | 263.43 ms | 800/800 成功 |
| Recall，并发 8 | 13.06 req/s | 777.67 ms | 863.10 ms | 400/400 成功 |
| Recall，并发 32 | 13.48 req/s | 2,738.33 ms | 3,169.88 ms | 800/800 成功 |
| Recall，并发 64 | 13.00 req/s | 5,605.15 ms | 6,439.92 ms | 800/800 成功 |
| Stats，并发 64 | 733.35 req/s | 136.30 ms | 154.71 ms | 1000/1000 成功 |

根因链路为：

1. `MemoryEngine::execute_recall` 先执行 FTS 和 recent fragments 查询；
2. 合并候选后，最多 64 个 Fragment 逐个调用 `get_fragment_in_scope`；
3. 并发 Recall 因串行 N+1 读取竞争 SQLite 默认连接池，增加连接等待；
4. Recall 激活引擎内部才启动 `timeout_ms` 计时，前置数据库时间没有纳入用户预算。

## 3. 方案边界

采用“静态报告 + 实时监控”双模式，不要求第一版部署完整 Prometheus、Grafana 或 Loki 集群。

```text
可复现压测脚本 ──> jcmem.load-test.v1 JSON ──> 浏览器本地解析 ──> 静态报告图表

HTTP/gRPC 请求 ──> Rust Server 指标采集 ──> /metrics ──> BFF 归一化 ──> 实时图表

Recall 请求 ──> FTS/recent 候选 ──> Scope 批量读取 ──> 激活与渲染 ──> ACL 返回校验
                  \_________________ 同一端到端 Deadline _________________/
```

完整 Prometheus/Grafana 部署资源属于后续可选集成。当前指标名称和标签保持兼容，避免未来重新定义监控契约。

## 4. Recall 优化

### 4.1 Scope 批量读取

在存储 Repository 增加批量读取方法，输入精确 `MemoryScope` 和去重后的 `FragmentId` 集合，单次查询返回 Fragment。实现必须：

- 查询同时限定 `scope_key` 和候选 ID；
- 使用参数绑定构建 `IN` 查询，不拼接外部输入；
- 对每条记录复用现有解码、完整性和 Scope 校验；
- 返回结果按 ID 映射，不依赖数据库隐式顺序；
- 任一请求 ID 缺失或越界时维持当前失败语义，不静默吞掉记录。

Core 在合并 FTS 与 recent 候选后调用一次批量读取，再按候选顺序构造 Episode、Evidence 和 Point。Quarantine 语义保持不变；若压测证明其成为新的 N+1 热点，再单独增加批量审查查询，不在首轮提前扩大范围。

### 4.2 端到端 Deadline

`query.limits.timeout_ms` 从进入 Core Recall 后立即计算绝对 Deadline。FTS、recent、批量读取、Quarantine、激活和 Packet 渲染共享同一预算。

- 超时必须返回稳定的 Recall timeout 错误，而不是继续执行后返回 HTTP 200；
- 已开始的数据库 Future 由 Tokio 取消语义释放，不提交任何写入；
- `timeout_ms=0` 的既有校验语义保持不变；
- Explain 使用同一执行路径，因此自动获得相同边界。

### 4.3 SQLite 连接池

连接池大小继续通过配置传入，不把“提高连接数”作为唯一修复。默认值是否调整由优化后压测决定。配置支持 YAML 与环境变量覆盖，并在启动日志中只记录非敏感数值。

## 5. 可复现压测报告

新增 HTTP 压测工具，输出 `jcmem.load-test.v1` JSON。报告至少包含：

- Git commit、工作树是否干净、平台、开始和结束时间；
- Server/BFF 地址的脱敏标识，不保存 API Key；
- 场景名称、请求数、并发数、成功数、失败数和各状态码计数；
- 吞吐、P50、P95、P99、最大延迟；
- 请求超时、传输错误和是否为真实 Rust/SQLite 链路；
- 数据规模、`max_in_flight`、连接池大小和请求超时配置；
- 预热与正式测量必须分离。

压测脚本默认拒绝对非本机地址施加负载，外部地址必须显式开启，避免误压生产环境。报告解析器验证 schema version、有限数值和计数一致性。

## 6. 性能页面

控制台新增“性能”一级页面，提供两个 Tab。

### 6.1 压测报告

浏览器通过 File API 本地读取 `jcmem.load-test.v1` JSON，不上传文件。页面展示：

- 汇总指标：总请求、成功率、总吞吐、最高 P99；
- 延迟分位数分组柱状图：各场景 P50/P95/P99；
- 吞吐与并发关系折线图；
- HTTP 状态分布图，突出 429 背压；
- 场景明细表和测量边界。

图表使用固定容器尺寸和响应式约束；颜色同时配合文字、图例和数值，不仅依赖颜色表达状态。

### 6.2 实时监控

页面按固定周期从 BFF 获取当前实例指标，保留浏览器会话内最近一段时间的环形样本：

- 每秒请求数；
- 成功率和错误率；
- 当前 in-flight；
- Recall、Observe、Stats 的 P50/P95/P99；
- 429 数量和超时数量。

实例离线、指标格式不兼容或采样中断时停止绘制连线，并展示最后成功采样时间，不把缺口补成零。

## 7. 指标契约

Rust Server 在现有 `/metrics` 增加低基数指标。标签只允许固定 operation、transport 和 status class，不记录 API Key、Scope、Tenant、查询文本或 ID。

建议指标：

```text
jc_memory_requests_total
jc_memory_request_duration_seconds
jc_memory_requests_in_flight
jc_memory_rate_limited_total
jc_memory_timeouts_total
jc_memory_server_ready
```

BFF 将 Prometheus 文本归一化为前端契约，前端不直接接触实例 API Key。Histogram 分位数由有边界的 Bucket 或服务端快照计算，并明确显示采样窗口。

## 8. 测试与验收

行为变更严格采用 TDD：

1. 存储测试先证明多个候选当前需要批量 API，并覆盖跨 Scope、缺失 ID 和顺序无关；
2. Core 测试先证明数据库阶段超过预算时当前不会按端到端 Deadline 失败；
3. Server 测试先证明 `/metrics` 当前缺少请求量、延迟、in-flight、429 和 timeout；
4. 压测报告解析测试覆盖合法报告、未知版本、非有限数值和计数不一致；
5. 前端测试覆盖两个 Tab、缺失字段、图例、空状态、错误状态和响应式容器；
6. Playwright 验证报告导入和实时指标主路径；
7. 优化前后使用同一数据库、配置、预热和请求集合重复 Recall 并发 8/32/64。

首轮验收目标：

- 候选 Fragment 查询从最多 64 次串行读取降低为一次批量读取；
- Recall 并发 32 的吞吐高于当前 13.48 req/s，且 P95/P99 不劣化；
- 超过端到端预算的 Recall 返回 timeout，而不是延迟数秒后返回 200；
- 压测 JSON 可在性能页面生成完整静态图表；
- 实时页面可显示真实请求、延迟、错误和背压数据；
- 无高基数或敏感信息进入指标。

性能提升不预设虚构百分比。最终以相同条件下的前后报告为准。

## 9. 非目标与未验证边界

本轮不包括：

- 将 SQLite 替换为其他数据库；
- 分布式追踪、集中日志和告警通知；
- 完整 Prometheus/Grafana/Loki 集群部署；
- 百万条真实 SQLite/FTS 数据加载；
- Windows、Linux、移动端或多节点生产压力验证；
- 外部 LLM、KMS/HSM 或真实云平台性能验证。

这些能力不得因图表存在而描述为已经完成。
