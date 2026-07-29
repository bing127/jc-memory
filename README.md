# Jiangcheng Memory

Jiangcheng Memory is a model-independent, memory-native agent kernel and neurocognitive memory database.

The current code implements the M1 trusted-storage foundation: typed cognitive fragments, immutable events, transactional SQLite WAL/FTS5 persistence, content-addressed blobs, integrity audit, and recovery primitives.

The embedded API currently supports Observe, fragment lookup by ID, and Health. Cognitive recall, graph and consolidation behavior, runtime services, SDKs, adapters, models, benchmarks, and release artifacts remain scoped to M2-M6 and are not claimed as available. Reserved schema tables do not imply implemented product behavior.

Chinese is the primary documentation language:

- [中文 README 与五分钟开始](README.zh-CN.md)
- [M1 storage architecture (Chinese)](docs/architecture/storage.md)
- [Architecture decisions (Chinese)](docs/adr/)

Run the executable M1 embedded-storage example through its integration test:

```bash
cargo test -p jc-memory-core --test observe_persistence
```

This project is proprietary source and is not currently planned for open-source publication. All rights reserved.
