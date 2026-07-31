# Swift SDK

当前已落地 Swift actor 远程客户端：`JiangchengMemoryClient` 使用 `URLSession.data(for:)` 和 `async/await`，提供 `observe`、`recall`、`commit`、`correct`、`explain`、`forget`、`stats` 和 `close`。调用方取消保留 `CancellationError`，HTTP 错误映射为 `JiangchengMemoryError`。

```bash
swift test --package-path sdk/swift
```

测试使用 URLProtocol Fixture，不访问外部网络。`scripts/build/build-swift-uniffi.sh` 会使用锁定 UniFFI 版本生成 Swift binding、FFI header/modulemap；SwiftPM 已提供独立的 `JiangchengMemoryEmbedded` target，当前 macOS ARM64 可编译并通过生成契约测试。

`scripts/build/build-xcframework.sh` 可在 macOS 当前主机准备本机 Framework 并生成 `JiangchengMemory.xcframework`；iOS device/simulator slices、签名发布和 dSYM 合并仍需对应 Xcode SDK/CI，不能把本机 macOS 结果描述为 Apple 全平台实机验证。
