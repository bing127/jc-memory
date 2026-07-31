# 本地 Sidecar

当前仓库提供 `jc-memory-sidecar` 的本地认证与生命周期核心，二进制名称为 `jc-memory-sidecar`（守护进程）和 `jcmem`（控制命令）。

## 已实现边界

- 数据目录下生成 32 字节随机 Token，文件权限为 Unix `0600`。
- 使用固定长度逐字节比较验证控制请求，不把 Token 写入日志或响应。
- 使用文件排他锁保证一个数据目录只运行一个 Sidecar；进程退出或崩溃后由操作系统释放锁。
- Unix 使用权限为 `0600` 的 Unix Domain Socket。
- 非 Unix 平台使用随机端口的本地 Loopback，并将地址写入受数据目录保护的端点文件；Named Pipe 尚未实现。
- 控制命令支持 `status`、`stop`、`doctor`，以及 `observe`、`commit`、`recall`、`correct`、`forget`、`explain`、`stats` 七个业务子命令。
- 绑定 Core 的守护进程支持 `execute` 控制消息，将 `MemoryRequest` 转发到共享 `CommandBus` 并返回 `ResponseEnvelope`；Commit 已覆盖该转发路径。
- 连接按任务并发处理；收到 `stop` 后停止接收新连接，并等待已经通过认证的在途请求完成响应后退出。

## 启动与控制

```bash
JC_MEMORY_SIDECAR_DATA_DIR=/var/lib/jc-memory-sidecar \
  cargo run --locked -p jc-memory-sidecar

JC_MEMORY_SIDECAR_DATA_DIR=/var/lib/jc-memory-sidecar \
  cargo run --locked -p jc-memory-sidecar --bin jcmem -- status

# 业务子命令从文件读取完整的 MemoryRequest JSON。
JC_MEMORY_SIDECAR_DATA_DIR=/var/lib/jc-memory-sidecar \
  cargo run --locked -p jc-memory-sidecar --bin jcmem -- stats \
  --request-file ./fixtures/stats-request.json

# 也可以直接传入 JSON；命令名称必须与请求中的 command 一致。
JC_MEMORY_SIDECAR_DATA_DIR=/var/lib/jc-memory-sidecar \
  cargo run --locked -p jc-memory-sidecar --bin jcmem -- stats \
  --request-json '{"api_version":"v1","request_id":"stats-1","access_scope":{"namespace_id":"00000000-0000-7000-8000-000000000001"},"command":{"command":"stats","payload":null}}'
```

`jcmem stop` 会请求守护进程停止接收新连接并退出；`doctor` 只检查本地控制端点和数据目录，不执行记忆读写。

业务子命令只负责读取和校验请求 Envelope，实际授权仍由 Sidecar 生成精确 Scope Grant 后交给 Core `CommandBus`。`--request-file` 与 `--request-json` 不能同时使用；请求文件不应包含服务端 Token。

## 尚未实现

Sidecar 尚未实现非 Unix Named Pipe、服务端 API Key/TLS 转发和崩溃后的 checkpoint 恢复；`execute` 转发复用本地 Token 和 Core ACL。Commit 的权威 Fragment 批次已在 SQLite 单事务中追加，自动 Quarantine 治理事件仍沿用既有治理路径。生产部署不能把当前 Sidecar 控制接口当作完整远程 Memory API；checkpoint、完整身份转发和 Runtime 闭环完成前不会创建 M4 总 Tag。
