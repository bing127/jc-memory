# 疆诚记忆（Jiangcheng Memory）

疆诚记忆是独立于具体大语言模型的 Memory-Native Agent Kernel 与神经认知记忆数据库。项目当前为私有专有源码，暂不计划开源发布。

当前代码已完成 M1 基础可信存储、M2 认知图/有界召回和 M3 巩固/安全治理：不可变 Fragment/Event/EvidenceRef、SQLite WAL/FTS5、内容寻址 Blob、Point/Edge/Surface、十种 Recall、Memory Packet、可恢复巩固 Job、确定性离线 Neural Fallback、Quarantine、Snapshot/Restore、Retention、Export 和 Hard Delete。

Core 当前实际接通 Observe、Commit、Fragment lookup、Health、Recall、Explain、Stats、Mutation、Quarantine 审查和 Snapshot/Restore/Rebuild，且这些操作显式接收 `AccessContext` 并采用默认拒绝 ACL。Recall 在检索前和返回前执行授权。SQLite FTS5 是实际执行路径；跨措辞候选使用确定性词项和稀疏字符特征，不是外部 Embedding。

M3 的巩固与治理能力目前以本地 Rust crate 和嵌入式 API 提供。`Operation::Consolidate` 尚未接入 Core Job 提交门面；状态、语义与 Surface 尚未全部汇入持久化 Packet；进程内 Mutation 图尚未在 Core 重启时自动恢复。派生重建的 FTS/Point 路径真实执行，其余模型相关阶段只验证本地契约。

中文文档入口：

- [详细 README 与五分钟开始](README.zh-CN.md)
- [Point、Edge 与 Surface](docs/concepts/point-edge-surface.md)
- [模式分离与有界模式补全](docs/concepts/pattern-completion.md)
- [Memory Packet 协议](docs/protocol/memory-packet.md)
- [巩固与派生重建](docs/architecture/consolidation.md)
- [访问控制](docs/security/access-control.md)
- [Quarantine 与污染防护](docs/security/quarantine.md)
- [Retention 与 Hard Delete](docs/security/retention.md)
- [M3 威胁模型](docs/security/threat-model.md)
- [Agent Runtime](docs/runtime.zh-CN.md)
- [Capability、Model 与 Tool](docs/capabilities.zh-CN.md)
- [HTTP 与 gRPC Server](docs/server.zh-CN.md)
- [本地 Sidecar](docs/sidecar.zh-CN.md)
- [MCP Server](docs/mcp.zh-CN.md)
- [故障恢复](docs/recovery.zh-CN.md)
- [Server 配置参考](docs/configuration.zh-CN.md)
- [Docker 与 Kubernetes 部署指南](docs/deployment.zh-CN.md)
- [架构决策](docs/adr/)
- [产品原始需求](docs/product/original-requirements.zh-CN.md)

运行 M3 聚焦验收：

```bash
CARGO_INCREMENTAL=0 cargo test -p jc-memory-core --test tenant_isolation --test poisoning --test snapshot_restore --offline
CARGO_INCREMENTAL=0 cargo test -p jc-memory-consolidation --test conservative_learning --test job_recovery --offline
CARGO_INCREMENTAL=0 cargo test -p jc-memory-security --test hard_delete --offline
CARGO_INCREMENTAL=0 cargo test -p jc-memory-neural --test fallback --offline
```

保留所有权利。当前仓库不声明外部服务、跨平台实机、SDK、模型训练、Benchmark 或公开发布能力已经完成。
