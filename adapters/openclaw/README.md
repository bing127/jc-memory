# OpenClaw Adapter

该 crate 将 OpenClaw `Memory.md` 导入疆诚记忆，并为 Hook/MCP 调用提供同一个 Core Recall 入口。适配器只负责文件来源映射，不自行维护第二套记忆存储。

## 来源保真

每个 Markdown 区段都保存以下信息：

- 原文件路径；
- 完整文件内容的 SHA-256；
- heading；
- 1-based 起止行；
- `memory.md/v1` 来源契约版本。

Fragment ID 根据上述 Provenance 和区段原文确定性生成。同一文件版本重复导入返回 `created=false`；文件内容变化会生成新的 Fragment，旧 Fragment/Event 不会被覆盖或删除。

显式的 `Preferences` 和 `Constraints` heading 分别映射为 `Preference` 与 `Constraint`，其他区段映射为 `Fact`；所有导入数据始终是 `Unverified`，不会因为类型映射而成为 Verified Fact。

## 配置与调用

配置示例见 `config.example.yaml`。调用方必须提供包含 `Mutate` 和 `Recall` 精确 Grant 的 `AccessContext`。`import_markdown`、`hook_recall` 与 `mcp_recall` 都只调用注入的 `CommandBus`。

## 验证边界

当前契约测试使用版本化 `Memory.md` Fixture 和真实 `EmbeddedCommandBus`。测试没有启动 OpenClaw 运行时，因此不得描述为 OpenClaw Hook/MCP 实机验证。
