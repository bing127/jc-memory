# Embedded 绑定部署边界

疆诚记忆的 Embedded SDK 只负责绑定边界和 JSON/协议转换，业务授权、Scope 校验、来源保真和生命周期由 Rust Core 与 `CommandBus` 负责。远程模式是所有平台的回退路径。

## 当前支持矩阵

| 绑定 | 当前仓库状态 | 本机验证 | 未验证边界 |
| --- | --- | --- | --- |
| Node napi-rs | 已有 JSON Embedded 门面 | macOS Rust 构建和单测 | Windows/Linux 发布包与 Electron 平台二进制 |
| Python PyO3 | 已有同步/异步 Embedded 门面 | 本机可构建时运行 PyO3 测试 | manylinux、Windows wheel、PyPy |
| Go C ABI | 已隔离受控 C ABI | Rust ABI 单测 | Windows/Linux cgo 动态库和交叉链接 |
| Kotlin/JVM UniFFI | JVM Facade、loader 和 checksum 契约已存在 | JVM 单测 | Android native slices、JNI 设备运行、connected test |
| Kotlin/Android | Android Library、AAR、四类 ABI filters、ProGuard、Embedded facade 和 native slices | Release AAR 包含四类 `.so` | native slice 运行、设备/模拟器 connected test |
| Swift UniFFI | SwiftPM Embedded target、生成 binding 和本机 XCFramework | macOS ARM64 生成/编译/XCFramework | iOS device/simulator、macOS x64、签名和 dSYM 合并 |
| Java/JVM | 远程 CompletableFuture 与 Embedded Facade 已存在 | JVM 单测 | native binding 在目标 classloader 和平台上的实机加载 |

## 生命周期与校验

- Native loader 必须在 `System.load` 前验证绝对路径、普通文件和 SHA-256；摘要不匹配时不得创建 Rust handle。
- Embedded client 关闭顺序是先通知 Core shutdown，再释放绑定句柄；重复关闭保持幂等，关闭后拒绝新请求。
- JVM 不执行 native unload；classloader 级缓存只保证同一路径不会重复加载，不能替代进程生命周期管理。
- 所有绑定边界的 `unsafe` 必须局部隔离并有句柄、取消、过期 generation 和错误路径测试；核心 crate 保持 `unsafe_code = forbid`。

## 构建前提

使用 `scripts/build/build-java-uniffi.sh`、`scripts/build/build-xcframework.sh` 或 SDK 自己的构建命令前，必须具备对应 Rust target、JDK/Gradle、Swift toolchain 和平台 SDK。缺少工具链时只记录 `SKIP_UNAVAILABLE_TOOLCHAIN`，不得把远程 Fixture 结果描述为 native 实机验证。
