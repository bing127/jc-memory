# Node.js / Electron SDK

疆诚记忆 Node SDK 提供两种模式：`RemoteMemoryClient` 通过 REST JSON Envelope 访问 `memory-server`；`EmbeddedMemoryClient` 通过 napi-rs 复用同一个 Rust Core CommandBus。两种模式共享 `observe`、`recall`、`commit`、`correct`、`explain`、`forget`、`stats` 和 `close` 方法。

## 安装与构建

当前包只在私有 CI 中构建，不代表已经发布到 npm。开发时执行：

```bash
corepack pnpm install --dir sdk/node
corepack pnpm --dir sdk/node test
corepack pnpm --dir sdk/node typecheck
corepack pnpm --dir sdk/node build
cargo build -p jc-memory-node
```

平台 `.node` 文件必须由与目标平台匹配的 CI 任务生成，`sdk/node/npm/` 中的包目录只描述平台选择和文件名。

## 远程模式

远程请求直接使用 `memory-protocol` 的 snake_case JSON 字段和版本化 Envelope。API Key 只从调用方配置传入，SDK 不把密钥写入日志或 Fixture。

```ts
import { RemoteMemoryClient } from "@jiangcheng-memory/node";

const client = new RemoteMemoryClient({
  baseUrl: process.env.JC_MEMORY_URL ?? "http://127.0.0.1:8080",
  apiKey: process.env.JC_MEMORY_API_KEY,
});
const response = await client.recall(requestEnvelope);
await client.close();
```

每个方法都接受可选 `AbortSignal`。HTTP 状态、取消和客户端 deadline 会映射为包含 `code`、`requestId`、`details` 的 `JiangchengMemoryError`。

## Embedded 模式

`crates/memory-node` 的 `NodeMemoryEngine` 只做 JSON 边界转换和安全的 Core 调用。业务授权由请求 Scope 和 Core Grant 决定；绑定宏生成的 `unsafe` 仅位于该 crate，核心工作区仍禁止 `unsafe`。

```ts
import { createNapiEmbeddedClient } from "@jiangcheng-memory/node";
import nativeEngine from "./jcmemory.node";

const client = createNapiEmbeddedClient(nativeEngine);
const response = await client.recall(requestEnvelope);
await client.close();
```

真实平台加载器应由发布包提供；未生成对应 `.node` 文件时，应用应回退到远程模式并明确记录运行模式。
