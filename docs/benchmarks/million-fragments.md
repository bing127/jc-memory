# Million Fragments 可恢复压力测试

Million Fragments 用于验证大规模 Fragment 生成过程的流式内存边界、批次提交和崩溃恢复。实现位于 `benchmarks/million-fragments`，生成器只保留 Seed、剩余数量和当前序号，不把全部 Fragment 放入内存。

## Smoke

先运行一千条 Fixture，确认 Checkpoint 往返、配置 Digest 和报告原始计数：

```bash
scripts/benchmark/million-fragments.sh \
  --fragments 1000 \
  --output target/benchmarks/million-fragments-smoke.json
```

输出 JSON 使用 `jcmem.million-fragments.v1` Schema。Checkpoint 会写入同目录的 `.checkpoint.json` 文件，批次写入采用临时文件、`sync_all` 和原子替换；恢复时必须匹配 `config_digest`，并从最后一个已提交批次的下一个 sequence 继续。

## Resume 与崩溃注入

库测试中的故障注入会在固定批次后停止，保留此前已经提交的 Checkpoint；随后 `resume_smoke` 校验配置并补齐剩余批次。配置 Digest 被篡改、sequence 不连续或原始计数不一致都会拒绝继续或拒绝报告。

## 百万长测

百万规模必须显式提供报告路径，避免把长测结果误写到当前目录：

```bash
scripts/benchmark/million-fragments.sh \
  --fragments 1000000 \
  --output /tmp/jcmem-million-fragments.json
```

CLI 会测量从生成开始到最后一个 Checkpoint 提交完成的实际写入吞吐，并写入 `write_throughput_per_second`。索引耗时、数据库/Blob 大小、Recall 百分位、局部图加载、巩固、RSS 和崩溃恢复等字段在尚未接入真实采集器时保持 `null`，不能据此宣称生产性能。

## 验证边界

- 仓库 Smoke 和单元测试使用确定性 Fixture，不等同于生产数据库或外部数据集长测。
- 当前实现验证了流式生成、批次 Checkpoint、原子写入、Digest 校验和恢复序列；百万长测是否完成必须以实际命令退出码及生成的 JSON 为准。
- 报告中的原始计数由 `requested_fragments` 和 `generated_fragments` 同时记录并校验，派生吞吐不会覆盖权威原始记录。
