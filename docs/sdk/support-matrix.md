# SDK 与 Adapter 支持矩阵

## SDK

| SDK | 远程模式 | Embedded 模式 | 当前验证边界 |
| --- | --- | --- | --- |
| Node.js / Electron | REST JSON Envelope | napi-rs JSON 门面 | macOS 本机；平台包由 CI 生成 |
| Python | 同步/异步 HTTP | PyO3 同步/异步门面 | 本机 Python 与 Rust 测试 |
| Go | `net/http` 与 Context | 受控 C ABI | Rust ABI；跨平台 cgo 待 CI |
| Kotlin | JDK HttpClient + Coroutine | UniFFI/JNI 规划边界 | JVM 测试；Android 设备待后续 |
| Swift | `URLSession` actor | UniFFI/XCFramework 规划边界 | macOS ARM64 测试 |
| Java | JDK HttpClient + CompletableFuture | JVM Facade 与 native loader 契约 | JVM 测试；native 实机待绑定生成 |

## Adapter

| Adapter | 来源保留内容 | 当前验证 |
| --- | --- | --- |
| Hermes | turn、Fragment、Candidate validity、source version | 版本化 Fixture + Embedded Core |
| OpenClaw | 路径、内容哈希、heading、行范围、原始 ID | Markdown Fixture + Embedded Core |
| Mem0 | legacy ID、metadata、时间、旧评分、manifest | Add/Search/Export round-trip Fixture |
| Graphiti | Node/Entity、Edge、valid_at/invalid_at、evidence、model version | temporal/inferred Edge Fixture |
| Dify | user/conversation/app/workflow/trace 外部 ID、Scope、MemoryPacket | HTTP/MCP 节点和失败边界 Fixture |

所有 Adapter 都只做映射和命令桥接；原始 Fragment、Event、EvidenceRef 仍是权威记录，Prediction、Inference、Plan 和 ToolResult 不会自动变成 Verified Fact。Fixture 通过不等于外部平台实机验证。
