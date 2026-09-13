# Kubernetes 部署、扩展与可靠性

## 1. 部署事实来源

Agentx 核心 Kubernetes 资源以 Helm 为唯一事实来源。部署主机通过 Rust原生 `agentxctl`运行，Windows 与 Linux 使用相同参数和行为。CLI嵌入固定版本Chart/Schema，调用 Helm/kubectl并解析 JSON输出，不实现第二套 Kubernetes Client，也不下载前置工具。

四个逻辑域对应四个独立 Release：

| Plane | Release | Namespace |
|---|---|---|
| Dependencies | `agentx-dependencies` | Dependencies Namespace |
| Control | `agentx-control` | Control Namespace |
| Runtime | `agentx-runtime` | Runtime Namespace |
| Observability | `agentx-observability` | Runtime Namespace |

ingress-nginx 使用固定上游 Chart和独立 Release `agentx-ingress-nginx`，位于 Dependencies Namespace。Observability 虽与 Runtime 共用 Namespace，仍独立拥有 ServiceAccount、Secret、NetworkPolicy、Migration、Doctor 和 Helm历史。

本地正式部署使用 ingress-nginx 的 LoadBalancer Service，在 Docker Desktop 提供 `http://agentx.localhost` 与 `http://run.agentx.localhost` 入口。`test` 环境及带 Run ID 的临时 E2E 部署使用 ClusterIP，并由测试进程 port-forward，避免争用正式环境的 80/443 端口。此差异只决定入口暴露方式，不改变三平面依赖边界。

Kustomize 只管理 LightRAG/Mem0 Addon与临时 E2E Fixture。核心 Helm与 Kustomize资源不得重名、使用同一 Selector或声明相同 Helm所有权。

## 2. Values 契约

环境配置是 YAML，顶层固定为 `global`、`control`、`runtime`、`observability`、`dependencies`。CLI 内嵌与版本绑定的 Docker Hub Beta Values，常规命令省略 `--values` 时使用该配置完成单文件快速部署；自定义和 production 配置必须显式提供。四个 Chart携带相同 `values.schema.json`，测试保证公共定义无漂移。

`global` 组合：

- 三个物理 Namespace和两个 Ingress Host；
- Registry、Repository Prefix、Tag/Digest、Pull Policy和可选 Pull Secret；
- bundled/external MySQL、Redis、ClickHouse、S3、Vault和外部 OpenSandbox；
- Egress Gateway端口、Sandbox私有入口、固定外部依赖 CIDR；
- 权威 Secret、工作负载 Secret和备份 RPO/RTO。

production 必须使用镜像摘要、existing Kubernetes Secret、外部状态依赖、HTTPS/私有 CA、MySQL `verify_identity`、`rediss://` 和已知云厂商内部 LoadBalancer Annotation。外部资源在安装、升级和卸载中都不被创建、修改或删除。

## 3. 安装和发布顺序

安装顺序是架构契约：

1. Values/工具/集群/Namespace/生产门禁。
2. production 只读校验所有权威、工作负载、CA/TLS和镜像 Secret，缺项时在创建资源前失败。
3. 创建或验证所选 Target的 Namespace及 Pod Security标签。
4. local/test创建或复用权威 Secret，并发布最小镜像 Secret。
5. 安装 ingress-nginx。
6. 安装 Dependencies并等待 Egress Gateway、Vault、MinIO等适用依赖 Ready。
7. 安装 Control、Runtime、Observability。
8. 等待 Migration、Bootstrap、Init Container、Deployment、StatefulSet和 PDB。
9. 执行每个 Release的 Helm Test/Doctor。
10. 输出 Revision、镜像、Namespace和访问入口；production保存 Release Manifest。

所有 Helm发布使用 `--atomic --wait --wait-for-jobs`。单 Target操作只检查前置 Release，不隐式修改其他 Target。Upgrade从集群读取当前 Deployment副本并通过本次 Helm Override保留；扩缩容必须由操作者显式执行。

## 4. Migration、Bootstrap 与契约窗口

Migration Job是普通 Helm资源，名称包含 Release Revision，不使用安装前 Hook。Job等待数据库并使用数据库锁保证并发唯一执行。应用 Pod的 Init Container查询目标 Schema Version，Schema可用前不启动业务容器。

Bootstrap在首次安装每个 Release时只创建一次，Migration完成前由同一 Job有界重试。Bootstrap必须幂等，双重执行作为 E2E不变量验证。

Expand Migration可手工创建一次性 Revision外 Job。Contract门禁检查旧 ReplicaSet、协议兼容窗口和 Release状态；任何失败都中止发布。Rollback只接受单 Target和明确 Helm Revision，不伪造跨 Release的“整体 Revision”。

## 5. Secret 权威源

Dependencies Namespace中的 `agentx-dependencies-secrets` 是共享 JWT、Bundle、Work Package、User签名、Egress Key/KID、Egress TLS和 Observability Redis材料的权威源。Pod不能跨 Namespace引用 Secret，因此每个工作负载使用本 Namespace的最小镜像 Secret。

- local/test：Rust共享密钥材料库生成RSA、Ed25519和TLS材料；先查权威 Secret，重复 Install/Upgrade保持原值。
- production：只接受预先创建的权威、工作负载和外部依赖 Secret。
- Helm：只渲染 Secret名称和 Key，不生成或承载明文。
- 普通 Install/Upgrade：不隐式轮换持久密钥。
- `sync-secrets`：同步与权威源同名的共享 Key并滚动消费者。
- `rotate-egress-keys`：互斥执行双公钥重叠、Gateway Ready、调用方逐个切换、旧公钥删除；失败恢复并重新滚动。

## 6. 外部依赖与 CA

Control、Runtime和 Observability Chart通过投影 Secret把私有 CA只读挂载至 `/etc/agentx-ca`，并设置各 Rust Client的 CA Path。生产不允许关闭证书校验。

- Control：Control MySQL、S3、Vault CA。
- Runtime：Runtime MySQL、Redis、S3、Vault、OpenSandbox CA。
- Observability：ClickHouse、受限 Redis、S3 CA。
- Sandbox Egress：独立 TLS/CA Secret和单一私有入口。

外部 S3 Bucket必须预创建。PVC不是备份。备份/恢复由外部 Adapter执行，主 CLI只做安全前置、Receipt白名单、RPO/RTO与 JSON Schema验证。

## 7. 网络与安全

- Control/Runtime执行 Restricted Pod Security；Dependencies仅为 ingress/OpenSandbox所需边界放宽。
- Runtime业务 Pod不能直连公网，Model/MCP/Memory/RAG/HTTP等动态流量经 Gateway `3128`。
- Sandbox默认断网；显式允许时只访问 Gateway `3129` TLS入口。
- Gateway应用层和 NetworkPolicy共同拒绝私网、回环、Metadata、保留网段和未批准端口。
- Runtime Gateway在服务层执行浏览器CORS：production只接受配置中的Control Origin，local/test允许临时port-forward Origin；Ingress继续保留同源白名单作为外层防护。
- Observability无 Runtime MySQL凭据，只使用受限 Redis ACL、ClickHouse和独立对象存储身份。
- 核心部署不引入 Prometheus、指标 Adapter、HPA、KEDA、Operator或 GitOps控制器。

严格 NetworkPolicy认证必须在实际执行策略的 CNI上完成；不支持策略执行的本地集群不能形成生产安全证据。

## 8. 健康、扩展和故障恢复

Readiness表示必需依赖与 Schema可用，Liveness只表示进程存活：

- Platform Control无状态扩展，Control MySQL保存权威管理状态。
- Runtime Gateway通过 Runtime MySQL持久化游标，Redis仅唤醒。
- Workflow Runtime多副本使用 MySQL状态条件、Claim/Lease/Fencing和 Outbox。
- Worker按 Capability扩展，并依赖 Runtime MySQL、Redis、S3和 Runtime API。
- `plugin_nodejs` Worker镜像固定Node.js 24.20.0和Runner摘要；启动时校验Node主版本与Runner文件。`pluginMaxProcesses`限制Node总进程数，`pluginIdleSeconds`控制按包摘要复用后的空闲回收。Linux使用进程组，Windows本地使用Job Object回收整棵进程树。
- Sandbox Manager多副本共享 MySQL Lease并接入独立 OpenSandbox。
- Observability消费受限 Redis Trace Stream写入 ClickHouse；ClickHouse故障不阻断 Execution提交。

Kubernetes重启不能替代幂等、Lease、Fencing、Outbox和恢复逻辑。

## 9. 卸载与数据保护

普通 Uninstall删除对应 Helm Release，但保留 Namespace、PVC和外部资源。Observability卸载不删除 Runtime Release或共享 Namespace。Runtime仍存在时拒绝单独卸载 Dependencies。

只有 local/test、`Target=all`且同时提供 `--purge-data --yes` 时才删除三个 Namespace；production直接拒绝。IngressClass仍有使用者时保留 Controller，Purge模式则失败并要求先处理使用者。

## 10. E2E 和质量门禁

pytest负责临时集群环境、安装/升级/回滚/Doctor/清理、port-forward、日志事件和证据。领域 Marker为 infrastructure、publishing、gateway、runtime、observability、security、upgrade、product。TypeScript Playwright继续负责 UI操作，不改写为 Python浏览器测试；OpenSandbox官方 Go SDK差分 Oracle继续保留 Go。

静态门禁覆盖：ruff、pytest、Values Schema、四 Chart lint/template、Kustomize渲染、资源所有权冲突、表/API/Claim契约、架构边界、2000行限制、Rust/Web测试和 `git diff --check`。运行命令与完整 Runbook见[部署手册](../deploy/README.md)。
