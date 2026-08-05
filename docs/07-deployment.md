# Kubernetes 部署与扩展

## 1. 可组合部署契约

Agentx 通过 `scripts/deploy.ps1` 和版本化 `agentx.io/deployment/v1alpha1` Profile 部署。四项状态型依赖是完整运行模式的必需依赖，只能使用 bundled 或 external：

| 组件 | 模式 |
|---|---|
| MySQL | `bundled` / `external` |
| Redis | `bundled` / `external` |
| ClickHouse | `bundled` / `external` |
| Object Storage | `bundled-minio` / `external-s3` |
| LightRAG、Mem0 | `bundled` / `external` / `disabled` |
| Sandbox | `disabled` / `remote` |
| Agentx Image | `local-build` / `registry` |

Profile 只保存非敏感配置和固定 Secret 引用。密码、API Key、Session Token 和加密 Key 只进入 Kubernetes Secret；CA、客户端证书和私钥由部署主机的绝对路径复制到只读 Trust Bundle。

Full 将四项 bundled 基础设施、两个 Addon 和全部核心服务部署在同一 Namespace。Custom 可以让每项依赖位于其他 Namespace、其他 Kubernetes 集群或独立机器，只要 Agentx Pod 能解析地址、完成 TLS 验证并通过 Doctor。

## 2. 目录边界

```text
deploy/
├── profiles/
├── ingress-nginx/
├── k8s/
│   ├── services/{core,migrations,sandbox-manager}/
│   ├── infrastructure/{mysql,redis,clickhouse,minio}/
│   ├── addons/{lightrag,mem0}/
│   ├── fixtures/{echo-mcp,echo-node}/
│   └── stacks/{full,e2e}/
└── opensandbox/{docker,kubernetes}/
```

Component Kustomization 不固定 Namespace。统一脚本负责 Namespace、动态 ConfigMap/Secret、镜像重写、Pull Policy 和应用顺序。Echo MCP/Node 只用于 local/E2E，不属于基础生产服务。

## 3. 安装流程

```powershell
.\scripts\deploy.ps1 -Action Doctor -Profile Full
.\scripts\deploy.ps1 -Action Install -Profile Full
.\scripts\deploy.ps1 -Action Install -Profile Custom -ConfigFile .\agentx.deploy.json -NonInteractive
```

顺序固定为：

1. 校验 Profile、PowerShell、kubectl、Docker/镜像模式、权限和 Kustomize/Helm 渲染。单独执行 `-Action Doctor` 到此结束，不创建资源。
2. 创建或复用 Namespace，创建/验证 Secret 和 CA Trust Bundle。
3. 安装专用 ingress-nginx。
4. 安装 selected bundled 基础设施并等待 StatefulSet/Bucket Job。
5. 运行 `doctor-infrastructure`，实际执行 MySQL `SELECT 1`、Redis `PING`、ClickHouse `SELECT 1` 和 S3 临时对象写入、读取、内容校验及删除。
6. 运行 MySQL/ClickHouse Migration。
7. 部署核心服务，并让 Readiness 反映周期依赖探测。
8. remote Sandbox 模式部署 Manager 并运行 `doctor-opensandbox`。
9. 部署 bundled Addon，或对 external Addon 做集群内 Endpoint 连通性检查。
10. 创建 Web Ingress，等待全部 Rollout，将 Profile、Hash 和最后 Target 保存到 `agentx-deployment-state`。

`-DryRun` 执行同样的 Profile、Chart 和 Kustomize 检查，但不修改 Kubernetes 资源。

## 4. 外部依赖与 TLS

MySQL 支持 TLS Mode、私有 CA 和 mTLS；Redis 使用 Rustls 并支持 `rediss://`、私有 CA、mTLS 和独立密码；ClickHouse 使用 HTTPS/Rustls 并合并系统 CA 与私有 CA Bundle；S3 支持 Access/Secret Key、Session Token、Path/Virtual-host Style 和私有 CA Bundle。

不能关闭证书校验。生产 Object Storage 禁止 HTTP。外部 S3 Bucket 必须预创建，部署脚本不会创建、清空或删除外部 Bucket。新环境变量使用 `AGENTX_S3_ACCESS_KEY/SECRET_KEY`，运行时代码短期兼容旧 MinIO 变量。

Sandbox Manager 只解析 MySQL 和 OpenSandbox Settings；Worker/Coordinator 只解析 MySQL、Redis 和 Object Storage；Trace Writer 只解析 MySQL、Redis 和 ClickHouse；Trigger Gateway 只解析 MySQL。服务不会因无关依赖配置缺失而拒绝启动。

## 5. 专用 Ingress

脚本管理固定 ingress-nginx Chart `4.15.1`、Controller `1.15.1`、Release `agentx-ingress-nginx` 和 `IngressClass=agentx-nginx`。它安装在 `agentx-ingress`，不是默认 IngressClass，不接管已有 Controller。

Profile 必须提供 Host，可选择已有 TLS Secret 和 `LoadBalancer/NodePort`。卸载前会扫描其他 Namespace；仍有 Ingress 使用 `agentx-nginx` 时保留 Controller。

## 6. Addon 边界

LightRAG/Mem0 是可选业务 Addon，不是 Agentx 权威状态存储。bundled 模式由 Profile 提供 Provider Base URL、模型和 Embedding 配置，由独立 Secret 提供 API Key；local Full 可以使用 Echo MCP，test/production 必须使用真实 Provider。

external 模式不把 Endpoint 或 Credential 作为全局租户资源。Bootstrap 后，管理员在 UI/API 创建 Credential、Connection、测试连接并向 Workflow Service Identity 创建 Grant。切换 Addon 模式时 Workload 可删除，但 PVC 默认保留。

## 7. OpenSandbox 边界

OpenSandbox Server、Controller、RuntimeClass 和计算节点不由主部署脚本安装。Sandbox disabled 时不部署 Manager，也不注入 Manager URL/Token；Worker 仍订阅 Sandbox capability，Code 节点会明确返回 `RUNTIME_UNAVAILABLE`，不会永久留在队列。

remote 模式部署一个逻辑 Sandbox Manager 服务；其多副本共享 MySQL Lease并连接一个 Lifecycle Endpoint。OpenSandbox Kubernetes Runtime 负责为会话创建并调度任意多个 Sandbox Pod。本阶段不实现多个独立 Docker Host Provider 的容量调度和 Sticky Routing。

本地 Docker+runc 和当前 Kubernetes 环境用于功能验收。gVisor/Kata、生产 Vault、镜像签名和攻击隔离当前暂不重试，由 M7 INT-006/010/011/014 作为生产发布强化验收，不阻塞当前 Kubernetes 部署认证。

## 8. 运行健康与扩展

- Platform API：无状态，可水平扩展；MySQL 为必需依赖，Redis/ClickHouse/Object Storage 故障显示 degraded。
- Trigger Gateway：无状态，只依赖 MySQL；SSE 状态通过 Redis/Execution Runtime 获取。
- Coordinator：多副本通过 MySQL 状态条件和 Lease 协作，周期探测 MySQL、Redis、Object Storage。
- Worker：按 capability/队列扩展，周期探测 MySQL、Redis、Object Storage 和 Coordinator。
- Sandbox Manager：共享 MySQL Lease，周期验证 MySQL、OpenSandbox `/health` 和认证列表接口，Heartbeat 使用真实状态。
- Trace Writer：周期探测 MySQL、Redis、ClickHouse；ClickHouse 故障不影响 Execution 提交。

Readiness 表示必需依赖可用，Liveness 只表示进程存活。所有外部调用仍需要重试、幂等键、Lease 和 Outbox，不能把 Kubernetes 重启当作一致性机制。

## 9. 升级与卸载

普通 Upgrade 禁止改变四项状态型依赖的 bundled/external 模式。此类迁移必须停写、备份、恢复到目标服务并重新 Install。Addon 可以切换；Sandbox remote 关闭前必须由 `doctor-drain` 确认无活跃 Lease。

Uninstall 只删除脚本所有权标签匹配的资源，外部依赖永不修改。PVC 默认保留；`-DeleteData` 只删除明确列出的 owned PVC。`-DeleteNamespace` 只删除脚本创建且带所有权 Annotation 的 Namespace。

非敏感 Profile 和 Hash 保存在 ConfigMap；Secret 不进入状态 ConfigMap。`-RotateSecrets` 只能用于 managed Secret 的全量 Target，并保留 Credential Keyring、bundled 持久化服务密码和需要外部协同的 Token；外部 Provider 凭据先在 Provider 轮换，remote Sandbox 还必须先 drain。

## 10. E2E

`scripts/deploy-tests.ps1` 覆盖 Profile 模式组合、Schema、Secret 明文、稳定 Hash、非法状态型依赖切换和所有权。`scripts/deploy-distributed-e2e.ps1` 使用 `agentx-e2e-deps` 与 `agentx-e2e` 验证依赖分散、升级数据保留和外部资源不被卸载。

`scripts/tls-integration.ps1` 验证系统 CA Store；ClickHouse/S3 HTTPS 私有 CA、错误 CA、认证失败和 S3 Session Token；以及真实 MySQL/Redis TLS 容器的私有 CA、mTLS、错误 CA 和认证失败。`-NoDocker` 只运行系统 CA、ClickHouse 和 S3 合同。

完整 `scripts/e2e.ps1` 使用临时 Agentx Namespace、真实 OpenSandbox Adapter、固定 LightRAG/Mem0 和 M2.1-M5 Playwright。测试前可缩容开发 Namespace；结束时清理本次 Sandbox、删除临时 Namespace并恢复开发副本数。
