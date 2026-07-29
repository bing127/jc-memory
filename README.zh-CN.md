# 疆诚记忆

疆诚记忆是独立于具体大语言模型的 Memory-Native Agent Kernel 与神经认知记忆数据库。

当前分支正在实施 M1 基础可信存储，包括认知片段协议、不可变事件、SQLite WAL/FTS5 和内容寻址 BlobStore。M2-M6 的认知召回、Runtime、服务、SDK、模型与发布能力将在对应里程碑实现，当前不宣称已经可用。

## 开发检查

```bash
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo doc --workspace --no-deps
```

本项目目前为私有源码，保留所有权利。
