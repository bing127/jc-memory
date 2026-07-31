# Java/JVM SDK

`sdk/java` 提供基于 JDK 17 `HttpClient` 的 Java 远程客户端。客户端使用与其他 SDK 相同的 JSON Request/Response Envelope，保留协议字段和原始响应内容，不在 Java 层复制记忆核心逻辑。

## 当前能力

- `JiangchengMemoryClient` 实现 `AutoCloseable`，关闭后请求会返回 `internal` 错误。
- `observe`、`commit`、`recall`、`stats`、`correct`、`forget`、`explain` 返回 `CompletableFuture<MemoryResponse>`。
- 对应的 `*Blocking` 方法明确阻塞等待异步结果，并直接抛出 `JiangchengMemoryError`，不会额外包裹业务错误。
- 请求支持可选 `Authorization: Bearer <apiKey>`，不会把 API Key 写入日志或 Fixture。
- 构造函数可传入 `Duration deadline`；JDK HTTP 超时会映射为 `deadline_exceeded`。
- 非 2xx 响应按 JSON `code`、`message`、`request_id` 映射为 `JiangchengMemoryError`；缺失 `code` 时根据 HTTP 状态码使用稳定分类。

## 快速开始

```java
import com.jiangcheng.memory.JiangchengMemoryClient;
import com.jiangcheng.memory.MemoryResponse;

try (var client = new JiangchengMemoryClient("http://127.0.0.1:8080", "local-token")) {
    MemoryResponse response = client.recallBlocking("{\"api_version\":\"v1\"}");
    System.out.println(response.json());
}
```

异步调用不阻塞当前线程：

```java
var future = client.recall(requestJson);
future.thenAccept(response -> consume(response.json()))
      .exceptionally(error -> {
          // 统一处理 JiangchengMemoryError 或传输错误
          return null;
      });
```

需要自定义 deadline 时，使用完整构造函数：

```java
var client = new JiangchengMemoryClient(
        "http://127.0.0.1:8080",
        apiKey,
        java.net.http.HttpClient.newHttpClient(),
        java.time.Duration.ofSeconds(5));
```

## 构建与测试

仓库使用 Gradle Kotlin DSL，测试不访问外部网络，而是通过 `sdk/contracts/fixtures/recall-exact.json` 和本地 `HttpServer` 验证协议、鉴权、错误、取消及同步 facade：

```bash
gradle -p sdk/java test --no-daemon
```

当前主机已验证 4 个 JVM 测试。测试覆盖不等同于 Android、iOS/macOS 或其他平台的实机验证。

## 边界与后续工作

当前实现是 HTTP 远程 SDK，不声明 gRPC Stub 已生成；仓库现有服务契约和其他远程 SDK 仍以 JSON Envelope 为共同 Fixture 真相源。UniFFI/JVM Embedded、平台 native loader、校验和及 classloader 生命周期管理需要 `memory-uniffi` crate 和独立 CI 构建后再实现，本提交不把它们标记为完成。
