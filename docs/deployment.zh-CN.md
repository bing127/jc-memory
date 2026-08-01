# 疆诚记忆部署指南

本指南覆盖本地二进制、Docker Compose 和 Kubernetes 部署。配置字段与来源优先级见 [Server 配置参考](configuration.zh-CN.md)。

## 前置边界

- 当前可运行二进制名为 `jc-memory-server`。
- HTTP 默认端口为 `8787`，gRPC 默认端口为 `8788`。
- `/healthz` 用于存活检查，`/readyz` 用于就绪检查。
- API Key 必须匹配 `^sk-jcmemory-[a-z0-9]{34}$`，并通过环境变量或 Secret 注入，不得写入镜像、ConfigMap 或仓库配置。
- 单个 Embedded 数据目录采用单写者边界；Kubernetes 模板使用一个副本和 `Recreate` 策略。

## 本地二进制

```bash
export JC_MEMORY_CONFIG_FILE=./server.yaml
export JC_MEMORY_API_KEY="sk-jcmemory-$(openssl rand -hex 17)"
cargo run --locked -p jc-memory-server
```

启动后可检查：

```bash
curl --fail http://127.0.0.1:8787/healthz
curl --fail http://127.0.0.1:8787/readyz
```

## Docker Compose

Compose 使用 [server.yaml.example](../deploy/docker/config/server.yaml.example) 作为非秘密配置，并将数据保存到命名卷 `jc-memory-data`。

```bash
export JC_MEMORY_API_KEY="sk-jcmemory-$(openssl rand -hex 17)"
export JC_MEMORY_NAMESPACE_ID='00000000-0000-7000-8000-000000000003'
docker compose -f deploy/docker/compose.yaml build
docker compose -f deploy/docker/compose.yaml up -d
```

查看状态与日志：

```bash
docker compose -f deploy/docker/compose.yaml ps
docker compose -f deploy/docker/compose.yaml logs memory-server
curl --fail http://127.0.0.1:8787/healthz
```

停止容器不会删除命名卷：

```bash
docker compose -f deploy/docker/compose.yaml down
```

删除命名卷会删除持久化记忆数据，执行前必须完成备份并明确确认目标卷。

## Kubernetes

部署前必须完成以下替换：

1. 将 `deploy/kubernetes/deployment.yaml` 中的镜像改为已推送的不可变版本或 digest；
2. 将 ConfigMap 中的 `namespace_id` 改为目标 Namespace UUID；
3. 根据集群 StorageClass 调整 PVC；
4. 使用真实随机 API Key 和有效 PEM 证书创建 Secret。

不要直接应用 `secret.example.yaml`。推荐使用命令创建 Secret，避免凭据进入 shell 历史时应改用受控文件或平台 Secret 管理器：

```bash
kubectl create secret generic jc-memory-server-secret \
  --from-literal=JC_MEMORY_API_KEY="sk-jcmemory-$(openssl rand -hex 17)" \
  --from-file=tls.crt=./server.crt \
  --from-file=tls.key=./server.key
```

应用非秘密资源：

```bash
kubectl apply -f deploy/kubernetes/configmap.yaml
kubectl apply -f deploy/kubernetes/persistentvolumeclaim.yaml
kubectl apply -f deploy/kubernetes/deployment.yaml
kubectl apply -f deploy/kubernetes/service.yaml
```

检查部署：

```bash
kubectl rollout status deployment/jc-memory-server
kubectl get pod,service,pvc -l app.kubernetes.io/name=jc-memory-server
kubectl port-forward service/jc-memory-server 8787:8787
curl --fail http://127.0.0.1:8787/readyz
```

## TLS 与网络边界

Kubernetes 模板为 gRPC 配置文件型 TLS。HTTP 健康端口当前仍为集群内明文，应由 NetworkPolicy、Service Mesh 或 Ingress 边界限制访问。Docker Compose 模板默认使用本机明文 gRPC 开发模式；生产部署应挂载证书/私钥并在 YAML 中配置 `tls`。

## 当前验证范围

部署 YAML 可在本地做结构解析，Rust Server 可做编译和协议测试。当前 macOS 验收还会启动真实 `jc-memory-server` 二进制，执行 HTTP Observe，发送 `SIGINT` 干净退出，并以同一数据目录重启后通过 Stats 验证权威 Fragment/Event 持久化。

该测试不等同于 Docker 镜像、Compose、Kubernetes、Linux 或 Windows 实机验证，也不代表五协议已在独立部署进程组合中完成同一 Fixture 验收。只有在实际安装了 Docker/Kubectl 并连接目标运行时后，才能声称镜像构建或 Kubernetes 实机部署通过。
