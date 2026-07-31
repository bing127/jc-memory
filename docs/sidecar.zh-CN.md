# 本地 Sidecar

当前仓库提供 `jc-memory-sidecar` 的本地认证与生命周期核心，二进制名称为 `jc-memory-sidecar`（守护进程）和 `jcmem`（控制命令）。

## 已实现边界

- 数据目录下生成 32 字节随机 Token，文件权限为 Unix `0600`。
- 使用固定长度逐字节比较验证控制请求，不把 Token 写入日志或响应。
- 使用文件排他锁保证一个数据目录只运行一个 Sidecar；进程退出或崩溃后由操作系统释放锁。
- Unix 使用权限为 `0600` 的 Unix Domain Socket。
- 非 Unix 平台保留本地 Loopback 传输抽象；Named Pipe 尚未实现。
- 控制命令支持 `status`、`stop` 和 `doctor`。
- 绑定 Core 的守护进程支持 `execute` 控制消息，将 `MemoryRequest` 转发到共享 `CommandBus` 并返回 `ResponseEnvelope`；Commit 已覆盖该转发路径。

## 启动与控制

```bash
JC_MEMORY_SIDECAR_DATA_DIR=/var/lib/jc-memory-sidecar \
  cargo run --locked -p jc-memory-sidecar

JC_MEMORY_SIDECAR_DATA_DIR=/var/lib/jc-memory-sidecar \
  cargo run --locked -p jc-memory-sidecar --bin jcmem -- status
```

`jcmem stop` 会请求守护进程停止接收新连接并退出；`doctor` 只检查本地控制端点和数据目录，不执行记忆读写。

## 尚未实现

Sidecar 当前仍未提供完整 Observe/Recall/Correct/Forget/Explain/Stats 的专用 CLI 子命令，也尚未接入服务端 API Key、TLS 或独立 MCP Server。`execute` 转发复用本地 Token 和 Core ACL；Commit 的权威 Fragment 批次已在 SQLite 单事务中追加，自动 Quarantine 治理事件仍沿用既有治理路径。生产部署不能把当前 Sidecar 控制接口当作完整 Memory API；这些能力完成前不会创建 M4 总 Tag。
