# LongMemEval 与 LoCoMo Harness

本阶段提供两个数据集边界的统一 Runner，但当前仓库只提交 `PRIVATE-FIXTURE` Smoke 数据。外部数据集没有下载、没有被写入 Git，也不能把本机 Fixture 结果描述为原始数据集或生产效果。

## 版本和许可

每个目录都有 `manifest.json`，固定数据集名称、版本、许可、来源 URI、Fixture 路径和 SHA-256：

- `benchmarks/longmemeval/manifest.json`
- `benchmarks/locomo/manifest.json`

调用下载脚本必须显式接受 manifest 中的许可字符串。当前 `repository://` 来源只复制仓库内 Fixture；将来切换到外部 URL 时，脚本仍会先校验固定 URL 和 SHA-256。

```bash
scripts/benchmark/fetch-datasets.sh \
  --dataset longmemeval \
  --accept-license PRIVATE-FIXTURE \
  --cache-dir /tmp/jcmem-benchmark-cache
```

缓存目录 `.cache/benchmarks/` 已加入 `.gitignore`。脚本不会把缓存内容提交到仓库。

## Fixture 覆盖

两个 Fixture 都覆盖信息提取、跨 Session、时间查询、状态更新、拒答、单跳、多跳和因果失败标签。Rust Runner 会：

1. 校验 `jcmem.suite.v1`、数据集名称、版本、许可、来源 Digest、Case ID 和嵌套字段；
2. 将 Session 轮次转换为权威 `CognitiveFragment`，保留原始 Fragment ID、Session ID 和发生时间；
3. 转换为统一 `BenchmarkCase`，保留原始 Case ID 和结构化 `ExpectedAnswer`；
4. 通过调用方注入的 `CaseExecutor` 执行，并交给统一 Harness 生成版本化报告。

拒答只读取 `MemoryPacket.points` 和 `MemoryPacket.missing_evidence`，不匹配“我不知道”等模型措辞。缺少证据时，只有空 Point 且存在 `missing_evidence` 才算结构化拒答。

## 当前验证边界

`cargo test -p jc-memory-benchmark-harness --all-targets --all-features` 验证仓库 Fixture、Case 转换、拒答信号和注入式 Smoke Runner。当前未验证外部 LongMemEval/LoCoMo 原始数据、全量数据集许可、其他平台或生产负载。
