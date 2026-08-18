# Agentx V2 Kubernetes 部署

统一入口是 `scripts/deploy-v2.ps1`，Profile API 固定为破坏性的 `agentx.io/deployment/v2alpha3`。`v2alpha2` 及更早版本不做转换，校验时会明确拒绝。

## 1. 物理 Namespace 与逻辑 Plane

| 物理 Namespace | 逻辑 Plane / 组件 |
|---|---|
| `agentx-v2-control` | Control：`web-console`、`platform-control`、Control MySQL、Migration、Ingress |
| `agentx-v2-runtime` | Runtime + Observability：四个 Runtime Deployment、Runtime MySQL/Redis、`observability`、ClickHouse、各自 Migration/Doctor/Bootstrap |
| `agentx-v2-deps` | Dependencies：`agentx-egress-gateway`、专用 ingress-nginx、Vault、MinIO、共享 Bootstrap；集群内部署的 OpenSandbox/LightRAG/Mem0 也属于此域 |

Observability 与 Runtime 共用 Namespace，但仍使用独立的 ServiceAccount、Secret、Redis ACL、ClickHouse账号、NetworkPolicy、Release State 和 `agentx.io/plane=observability` Pod 标签。Observability 不持有 Runtime MySQL 凭据。

生产环境也创建这三个 Namespace。外部 MySQL、Redis、ClickHouse、S3、Vault 和 OpenSandbox 不由部署器安装；Dependencies Namespace 至少承载专用 ingress-nginx。

## 2. 前置条件与 Profile

- PowerShell 7、`kubectl` 和可访问的 Kubernetes 集群。
- `local-build` 镜像模式需要 Docker；`registry` 模式需要集群可拉取 Profile 中的固定镜像。
- 脚本优先使用 PATH 中的 Helm；否则下载并校验固定 Helm `3.18.4`。
- ingress-nginx 固定 Chart `4.15.1`、Controller `1.15.1` 和 Chart SHA-256。

Profile 的 `namespaces` 只能包含不同且非空的 `control`、`runtime`、`dependencies`。本地示例为 `deploy/profiles/v2-full-local.json`，生产示例为 `deploy/profiles/v2-production.example.json`。

```powershell
.\scripts\deploy-v2.ps1 -Action Validate -Target All -ConfigFile deploy/profiles/v2-full-local.json
.\scripts\deploy-v2.ps1 -Action Render -Target All -ConfigFile deploy/profiles/v2-full-local.json
```

Profile 只保存非敏感配置和 Secret 引用。`generated-local` 在三个 Namespace 中生成分域 Secret；生产使用 `existing-kubernetes`，必须预先创建 Profile 引用的工作负载 Secret。

## 3. 安装与独立逻辑 Target

```powershell
.\scripts\deploy-v2.ps1 -Action Install -Target All -ConfigFile deploy/profiles/v2-full-local.json
.\scripts\deploy-v2.ps1 -Action Status -Target All -ConfigFile deploy/profiles/v2-full-local.json
.\scripts\deploy-v2.ps1 -Action Doctor -Target All -ConfigFile deploy/profiles/v2-full-local.json
```

`-Target` 支持 `Control`、`Runtime`、`Observability`、`Dependencies`、`All`。Runtime 与 Observability 虽共享 Namespace，渲染、升级、状态、Doctor、回滚和卸载仍按逻辑 Plane 标签与资源清单隔离。Release State 分别保存为 `agentx-v2-release-state-control`、`agentx-v2-release-state-runtime` 和 `agentx-v2-release-state-observability`。

安装顺序为：Profile/工具预检、三个 Namespace、分域 Secret、Dependencies ingress-nginx、Gateway 公钥/TLS/NetworkPolicy、Gateway Ready、bundled 基础设施、Migration、Bootstrap/Doctor、其余七类应用 Deployment、Rollout 和独立 Release State。八类应用首次安装默认均为 1 副本；Upgrade/Rollback 保留集群中现有副本数。

## 4. Ingress

Control 与 Runtime 的 Ingress 对象留在各自业务 Namespace。Controller 的 Helm Release 固定为 `agentx-ingress-nginx`，安装在 Profile 的 Dependencies Namespace，IngressClass 使用 Profile 的 `ingress.className`，且不会设为默认类。

部署器使用 Helm Annotation 与 `agentx-ingress-ownership` ConfigMap 校验所有权；发现同名但未受管的 Release 或 IngressClass 会拒绝接管。卸载时先删除本 Target 的受管 Ingress，再扫描全集群使用者；只有无人使用且所有权匹配时才卸载 Controller 和 IngressClass。

RunId E2E 为每次运行生成三个临时 Namespace、独立 IngressClass 和独立 Helm 资源名，并将 Controller Service 设为 `ClusterIP`，避免并发临时环境争用宿主机 80/443。

## 5. 安全边界

- Control、Runtime Namespace 执行 Restricted Pod Security；Dependencies 按 ingress-nginx/OpenSandbox 所需权限配置。
- ingress-nginx 只能访问 Control/Runtime 公共入口，不能访问内部 API、MySQL、Redis、ClickHouse或 Vault 管理端口。
- Observability 只能使用受限 Runtime Redis ACL、ClickHouse和自己的对象存储身份；不能读取 Runtime MySQL、Control数据或 Provider Secret。
- Control MySQL位于 Control；Runtime MySQL、Redis、ClickHouse位于 Runtime；Vault、MinIO、OpenSandbox位于 Dependencies。
- Runtime 应用不能直连公网；Model/MCP/Memory/RAG/HTTP/Remote Action/Poll/Lifecycle 只允许通过 Gateway 的 3128。Sandbox 默认无网络，双重显式开启后只允许 DNS 和 Gateway 的 3129 TLS 入口。
- Gateway 只允许 DNS 和 Profile 登记的公共 HTTPS 端口，应用层再次阻断私网、集群、Metadata、回环和保留地址；Gateway Secret 只有公钥/TLS，不含任何业务数据凭据。
- 不部署 Prometheus、Prometheus Adapter、Metrics Server、HPA 或 KEDA。

## 6. 升级、回滚与卸载

```powershell
.\scripts\deploy-v2.ps1 -Action Upgrade -Target All -ConfigFile deploy/profiles/v2-full-local.json
.\scripts\deploy-v2.ps1 -Action Rollback -Target Runtime -ConfigFile deploy/profiles/v2-full-local.json -PreviousReleaseManifest <manifest.json>
.\scripts\deploy-v2.ps1 -Action Uninstall -Target Observability -ConfigFile deploy/profiles/v2-full-local.json
```

`Uninstall -Target Observability` 只删除 Observability标签资源，不删除 Runtime工作负载、Runtime MySQL/Redis或共享 Runtime Namespace。普通卸载保留 Namespace；非生产 RunId 环境使用 `-PurgeTestResources` 且 `-Target All` 时，才会删除去重后的三个临时 Namespace和无人使用的受管 Ingress集群资源。

Runtime 仍存在带 `agentx.io/egress-client=managed` 的 Deployment 时，单独执行 `Uninstall -Target Dependencies` 会被拒绝；完整卸载应先移除 Runtime 再移除 Gateway。

Egress 四个调用身份使用独立私钥/KID。轮换先以 `Plan` 检查当前调用方，再执行 `Rotate`；脚本先发布新旧双公钥并滚动 Gateway，再逐个替换调用方私钥/KID，全部 Ready 后才删除旧公钥。中途失败会把调用方和 Gateway 恢复到旧 KID，且使用 Namespace 内互斥锁拒绝并发轮换。

```powershell
.\scripts\rotate-egress-keys.ps1 -Action Plan -ConfigFile deploy/profiles/v2-full-local.json
.\scripts\rotate-egress-keys.ps1 -Action Rotate -ConfigFile deploy/profiles/v2-full-local.json
```

本地测试数据允许全部重建时，可在完整 `Target All` 操作中使用 `-RecreateV2Data`。生产环境禁止该开关；PVC本身不是备份，状态型依赖切换必须另行执行停写、导出、恢复和验证。

## 7. 验收

```powershell
.\scripts\v2-profile-tests.ps1
.\scripts\v2-07-profile-tests.ps1 -SkipWebSourceBaseline
```

临时 Kubernetes E2E 通过 `-RunId` 创建并在结束时删除三个临时 Namespace。验收应确认：只有三个物理 Namespace、八类 Deployment 均为 1 副本、八个 PDB、零 HPA/指标栈资源，Observability/ClickHouse 位于 Runtime，Gateway/ingress-nginx 位于 Dependencies；公共 HTTPS 正向调用与私网/Metadata/直连公网负向矩阵通过，并完成 Migration、Bootstrap、Doctor、发布、Invocation、Worker 和 Trace 查询闭环。

公网出口专项 E2E 分成两个互不混淆的阶段：功能矩阵使用本机 Fixture 加固定摘要的 Cloudflare Quick Tunnel 容器，覆盖 Model、MCP JSON/SSE、Memory、RAG、HTTP Request、Remote Action、Poll、Lifecycle和私网/Metadata拒绝；长稳阶段通过 `-StabilityOnlyEndpoint` 直接连接稳定公共 HTTPS Endpoint，覆盖并发 Tunnel、Drain、滚动升级和连接回收，不把匿名 Tunnel 的可用性计入 Gateway 稳定性。成功或失败都会删除 Job/Pod、停止 Fixture并强制清理本次唯一命名的 Tunnel 容器。

Quick Tunnel 无可用性保证。脚本最多申请三个候选域名，每个都必须从部署主机通过公网 `/health` 后才创建 Kubernetes Job；候选失败不会重跑已经开始的业务矩阵。发布认证必须分别保存一次功能矩阵成功结果和一次 `-StabilityMinutes 30` 长稳结果。

直连公网拒绝是严格门禁，要求集群 CNI 实际执行 Kubernetes NetworkPolicy。当前 Docker Desktop 若使用不支持 NetworkPolicy enforcement 的 `kindnet`，该断言会正确失败；仅在已经单独记录此环境限制时，可显式添加 `-SkipDirectNetworkPolicyAssertion` 验证其余 Gateway 矩阵。生产验收不得使用该开关，必须在 Calico、Cilium 或等价 CNI 上执行严格模式。

```powershell
.\scripts\v2-egress-e2e.ps1 -ConfigFile deploy/profiles/v2-full-local.json -Concurrency 16
.\scripts\v2-egress-e2e.ps1 -ConfigFile deploy/profiles/v2-full-local.json -StabilityMinutes 30 -Concurrency 16 -StabilityOnlyEndpoint https://www.cloudflare.com/cdn-cgi/trace
```
