# 统一 Benchmark Harness

`jc-memory-benchmark-harness` 为六套 Benchmark 提供共同的 Case、执行器、指标和报告边界。Harness 不生成或伪造业务答案；实际 Recall 通过 `CaseExecutor` 注入，外部数据集不可用时应使用版本化 Fixture，并在上层报告中标记为 Smoke。

## Case 与执行器

`BenchmarkCase` 保存稳定的原始 Case ID、准备阶段的 `CognitiveFragment`、`RecallQuery`、结构化 `ExpectedAnswer` 和标签。`run_cases` 按输入顺序调用显式执行器，任意失败都会停止并保留执行器错误。`CaseResult` 除 `MemoryPacket` 和延迟外，还记录数据库字节、进程峰值内存、写入计数、写入耗时、巩固耗时和 Model Swap 结构化一致性。

## 指标约定

报告输出以下 19 个固定英文名称：

| 指标 | 计算约定 |
| --- | --- |
| `critical_memory_recall_at_5` | 前 5 个 Point 命中预期 Point 的比例 |
| `evidence_coverage` | Packet Evidence 命中预期 Evidence 的比例 |
| `current_state_accuracy` / `historical_state_accuracy` | 对应状态 Case 的结构化答案准确率 |
| `temporal_accuracy` / `causal_path_accuracy` | 预期 Point 是否出现在对应 Recall Path |
| `conflict_detection_rate` | Packet 是否检测到预期冲突 |
| `stale_memory_rate` | 返回 Point 中 `Superseded` 或 `Invalidated` 的比例 |
| `false_memory_rate` | 返回但不在预期 Point 集合中的比例 |
| `cross_tenant_leakage_rate` | Point 来源 Fragment 是否越过 Query Scope |
| `source_traceability` | 返回 Point 是否带有来源 Fragment |
| `abstention_accuracy` | 仅依据空 Point 与 `missing_evidence` 判断拒答 |
| `model_swap_consistency` | 注入执行器报告的结构化输出一致率 |
| `write_throughput` | Fragment/秒，按 Case 平均 |
| `recall_p50` / `recall_p95` / `recall_p99` | 延迟毫秒的 nearest-rank 百分位 |
| `storage_amplification` | 数据库字节数 / 写入 Fragment 数 |
| `consolidation_cost` | 巩固阶段平均毫秒数 |

没有可用分母时，指标值为 `0`，`sample_count` 同时记录为 `0`，不能把缺失样本当作成功。

## 报告

`BenchmarkReport::to_json` 生成 `jcmem.benchmark.v1` JSON；`to_markdown` 从同一结构生成中文 Markdown，避免两份结果分别计算。命令行程序用于把已验证的 JSON 转为 Markdown：

```bash
cargo run -p jc-memory-benchmark-harness -- report.json report.md
```

当前 Task 3 的测试只使用仓库内确定性 Fixture，不代表外部数据集、其他操作系统或生产负载的实机验证。
