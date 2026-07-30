# 疆诚记忆（Jiangcheng Memory）

疆诚记忆是独立于具体大语言模型的 Memory-Native Agent Kernel 与神经认知记忆数据库。项目当前为私有专有源码，暂不计划开源发布。

当前代码已完成 M1 基础可信存储与 M2 认知图/有界召回：不可变 Fragment/Event/EvidenceRef、SQLite WAL/FTS5、内容寻址 Blob、Point/Edge/Surface、状态版本、证据约束的语义记忆、十种 Recall、Explain、Memory Packet，以及追加式 Correct/Reinforce/Weaken/Supersede/Contradict/Forget。

Core 当前实际接通 Observe、Fragment lookup、Health、Recall、Explain、Stats 和上述 Mutation。SQLite FTS5 是实际执行路径；跨措辞候选使用确定性词项和稀疏字符特征，不是外部 Embedding。状态、语义与 Surface 模块已有独立领域实现，但尚未全部汇入 Core 的持久化 Packet；Mutation 派生图目前保存在进程内，重启后的自动重建仍未接通。

中文文档入口：

- [详细 README 与五分钟开始](README.zh-CN.md)
- [Point、Edge 与 Surface](docs/concepts/point-edge-surface.md)
- [模式分离与有界模式补全](docs/concepts/pattern-completion.md)
- [Memory Packet 协议](docs/protocol/memory-packet.md)
- [架构决策](docs/adr/)
- [产品原始需求](docs/product/original-requirements.zh-CN.md)

运行 M2 Core 验收：

```bash
CARGO_INCREMENTAL=0 cargo test -p jc-memory-core
```

保留所有权利。当前仓库不声明外部服务、跨平台实机、SDK、模型训练、Benchmark 或公开发布能力已经完成。
