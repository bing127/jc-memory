# M3 威胁模型

本文限定于当前本地 Rust、SQLite 和文件系统实现。它不把未来的网络服务、SDK、外部模型或发布基础设施当作已存在的防护面。

## 1. 资产与信任边界

受保护资产包括原始 `Fragment`、`Event`、`EvidenceRef`、Blob、Scope 身份、治理审计、Snapshot、派生图和 Job Checkpoint。权威记录优先级高于摘要、Embedding、Point、Edge、Surface、巩固产物和 Recall Packet。

主要边界是：

- 调用方到 `MemoryEngine`/`GovernanceService` 的 `AccessContext`；
- 一个精确 `MemoryScope` 到另一个 Scope；
- 不可信正文和 ToolResult 到 Quarantine 准入；
- 权威三表到可重建派生层；
- SQLite 事务到 Blob 文件系统；
- Snapshot staging 目录到当前数据目录；
- 巩固 Handler 到受限 `BatchContext`。

当前实现信任宿主进程、操作系统账户和数据目录权限。能任意修改进程内存、替换二进制或直接写数据库文件的攻击者不受 Rust API 层 ACL 约束。

## 2. 威胁与控制

| 威胁 | M3 控制 | 剩余风险 |
| --- | --- | --- |
| 跨 Tenant/Project 数据泄漏 | 默认拒绝 ACL、精确 Grant、查询前授权、Scope 绑定 SQL、Packet 返回前来源复核 | 调用方伪造可信 `AccessContext`；本地文件被绕过 API 直接读取 |
| Prompt Injection 进入默认 Recall | 版本化离线规则、Quarantine、默认 Recall 隐藏、Admin 审查追加记录 | 规则有漏报和误报；召回文本仍应被上层当作数据 |
| Memory Poisoning 与伪造 ToolResult | 重复争议、跨 Scope Evidence、自验证 ToolResult 检测；模型输出强制非 Verified | 新型表达可能不命中规则；人工 Release 仍需外部治理 |
| Prediction/Inference/Plan 升级为事实 | 协议校验、Fallback 分类和 Semantic Promotion 独立 Verified 来源门槛 | 错误但形式上 Verified 的上游证据仍需纠错流程 |
| 巩固覆盖权威记录 | `BatchContext` 不暴露 Transaction；权威表写入静态拒绝；派生引用逐项核对 Scope | 直接绕过 crate API 写 SQLite 不在此边界内 |
| Job 崩溃、重复执行或资源耗尽 | 每批事务、提交后推进 cursor、持久化 Checkpoint/Lease、幂等 ID、重试上限、批量/CPU/IO/墙钟预算、取消 | CPU 预算是批次 elapsed 的保守代理，不是操作系统 CPU 配额；无全局调度配额 |
| Snapshot 篡改或错 Scope Restore | Manifest 与文件 SHA-256、Schema/Scope/Blob 清单、SQLite quick/integrity check、隔离校验和目录切换回滚 | SHA-256 提供完整性检测但 Manifest 未签名；拥有文件写权限者可整体替换自洽 Snapshot |
| 未授权执行或干扰 Retention | 计划与执行均要求精确 Scope 的 `Delete` Grant，执行在读取或改变计划状态前重新授权 | 调用方仍需保护计划对象和本地数据库访问 |
| Hard Delete 留下引用或正文审计 | 排他锁、事务清理、Blob 暂存、派生层全 Scope 清理、无正文摘要凭证、重建排队 | 文件系统安全擦除、外部备份和副本清理未实现 |
| 密钥泄露 | `KeyProvider` 抽象、`Zeroizing`、Debug 脱敏、轮换元数据与材料分离 | 当前仅进程内 Provider；交换区、崩溃转储、KMS/HSM 不在 M3 验证范围 |
| Neural Provider 不可用或不确定 | 八个 Trait 的本地确定性 Fallback、Manifest 校验和、批量限制、超时与三态熔断 | M3 未接入外部 Provider，也未验证模型质量或对抗鲁棒性 |

## 3. 安全不变量

- 原始 Fragment、Event 和 EvidenceRef 不得被摘要、Embedding、图关系、Surface 或巩固产物覆盖。
- Prediction、Inference 和 Plan 不得直接成为 Verified Fact。
- Recall 在检索前和返回前都必须授权；越权引用使整个结果失败。
- Quarantine 改变可见性和准入状态，不抹除原始证据。
- 巩固只通过白名单派生写入，并保留来源 Evidence 或事件游标。
- Snapshot/Restore 在发布前校验；派生层损坏时从权威记录重建。
- Hard Delete 的成功审计不保存正文，且只有数据库与 Blob 清理完成后才能标记成功。

## 4. 验证与未覆盖项

M3 验收在 Darwin 25.6.0、Apple Silicon arm64、Rust 1.97.1 上运行本地 Fixture。覆盖跨 Tenant、注入/污染、确定性 Fallback、Job 恢复与预算、经验学习、Snapshot/Restore、派生重建和 Hard Delete。

这些结果不构成 Linux、Windows、macOS x64、移动端、远程身份系统、网络 TLS、KMS/HSM、云备份、外部模型、渗透测试、模糊测试、Benchmark 或生产负载验证。
