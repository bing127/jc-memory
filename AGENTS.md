# 仓库开发规则

- 产品文档以中文为主；代码标识、协议字段和命令使用英文。
- 原始 Fragment、Event 和 EvidenceRef 是权威记录，不得被摘要、Embedding、图关系或 Surface 覆盖。
- LLM 的 Prediction、Inference 和 Plan 不得直接写为 Verified Fact。
- 核心逻辑禁止 `unsafe`；绑定边界确需使用时必须隔离、说明不变量并测试。
- 行为变更采用 TDD：先运行会因目标行为缺失而失败的测试，再实现最小代码。
- 每个 Task 运行聚焦测试；提交前运行适用的 fmt、clippy、test、doc 和 `git diff --check`。
- 保留工作树中不属于当前任务的用户修改，不重置或覆盖无关文件。
- 外部平台不可用时使用版本化 Fixture 和契约测试，不把 Fixture 结果描述为实机验证。
