# Agentx V2 Kubernetes 部署

统一入口是 `scripts/deploy-v2.ps1`，Profile API 固定为破坏性的 `agentx.io/deployment/v2alpha2`。旧 `v2alpha1` 不做转换，校验时会明确拒绝。

## 1. 物理 Namespace 与逻辑 Plane

| 物理 Namespace | 逻辑 Plane / 组件 |
|---|---|
| `agentx-v2-control` | Control：`web-console`、`platform-control`、Control MySQL、Migration、Ingress |
| `agentx-v2-runtime` | Runtime + Observability：四个 Runtime Deployment、Runtime MySQL/Redis、`observability`、ClickHouse、各自 Migration/Doctor/Bootstrap |
| `agentx-v2-deps` | Dependencies：专用 ingress-nginx、Vault、MinIO、共享 Bootstrap；集群内部署的 OpenSandbox/LightRAG/Mem0 也属于此域 |

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

安装顺序为：Profile/工具预检、三个 Namespace、分域 Secret、Dependencies ingress-nginx、bundled 基础设施、Migration、Bootstrap/Doctor、七类应用 Deployment、Rollout 和独立 Release State。七类应用首次安装默认均为 1 副本；Upgrade/Rollback 保留集群中现有副本数。

## 4. Ingress

Control 与 Runtime 的 Ingress 对象留在各自业务 Namespace。Controller 的 Helm Release 固定为 `agentx-ingress-nginx`，安装在 Profile 的 Dependencies Namespace，IngressClass 使用 Profile 的 `ingress.className`，且不会设为默认类。

部署器使用 Helm Annotation 与 `agentx-ingress-ownership` ConfigMap 校验所有权；发现同名但未受管的 Release 或 IngressClass 会拒绝接管。卸载时先删除本 Target 的受管 Ingress，再扫描全集群使用者；只有无人使用且所有权匹配时才卸载 Controller 和 IngressClass。

RunId E2E 为每次运行生成三个临时 Namespace、独立 IngressClass 和独立 Helm 资源名，并将 Controller Service 设为 `ClusterIP`，避免并发临时环境争用宿主机 80/443。

## 5. 安全边界

- Control、Runtime Namespace 执行 Restricted Pod Security；Dependencies 按 ingress-nginx/OpenSandbox 所需权限配置。
- ingress-nginx 只能访问 Control/Runtime 公共入口，不能访问内部 API、MySQL、Redis、ClickHouse或 Vault 管理端口。
- Observability 只能使用受限 Runtime Redis ACL、ClickHouse和自己的对象存储身份；不能读取 Runtime MySQL、Control数据或 Provider Secret。
- Control MySQL位于 Control；Runtime MySQL、Redis、ClickHouse位于 Runtime；Vault、MinIO、OpenSandbox位于 Dependencies。
- 不部署 Prometheus、Prometheus Adapter、Metrics Server、HPA 或 KEDA。

## 6. 升级、回滚与卸载

```powershell
.\scripts\deploy-v2.ps1 -Action Upgrade -Target All -ConfigFile deploy/profiles/v2-full-local.json
.\scripts\deploy-v2.ps1 -Action Rollback -Target Runtime -ConfigFile deploy/profiles/v2-full-local.json -PreviousReleaseManifest <manifest.json>
.\scripts\deploy-v2.ps1 -Action Uninstall -Target Observability -ConfigFile deploy/profiles/v2-full-local.json
```

`Uninstall -Target Observability` 只删除 Observability标签资源，不删除 Runtime工作负载、Runtime MySQL/Redis或共享 Runtime Namespace。普通卸载保留 Namespace；非生产 RunId 环境使用 `-PurgeTestResources` 且 `-Target All` 时，才会删除去重后的三个临时 Namespace和无人使用的受管 Ingress集群资源。

本地测试数据允许全部重建时，可在完整 `Target All` 操作中使用 `-RecreateV2Data`。生产环境禁止该开关；PVC本身不是备份，状态型依赖切换必须另行执行停写、导出、恢复和验证。

## 7. 验收

```powershell
.\scripts\v2-profile-tests.ps1
.\scripts\v2-07-profile-tests.ps1 -SkipWebSourceBaseline
```

临时 Kubernetes E2E 通过 `-RunId` 创建并在结束时删除三个临时 Namespace。验收应确认：只有三个物理 Namespace、七类 Deployment 均为 1 副本、七个 PDB、零 HPA/指标栈资源，Observability/ClickHouse 位于 Runtime，ingress-nginx 位于 Dependencies，并完成 Migration、Bootstrap、Doctor、发布、Invocation、Worker 和 Trace 查询闭环。
