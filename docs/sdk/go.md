# Go SDK 与 C ABI

`sdk/go` 提供接收 `context.Context` 的远程 HTTP 客户端，使用标准库 `net/http`，并把协议 Envelope 的命令和 payload 保留为 `json.RawMessage`，因此 optional 字段不会被零值覆盖。

## 验证

```bash
(cd sdk/go && go test ./...)
(cd sdk/go && go test -race ./...)
cargo test -p jc-memory-ffi
cargo clippy -p jc-memory-ffi --all-targets --all-features -- -D warnings
```

远程错误通过 `memory.Code(err)` 和 `memory.RequestID(err)` 读取稳定分类与请求标识。Context 取消映射为 `cancelled`，deadline 映射为 `deadline_exceeded`。

## C ABI

`crates/memory-ffi/include/jcmemory.h` 只暴露 `create`、`destroy`、`execute`、`cancel`、`free_buffer` 及带 token 的执行变体。Rust 分配的输出必须使用返回的精确长度交还 `free_buffer`；调用方不得在 `destroy` 后复用 handle。generation 和 cancellation token 用于拒绝过期取消请求，不能替代调用方的句柄生命周期管理。

当前仓库只验证 macOS 主机上的 Rust ABI 单测，Windows/Linux 动态库和 Go cgo 链接由后续 CI 构建矩阵验证，不能把本地 Fixture 结果当成这些平台的实机验证。
