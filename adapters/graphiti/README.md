# Graphiti Adapter

该 crate 将 Graphiti 的 Node 和 temporal Edge 映射到疆诚记忆的 `CognitivePoint`、`GraphitiEntity` 和 `EdgeCandidate`。它只做来源映射，不直接写入权威 Fragment/Event，也不绕过 Core ACL。

## 来源与时间保真

- Node 必须携带真实的外部 ID、`FragmentId`、Point ID、Scope、发生时间和来源置信度；适配器不会伪造 Fragment。
- Edge 的 `valid_at` 和 `invalid_at` 原样映射为 `valid_from` 和 `valid_to`，失效时间必须晚于生效时间。
- Edge 保留 `evidence_fragment_id`、外部 ID、`source_version` 和可选 `model_version_id`。
- `inferred` 关系始终保持 `human_confirmed=false`、`tool_verified=false`，不能自动升级为已验证事实。

## 配置

复制 `config.example.yaml` 后注入实际的来源版本。仓库中的测试只使用版本化 Fixture，没有连接 Graphiti 实例。

## 验证边界

当前契约测试覆盖 Node、temporal Edge、推断边和端点约束；Fixture 结果不代表 Graphiti 服务实机验证。
