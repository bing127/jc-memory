# Kotlin / Android SDK

当前已落地并验证 Kotlin/JVM 远程客户端：`JiangchengMemoryClient` 使用 JDK `HttpClient`、Kotlin Coroutine 和 `Flow`，提供 `observe`、`recall`、`commit`、`correct`、`explain`、`forget`、`stats` 和 `close`。

```bash
gradle -p sdk/kotlin test --no-daemon
```

所有远程方法都是 `suspend`；`recallFlow` 提供单元素可取消 Flow。HTTP 错误统一为包含 `code`、`requestId`、`details` 和状态码的 `JiangchengMemoryError`。取消不会被包装成普通业务错误。

Android AAR、UniFFI/JNI 本地引擎、ABI filters、consumer ProGuard 和 `connectedAndroidTest` 尚未在当前提交中声明为完成。原因是仓库尚无 `memory-uniffi` crate，且本机没有可用的 Android 设备/模拟器运行 connected test；这些边界保留给后续绑定任务和 CI，并不把 JVM 测试当作 Android 实机验证。
