# Benchmark 方法与真实性门禁

本项目的 Benchmark 报告分为两层：Harness 负责生成 `jcmem.benchmark.v1` 指标 JSON；审计封套负责证明报告来自哪个提交、平台、配置和数据版本。审计封套使用 `jcmem.benchmark.audit.v1`，由 `scripts/benchmark/verify-report.sh` 校验。

## 必填证据

每份审计报告都必须包含：

- `git.commit` 和 `git.dirty`，用于定位源码和工作树状态；
- Rust、Python、OS、CPU 平台信息；
- 数据来源、规范化内容摘要及 SHA-256；
- 配置的规范化文本及 SHA-256；
- 带时区的开始/结束时间；
- `raw_counts` 原始 Case、Fragment、Evidence 和 Result 数量；
- `derived_metrics` 派生指标，未采集的指标必须使用 `null`；
- `smoke` 和 `non_release` 状态。

配置和数据 Digest 由验证器对报告中的 canonical 文本重新计算，不能只填写一个未经验证的摘要。对象不允许未知字段，重复 JSON 字段、NaN/Infinity 和缺少原始计数都会被拒绝。

## Dirty 与发布边界

Smoke 可以在 dirty 工作树上运行，但必须保留 `git.dirty=true`。非 Smoke 的长测在 dirty 工作树上默认拒绝；只有明确写入 `non_release=true` 才能作为非发布结果验证。`non_release` 不会把结果变成可发布基线。

## Smoke 与长测

运行仓库内可重复 Smoke：

```bash
scripts/benchmark/smoke.sh --output target/benchmarks/benchmark-smoke.json
```

该命令运行 Harness 和 Million Fragments 聚焦测试，按版本化 Fixture 计算数据摘要，并立即验证生成的审计 JSON。`raw_counts.results=0` 表示本命令没有执行生产 Recall 结果；这不会被伪装成模型性能。百万 Fragment 长测必须单独显式指定输出路径，结果不应提交到仓库。

## Markdown 来源

指标 Markdown 只能由已验证的 Harness JSON 生成：

```bash
cargo run -p jc-memory-benchmark-harness -- <report.json> <report.md>
```

不得手工修改 Markdown 中的数值，也不得把审计封套、Fixture 结果或 CI 配置描述为生产实机性能。
