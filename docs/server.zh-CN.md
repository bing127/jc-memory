# HTTP 与 gRPC Server

`jc-memory-server` 同时提供 Axum HTTP 和 Tonic gRPC 门面，两者把请求转换为共享 `MemoryRequest`，最终进入同一个 Core `CommandBus`。协议层不复制记忆业务逻辑，也不能绕过 Core ACL。

## 已实现接口

HTTP 提供：

- `/healthz`、`/readyz`、`/metrics`；
- `/v1/memory/observe`、`commit`、`recall`、`correct`、`forget`、`explain` 和 `stats`。

gRPC 提供 `jiangcheng.memory.v1.MemoryService/Execute`，使用强类型 Protobuf 请求和响应。REST 与 gRPC 共享错误分类、`request_id`、并发门禁、请求超时和按 API Key 隔离的固定窗口速率限制。

## 身份与权限

服务端从 `x-api-key` 解析调用方身份，并在服务端构造 `AccessContext`。客户端提交的 `access_scope` 必须与 API Key 绑定的 Namespace 一致，客户端不能自行授予 Grant。

当前启动入口为单 API Key、单 Namespace，授予 Observe、Recall、Mutate 和 Explain。Admin、Export、Delete、Consolidate 及组织级角色系统尚未通过远程 Server 暴露。

## TLS 与关闭

gRPC TLS 从 PEM 证书链和私钥文件加载；两者必须成对配置。未配置时保留本机明文开发模式。私钥内容不进入日志和错误正文。

HTTP 与 gRPC 在同一进程启动。收到 Ctrl-C 后通过共享 CancellationToken 停止两个监听任务，并等待服务任务结束。

配置字段、来源优先级和 Secret 边界见 [Server 配置参考](configuration.zh-CN.md)，Docker 与 Kubernetes 资源见 [部署指南](deployment.zh-CN.md)。仓库中的容器和 Kubernetes 资源目前只有静态结构与契约验证，不代表真实运行时或集群验证。
