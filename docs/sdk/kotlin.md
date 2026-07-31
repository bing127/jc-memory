# Kotlin / Android SDK

当前已落地并验证 Kotlin/JVM 远程客户端：`JiangchengMemoryClient` 使用 JDK `HttpClient`、Kotlin Coroutine 和 `Flow`，提供 `observe`、`recall`、`commit`、`correct`、`explain`、`forget`、`stats` 和 `close`。

```bash
gradle -p sdk/kotlin test --no-daemon
```

所有远程方法都是 `suspend`；`recallFlow` 提供单元素可取消 Flow。HTTP 错误统一为包含 `code`、`requestId`、`details` 和状态码的 `JiangchengMemoryError`。取消不会被包装成普通业务错误。

仓库现在提供独立的 `sdk/kotlin/android` Android Library：已配置 Debug/Release AAR、`arm64-v8a`/`armeabi-v7a`/`x86_64`/`x86` ABI filters、consumer ProGuard、UniFFI `JcMemoryEngine` 和 `AndroidEmbeddedMemoryClient`。当前主机已生成四类 native slices，并通过 `gradle -p sdk/kotlin/android verifyNativeInputs assembleRelease --no-daemon`；AAR 已检查包含四类 `.so`。

使用 `scripts/build/build-android-uniffi.sh` 需要 Android NDK 和四个 Rust Android targets；脚本会 fail closed，不下载工具链或生成空 slice。`connectedAndroidTest` 仍需真实设备/模拟器，不能把 AAR 构建描述为 Android 实机验证。
