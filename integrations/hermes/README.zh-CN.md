# Hermes 疆诚记忆插件

插件把 Hermes Desktop 的完整消息、工具调用、压缩前刷新和 Session 结束事件映射为疆诚记忆的权威 Fragment，并通过同一个 `JcMemoryProvider` 管理 Recall 与 Commit。Hermes 自带的语义记忆保持关闭，原始 Fragment、Event 和 EvidenceRef 由疆诚记忆负责保存。

## 配置边界

`memory.jc_memory` 只允许以下字段：

- `base_url`：无凭据的 HTTP(S) origin，不能包含路径、查询、片段或用户名密码；
- `namespace_id`：必填 UUID；`tenant_id`、`user_id`、`agent_id`、`project_id` 可为 UUID 或 `null`；
- `timeout_seconds`：`0.25..=300` 秒；
- `recall`：必须完整提供 `max_seeds`、`max_neighbors`、`max_hops`、`max_nodes`、`timeout_ms`，范围分别为 `1..=64`、`1..=128`、`1..=16`、`1..=1024`、`1..=60000`。

未知字段、空值、非 UUID、非法 URL 和越界预算在插件初始化前拒绝。API Key 不从 YAML 读取，只从 `JC_MEMORY_API_KEY` 环境变量注入；日志和 `repr` 不显示 Secret。

## 来源锁定

`hermes.lock.yaml` 是上游 Hermes Desktop 的唯一 Commit 来源。插件 Provider 中的 `HERMES_COMMIT` 必须与锁文件完全一致，契约测试会在 CI 中拒绝漂移。更新 Hermes 时先更新锁文件和测试，再运行完整插件测试。

## 本地验证

```bash
pytest -q integrations/hermes/tests
```

测试使用版本化 Fixture；只有连接真实 `jc-memory-server` 并完成写入后，才能声称跨 Session Recall 的实机链路通过。
