# 本地 Sidecar

当前仓库提供 `jc-memory-sidecar` 的本地认证与生命周期核心，二进制名称为 `jcmemd`（守护进程）和 `jcmem`（控制命令）。

Sidecar 可通过 `JC_MEMORY_SIDECAR_DAEMON_BIN` 显式指定守护进程可执行文件路径；未设置时使用当前 PATH 中的 `jcmemd`。该变量只影响本地进程启动，不进入 MemoryRequest、协议字段或远程 API。

## 已实现边界

- 数据目录下生成 32 字节随机 Token，文件权限为 Unix `0600`。
- 使用固定长度逐字节比较验证控制请求，不把 Token 写入日志或响应。
- 使用文件排他锁保证一个数据目录只运行一个 Sidecar；锁文件记录 PID/启动标识，进程退出或崩溃后由操作系统释放锁，并在下次取得可用锁时识别残留元数据。
- Unix 使用权限为 `0600` 的 Unix Domain Socket。
- Windows 使用 Tokio Named Pipe；配置端点路径通过稳定的本地管道名映射，不在数据目录写入地址文件。
- 其他非 Unix/Windows 平台使用随机端口的本地 Loopback，并将地址写入受数据目录保护的端点文件。
- 控制命令支持 `status`、`stop`、`doctor`，以及 `observe`、`commit`、`recall`、`correct`、`forget`、`explain`、`stats` 七个业务子命令。
- 绑定 Core 的守护进程支持 `execute` 控制消息，将 `MemoryRequest` 转发到共享 `CommandBus` 并返回 `ResponseEnvelope`；Commit 已覆盖该转发路径。
- 绑定 Core 的守护进程还支持 `execute_as` 控制消息；经过本地 Token 认证的 MCP 主体可以通过该消息传到 Core，默认 `execute` 仍使用 `sidecar-local` 主体。
- 连接按任务并发处理；单个畸形控制消息只终止对应连接。收到 `stop` 后停止接收新连接、取消尚未认证的空闲连接，并等待已经通过认证的在途请求完成响应后退出。
- `jcmem start` 会在当前数据目录启动 `jcmemd`；如果已有可认证的守护进程则返回 `already_running`。
- 生命周期检查点记录 `running`、`stopped`、PID 和启动时间，并通过同目录临时文件原子替换。下一次启动可识别上次未干净停止；干净停止会显式关闭 Embedded SQLite 连接池后写入 `stopped`，关闭失败则保留 `running`。

## 启动与控制

```bash
JC_MEMORY_SIDECAR_DATA_DIR=/var/lib/jc-memory-sidecar \
  cargo run --locked -p jc-memory-sidecar --bin jcmemd

JC_MEMORY_SIDECAR_DATA_DIR=/var/lib/jc-memory-sidecar \
  cargo run --locked -p jc-memory-sidecar --bin jcmem -- start

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

## 尚未验证或不在范围内

Windows Named Pipe 已实现并有目标专用契约测试，但当前 macOS 主机缺少 `x86_64-w64-mingw32-gcc`，未完成 Windows 编译或实机传输验证。服务端 API Key/TLS 转发仍不属于 Sidecar 控制接口；`execute`/`execute_as` 转发复用本地 Token 和 Core ACL。生命周期检查点只识别干净或非干净退出，不替代 Snapshot/Restore。Commit 的权威 Fragment 批次已在 SQLite 单事务中追加，自动 Quarantine 治理事件仍沿用既有治理路径。生产部署不能把当前 Sidecar 控制接口当作完整远程 Memory API。
