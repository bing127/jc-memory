# Java/JVM UniFFI Embedded 设计

- **状态：** 已获用户确认，待实施计划
- **日期：** 2026-07-31
- **范围：** M5 Task 7 Java/JVM Embedded

## 1. 目标与边界

为 Java SDK 增加真实的本地 Embedded 能力：Rust Core 只保留一套实现，Java 通过 UniFFI 生成的 JVM binding 调用它，不复制 JNI 业务逻辑。远程 HTTP 客户端的行为和协议 Fixture 不改变。

本次交付覆盖：

- 新增 `crates/memory-uniffi`，公开 `JcMemoryEngine` 和稳定 JSON Command API；
- 生成 Kotlin/JVM binding，并由 Java SDK 通过小型 Java facade 调用；
- native library 按显式路径或 classpath resource 加载；
- 加载前校验 SHA-256，校验失败禁止 `System.load`；
- 以 classloader 为边界缓存已加载库，重复加载同一校验和可复用，关闭 facade 不卸载 JVM native library；
- 对 native handle 的关闭、重复关闭和调用后生命周期提供测试。

本次不宣称完成 Android AAR、iOS/macOS XCFramework、Windows/Linux/macOS 全平台发布矩阵或移动设备实机测试。当前主机只验证 macOS ARM64 JVM native 产物和 Rust binding 单测；其他平台由后续 CI 验证。

## 2. 分层设计

```text
Java EmbeddedMemoryClient
        |
Kotlin/JVM UniFFI generated binding
        |
memory-uniffi JcMemoryEngine
        |
memory-core EmbeddedCommandBus
```

`memory-uniffi` 只负责参数转换、句柄生命周期和错误转换。它不得修改 Fragment、Event、EvidenceRef 的权威语义，也不得把 Prediction、Inference 或 Plan 提升为 Verified Fact。请求和响应继续使用 `memory-protocol` 的 JSON Envelope，便于与 HTTP、Node、Python 和 Go Fixture 对照。

## 3. Native Loader

Java loader 提供两个入口：

1. 调用方传入 native library 文件路径和期望 SHA-256；
2. 调用方传入 classpath resource、目标目录和期望 SHA-256，loader 将 resource 原子复制到临时文件后校验并加载。

loader 的规则：

- SHA-256 使用 Java 标准库计算，比较采用大小写不敏感的十六进制规范化；
- 文件不存在、读取失败、摘要不匹配或路径不是普通文件时，返回结构化 `NativeLibraryLoadException`；
- 摘要校验完成前不得调用 `System.load`；
- 以 `ClassLoader` 和绝对路径为缓存键，同一 loader 重复加载相同库只执行一次；
- 同一 classloader 下相同路径但摘要不同直接失败，防止库替换后静默复用；
- loader 不删除正在使用的文件，不提供伪造的 unload；JVM 本身不支持安全卸载 native library。

## 4. 生命周期与错误

`EmbeddedMemoryClient` 持有生成 binding 返回的 engine handle，并实现 `AutoCloseable`。`close()` 幂等；关闭后所有操作返回稳定的 `internal`/`closed` 错误。native binding 只允许在 handle 存活期间调用，Java facade 不缓存裸指针或跨关闭保存请求缓冲区。

Rust 错误转换为稳定错误码和中文消息；Java loader 错误与 Core 业务错误分开，便于诊断 checksum、平台库和请求语义问题。取消只取消当前请求，不关闭共享 engine。

## 5. 测试与验收

- Rust：UniFFI crate 单测覆盖 JSON round-trip、空输入、关闭后调用和错误映射；运行 `cargo test -p jc-memory-uniffi`、fmt、严格 Clippy 和 doc。
- JVM：测试先覆盖 loader 缺失文件、摘要错误、同 classloader 缓存、跨 classloader 隔离和 close 幂等，再覆盖 Fixture Embedded recall。
- 集成：构建当前主机 native library，计算真实 SHA-256，通过 JVM loader 加载后执行 `recall-exact.json`，比较 response canonical JSON。
- 未具备 Android/iOS/Windows/Linux 产物时，使用版本化 Fixture 和结构契约测试，不把本机结果描述为其他平台实机验证。

## 6. 发布与后续

当前提交只把 macOS ARM64 JVM Embedded 标记为已验证。后续 CI 需要补充 Linux、Windows、macOS x64/ARM64 的 native library 构建、签名或校验和清单，以及 Kotlin Android AAR、consumer ProGuard 和 connected test。所有新增提交继续采用中文 Conventional Commit 标题和中文正文。
