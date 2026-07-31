# Electron 示例

这个示例展示 Electron 主进程如何使用远程客户端。它不启动服务、不内置密钥，也不把本地用户数据写入仓库。

```bash
export JC_MEMORY_URL=http://127.0.0.1:8080
export JC_MEMORY_API_KEY=change-me
node examples/node-electron/index.mjs
```
