# Agentx Kubernetes 部署

统一入口是 `scripts/deploy.ps1`。脚本支持交互式 Custom 向导和版本化 JSON Profile；Profile 只保存模式、地址、镜像、Ingress、CA 文件路径和 Kubernetes Secret 名称，不保存密码、API Key 或 Session Token。

## 1. 前置条件

- PowerShell 7、`kubectl`，以及可访问的 Kubernetes 集群。
- `local-build` 需要 Docker；`registry` 需要集群能拉取 Profile 指定的镜像。
- 脚本优先使用 PATH 中的 Helm；不存在时自动下载固定 Helm `3.18.4` 并校验 SHA-256。
- ingress-nginx 固定 Chart `4.15.1`、Controller `1.15.1`，Chart SHA-256 为 `3eff0bd18151d6e6b1c441463410571443dda1ac78292cb189346628de784f0c`。

先执行：

```powershell
.\scripts\deploy.ps1 -Action Doctor -Profile Full
.\scripts\deploy.ps1 -Action Install -Profile Full
```

Full 在同一 Namespace 部署 MySQL、Redis、ClickHouse、MinIO、LightRAG、Mem0 和 Agentx 核心服务。Full 默认不启用 Sandbox；OpenSandbox 必须独立安装后再使用 remote Profile 接入。

## 2. Custom 与外部分散依赖

从 `deploy/profiles/custom.example.json` 创建环境 Profile，然后选择：

| 组件 | 模式 |
|---|---|
| MySQL | `bundled` / `external` |
| Redis | `bundled` / `external` |
| ClickHouse | `bundled` / `external` |
| Object Storage | `bundled-minio` / `external-s3` |
| LightRAG、Mem0 | `bundled` / `external` / `disabled` |
| Sandbox | `disabled` / `remote` |
| 镜像 | `local-build` / `registry` |

四项状态型依赖不能禁用。外部 S3 Bucket 必须预先创建；Install/Upgrade 阶段的 `doctor-infrastructure` 会执行 MySQL `SELECT 1`、Redis `PING`、ClickHouse 查询以及 S3 临时对象的写入、读取和删除，以验证 Bucket、凭据和读写权限。单独的 `-Action Doctor` 只做本机工具、权限、Chart 和 Kustomize 预检，不会创建 Kubernetes 资源。外部依赖不会被安装、升级或卸载命令修改。

非交互 managed Secret 使用以下环境变量：

| 条件 | 环境变量 |
|---|---|
| 外部 MySQL | `AGENTX_DEPLOY_MYSQL_PASSWORD` |
| 外部 Redis | `AGENTX_DEPLOY_REDIS_PASSWORD` |
| 外部 ClickHouse | `AGENTX_DEPLOY_CLICKHOUSE_PASSWORD` |
| 外部 S3 | `AGENTX_DEPLOY_S3_ACCESS_KEY`、`AGENTX_DEPLOY_S3_SECRET_KEY`，可选 `AGENTX_DEPLOY_S3_SESSION_TOKEN` |
| remote Sandbox | `AGENTX_DEPLOY_OPENSANDBOX_API_KEY` |
| bundled LightRAG | `AGENTX_DEPLOY_LIGHTRAG_OPENAI_API_KEY`；非本地环境必填 |
| bundled Mem0 | `AGENTX_DEPLOY_MEM0_OPENAI_API_KEY`；非本地环境必填 |

也可设置 `secrets.mode=existing`，预先创建 Profile 引用的 `agentx-secrets`、`agentx-lightrag-secrets` 和 `agentx-mem0-secrets`。脚本只验证键名，不读取或保存明文。

```powershell
.\scripts\deploy.ps1 -Action Doctor -Profile Custom -ConfigFile .\agentx.deploy.json
.\scripts\deploy.ps1 -Action Install -Profile Custom -ConfigFile .\agentx.deploy.json -NonInteractive
```

安装顺序为：预检、Namespace/Secret/CA、专用 Ingress、bundled 依赖、实际依赖 Doctor、Migration、核心服务、可选 Sandbox Manager、Addon、Web Ingress、Rollout/Health。

## 3. TLS 与 CA

- MySQL 支持 `disabled/preferred/required/verify_ca/verify_identity`、私有 CA 和 mTLS。
- Redis 使用 Rustls，支持 `rediss://`、独立密码 Secret、私有 CA 和 mTLS。
- ClickHouse 支持 HTTPS、系统 CA 和私有 CA Bundle。
- S3 支持 HTTPS、私有 CA Bundle、Access/Secret Key、Session Token 和 Path/Virtual-host Style。

CA、客户端证书和私钥必须是部署主机的绝对路径。脚本复制到只读 `agentx-trust-bundle` Secret，并挂载到需要的 Pod；不提供关闭证书校验的选项。

TLS 客户端合同可独立复核：

```powershell
.\scripts\tls-integration.ps1
```

该脚本验证系统 CA Store、ClickHouse/S3 HTTPS 私有 CA、错误 CA、认证失败、S3 Session Token，以及真实 MySQL/Redis TLS 容器的私有 CA、mTLS、错误 CA 和认证失败。仅运行不依赖 Docker 的部分可传 `-NoDocker`。统一脚本新生成的 managed Secret 使用 Base64URL 字符集，避免第三方客户端未转义密码时破坏连接 URI；已有 Secret 在 Upgrade 时保持不变。

## 4. Ingress

Agentx Web 只通过 `IngressClass=agentx-nginx` 暴露。Controller 安装在 `agentx-ingress`，Helm Release 为 `agentx-ingress-nginx`，不会设为默认 IngressClass。Profile 必须提供 Host，可选已有 TLS Secret，并可选择 `LoadBalancer` 或 `NodePort`。

卸载前脚本扫描所有 Namespace。若仍有其他 Ingress 使用 `agentx-nginx`，Controller 会保留。

## 5. Addon 与 OpenSandbox

外部 LightRAG/Mem0 不作为全局租户配置注入：脚本只做集群内连通性检查。完成 Agentx Bootstrap 后，在 UI/API 中依次创建 Credential、Connection、测试连接并给 Workflow Service Identity 创建 Grant。

OpenSandbox Server/Runtime 安装见 `deploy/opensandbox/README.md`。一个逻辑 Sandbox Manager 可以多副本共享 MySQL Lease，并连接一个 OpenSandbox Lifecycle Endpoint；OpenSandbox Kubernetes Runtime 再为每次会话创建任意数量的 Sandbox Pod。本阶段不提供多 Docker Host Provider 调度。

## 6. 升级、状态与卸载

```powershell
.\scripts\deploy.ps1 -Action Upgrade -ConfigFile .\agentx.deploy.json
.\scripts\deploy.ps1 -Action Status -Namespace agentx
.\scripts\deploy.ps1 -Action Uninstall -Namespace agentx
```

- 普通 Upgrade 禁止切换 MySQL、Redis、ClickHouse、Object Storage 的 bundled/external 模式。
- Addon 可切换模式，PVC 默认保留。
- remote Sandbox 切换为 disabled 或卸载前，Manager 的 `doctor-drain` 必须确认没有活跃 Lease。
- `-RotateSecrets` 只能与 `-Target all` 和 managed Secret 一起使用。它不轮换 Credential 加密 Key、bundled 数据库/存储密码、LightRAG API Token、Mem0 PostgreSQL/JWT 或远端 Node Token，避免存量数据和外部消费者失联；外部基础设施、OpenSandbox 和 Addon Provider 凭据必须先在 Provider 侧完成轮换，再通过部署环境变量写入。remote Sandbox 轮换前必须 drain。
- Uninstall 只删除带 `app.kubernetes.io/managed-by=agentx-deploy` 的资源，外部依赖永不修改。
- `-DeleteData` 只删除已标记所有权的已知 PVC；`-DeleteNamespace` 只删除由脚本创建并标记所有权的 Namespace。

交互模式下删除数据或 Namespace 必须再次输入 Namespace；CI 需要同时传 `-NonInteractive` 和显式删除开关。

## 7. 备份与恢复

升级或切换外部依赖前分别备份 MySQL、ClickHouse 和对象存储；Redis 不作为权威数据备份。bundled PVC 默认保留，但 PVC 不是备份。恢复顺序为 MySQL、对象存储、ClickHouse，随后运行 `Upgrade` 重新执行幂等 Migration 和 Rollout。

若状态型依赖需要从 bundled 迁移到 external，先停写并导出数据，验证外部服务，再使用新 Profile 执行卸载但保留 PVC，最后重新 Install；普通 Upgrade 会拒绝直接切换模式。

## 8. 验收

```powershell
.\scripts\deploy-tests.ps1
.\scripts\deploy.ps1 -Action Install -Profile Full -Namespace agentx-dryrun -DryRun -NonInteractive
.\scripts\deploy-distributed-e2e.ps1
.\scripts\e2e.ps1
```

`deploy-distributed-e2e.ps1` 将依赖放在 `agentx-e2e-deps`，Agentx 放在 `agentx-e2e`，验证跨 Namespace FQDN、Migration、升级数据保留和外部资源不被卸载。它还会在 Sandbox disabled Profile 下执行真实 Code Workflow，要求结果为 `RUNTIME_UNAVAILABLE`、Sandbox Lease 为 0 且 Sandbox Manager 不存在。默认结束后删除两个 Namespace；仅在确认当前源码镜像已经构建并导入集群时才使用 `-SkipBuild`。

空集群或清理旧环境后的基础安装验收使用：

```powershell
.\scripts\deploy.ps1 -Action Install -Profile Full -Namespace agentx -NonInteractive
.\scripts\deploy.ps1 -Action Status -Namespace agentx
```

成功状态必须包含 `agentx-deployment-state`，四项 bundled 基础设施、核心服务和所选 Addon 均 Ready；Full 默认 Sandbox disabled，因此不应存在 Sandbox Manager。
