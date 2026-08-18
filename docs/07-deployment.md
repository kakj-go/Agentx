# Kubernetes 部署与扩展

## 1. 可组合部署契约

Agentx V2 通过 `scripts/deploy-v2.ps1` 和版本化 `agentx.io/deployment/v2alpha2` Profile 部署。Profile 固定 `control/runtime/dependencies` 三个物理 Namespace；Observability 是独立逻辑 Plane，但与 Runtime 共用物理 Namespace。状态型依赖只能使用 bundled 或 external：

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

本地 Full 将 Control MySQL 放在 Control，Runtime MySQL/Redis/ClickHouse 放在 Runtime，Vault/MinIO/Ingress 放在 Dependencies。生产使用相同三个 Namespace，但状态型中间件可以全部位于集群外，只要 Agentx Pod 能解析地址、完成 TLS 验证并通过 Doctor。

## 2. 目录边界

```text
deploy/
├── profiles/
├── ingress-nginx/
├── k8s/
│   ├── v2/{control,runtime,observability,dependencies}/
│   ├── addons/{lightrag,mem0}/
│   └── fixtures/
└── opensandbox/{docker,kubernetes}/
```

Control、Runtime、Observability、Dependencies 继续保持四个逻辑 Kustomization；部署器把 Observability 映射到 Runtime Namespace，并负责动态 Namespace、Secret、镜像重写、Pull Policy 和应用顺序。Echo MCP/Node 只用于 local/E2E，不属于基础生产服务。

## 3. 安装流程

```powershell
.\scripts\deploy-v2.ps1 -Action Validate -ConfigFile deploy/profiles/v2-full-local.json
.\scripts\deploy-v2.ps1 -Action Install -Target All -ConfigFile deploy/profiles/v2-full-local.json
.\scripts\deploy-v2.ps1 -Action Doctor -Target All -ConfigFile deploy/profiles/v2-full-local.json
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
10. 创建业务 Ingress，等待全部 Rollout，并把每个逻辑 Plane 的发布描述写入独立的 `agentx-v2-release-state-<plane>` ConfigMap。

`-Action Render` 执行 Profile、Chart 和 Kustomize 渲染但不修改 Kubernetes 资源；`-Action Validate` 只校验 Profile 和架构约束。

## 4. 外部依赖与 TLS

MySQL 支持 TLS Mode、私有 CA 和 mTLS；Redis 使用 Rustls 并支持 `rediss://`、私有 CA、mTLS 和独立密码；ClickHouse 使用 HTTPS/Rustls 并合并系统 CA 与私有 CA Bundle；S3 支持 Access/Secret Key、Session Token、Path/Virtual-host Style 和私有 CA Bundle。

不能关闭证书校验。生产 Object Storage 禁止 HTTP。外部 S3 Bucket 必须预创建，部署脚本不会创建、清空或删除外部 Bucket。新环境变量使用 `AGENTX_S3_ACCESS_KEY/SECRET_KEY`，运行时代码短期兼容旧 MinIO 变量。

Sandbox Manager 只解析 Runtime MySQL 和 OpenSandbox Settings；Runtime Gateway、Workflow Runtime、Workflow Worker 按各自 Role 使用 Runtime MySQL、Redis、Object Storage、Vault或 Provider；Observability只使用受限 Redis ACL、ClickHouse和独立对象存储身份。服务不会因无关依赖配置缺失而获得跨域凭据。

## 5. 专用 Ingress

脚本管理固定 ingress-nginx Chart `4.15.1`、Controller `1.15.1`、Release `agentx-ingress-nginx` 和 Profile 指定的 IngressClass。它安装在 `agentx-v2-deps`（或 RunId 对应 Dependencies Namespace），不是默认 IngressClass，不接管未带 V2 所有权标记的 Controller。

Profile 必须提供 Host，可选择已有 TLS Secret。默认环境使用 `LoadBalancer`；RunId 临时环境使用独立 IngressClass、独立 Helm资源名和 `ClusterIP`，避免并发环境争用 80/443。卸载前会扫描全集群；仍有 Ingress 使用该 IngressClass 时保留 Controller。

`network.allowedEgressCidrs` 是核心服务访问外部模型供应商、外部依赖和远程 Sandbox 的显式出口白名单。Docker Desktop 会把公网 DNS 映射到 `198.18.0.0/15`，本地 Sandbox Profile 因此允许该保留网段；生产 Profile 必须改为供应商实际出口 CIDR，不能照搬本地范围。

## 6. Addon 边界

LightRAG/Mem0 是可选业务 Addon，不是 Agentx 权威状态存储。bundled 模式由 Profile 提供 Provider Base URL、模型和 Embedding 配置，由独立 Secret 提供 API Key；local Full 可以使用 Echo MCP，test/production 必须使用真实 Provider。

external 模式不把 Endpoint 或 Credential 作为全局租户资源。Bootstrap 后，管理员在 UI/API 创建 Credential、Connection、测试连接并向 Workflow Service Identity 创建 Grant。切换 Addon 模式时 Workload 可删除，但 PVC 默认保留。

## 7. OpenSandbox 边界

OpenSandbox Server、Controller、RuntimeClass 和计算节点归 Dependencies，但当前仍由其独立安装流程管理，不由主部署脚本隐式创建。Sandbox disabled 时不部署 Manager，也不注入 Manager URL/Token；Worker仍订阅 Sandbox capability，Code节点会明确返回 `RUNTIME_UNAVAILABLE`，不会永久留在队列。

remote 模式部署一个逻辑 Sandbox Manager 服务；其多副本共享 MySQL Lease并连接一个 Lifecycle Endpoint。OpenSandbox Kubernetes Runtime 负责为会话创建并调度任意多个 Sandbox Pod。本阶段不实现多个独立 Docker Host Provider 的容量调度和 Sticky Routing。

本地 Docker+runc 和当前 Kubernetes 环境用于功能验收。gVisor/Kata、生产 Vault、镜像签名和攻击隔离当前暂不重试，由 M7 INT-006/010/011/014 作为生产发布强化验收，不阻塞当前 Kubernetes 部署认证。

## 8. 运行健康与扩展

- Platform Control：无状态，可水平扩展；Control MySQL为必需依赖，通过内部 API查询 Runtime/Observability。
- Runtime Gateway：无状态；SSE游标持久化在 Runtime MySQL，Redis只用于唤醒。
- Workflow Runtime：多副本通过 Runtime MySQL状态条件和 Lease协作，承载 command/outbox/recovery/trigger/trace-relay 等 Role。
- Workflow Worker：按 capability/队列扩展，周期探测 Runtime MySQL、Redis、Object Storage 和 Runtime API。
- Sandbox Manager：共享 MySQL Lease，周期验证 MySQL、OpenSandbox `/health` 和认证列表接口，Heartbeat 使用真实状态。
- Observability：消费受限 Runtime Redis Trace Stream并写入/查询 ClickHouse；没有 Runtime MySQL凭据，ClickHouse故障不影响 Execution提交。

Readiness 表示必需依赖可用，Liveness 只表示进程存活。所有外部调用仍需要重试、幂等键、Lease 和 Outbox，不能把 Kubernetes 重启当作一致性机制。

## 9. 升级与卸载

Upgrade/Rollback按 `Control/Runtime/Observability/Dependencies` 逻辑 Target 独立操作；Runtime与 Observability共享 Namespace时仍使用不同 Release State和资源标签。Upgrade/Rollback保留集群现有副本数，首次 Install使用 Profile默认的 1 副本。

Uninstall按逻辑 Plane删除资源；`Uninstall -Target Observability` 不删除 Runtime Deployment、Runtime MySQL/Redis或 Runtime Namespace。普通卸载保留 Namespace；只有非生产 RunId 的 `-Target All -PurgeTestResources` 才删除三个去重后的临时 Namespace。外部依赖永不修改，PVC不是备份。

ingress-nginx通过 Helm Annotation和 Ownership ConfigMap验证所有权。卸载时先删除受管业务 Ingress，再扫描 IngressClass使用者；只有无人使用且所有权匹配时才删除 Controller和集群级 IngressClass。

## 10. E2E

`scripts/v2-profile-tests.ps1` 覆盖 `v2alpha2` Schema、旧 Profile拒绝、三 Namespace渲染、逻辑 Target、七类 Deployment/PDB和零 HPA/指标栈资源。`scripts/v2-07-profile-tests.ps1` 继续验证安全与发布 Profile门禁。

完整 V2 E2E 使用 RunId创建 Control/Runtime/Dependencies三个临时 Namespace和唯一 IngressClass，执行 Migration、Bootstrap、Doctor、发布、Invocation、Worker、Trace、故障与恢复验证。测试结束删除三个临时 Namespace以及本次受管 Helm Release/IngressClass，不触碰正式开发 Namespace。历史 `2→4→2` Run仅作为横向扩展正确性的既有证据；当前默认常驻副本数为 1。
