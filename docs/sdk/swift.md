# Swift SDK

当前已落地 Swift actor 远程客户端：`JiangchengMemoryClient` 使用 `URLSession.data(for:)` 和 `async/await`，提供 `observe`、`recall`、`commit`、`correct`、`explain`、`forget`、`stats` 和 `close`。调用方取消保留 `CancellationError`，HTTP 错误映射为 `JiangchengMemoryError`。

```bash
swift test --package-path sdk/swift
```

测试使用 URLProtocol Fixture，不访问外部网络。当前验证目标是 macOS ARM64；iOS device/simulator slices、UniFFI 生成的 Embedded handle 和 XCFramework 仍待 `memory-uniffi` crate 完成后由 CI 验证。
