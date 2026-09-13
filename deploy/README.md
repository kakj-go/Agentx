# Agentx Helm + agentxctl 部署手册

Agentx 使用四个独立 Helm Release 管理核心资源，使用 Rust 原生 `agentxctl` 在 Windows x64 与 Linux x64 提供相同命令。部署入口不依赖 Python、uv 或源码仓库。

## 前置条件

- 对应平台的 `agentxctl` 单文件二进制或完整发布包；
- Helm 3；
- kubectl；
- 可访问的 Kubernetes 集群；
- Docker 仅用于开发期 `cargo xtask images`。

Release 同时提供可直接下载的 Windows/Linux 单文件二进制和完整归档包。归档包含 `agentxctl`、三个 Values 示例和许可证；四个 Agentx Chart、JSON Schema、Docker Hub Beta Values、ingress-nginx Chart 与 Values 已嵌入二进制，安装时不会下载部署资源。

```bash
agentxctl validate
agentxctl install
```

## Release 与 Namespace

| Target | Release | Namespace | 内容 |
|---|---|---|---|
| `dependencies` | `agentx-dependencies` | Dependencies Namespace | Egress Gateway、local/test Vault/MinIO |
| `control` | `agentx-control` | Control Namespace | Web、Platform Control、Control MySQL |
| `runtime` | `agentx-runtime` | Runtime Namespace | Gateway、Runtime、Worker、Sandbox Manager、MySQL/Redis |
| `observability` | `agentx-observability` | Runtime Namespace | Observability、ClickHouse |
| Ingress | `agentx-ingress-nginx` | Dependencies Namespace | 固定 ingress-nginx 4.15.1 |

Kustomize只管理可选Addon与E2E Fixture，不拥有核心资源。OpenSandbox独立安装。

## Values

常规集群命令未提供 `--values` 时使用与 CLI 版本严格绑定的内嵌 `dockerhub-beta.yaml`，用于单文件快速部署。自定义和 production 部署必须显式提供 `--values`。顶层固定为 `global`、`control`、`runtime`、`observability`、`dependencies`。

- `local.yaml`：本地镜像、bundled状态依赖、自动生成Secret。
- `dockerhub-beta.yaml`：公开Beta镜像和本地依赖。
- `production.example.yaml`：外部状态依赖、TLS、existing Secret、镜像摘要示例。

production另外强制要求 `global.images.sourceCommit` 为镜像对应的40位小写Git SHA。Release Manifest使用该值，不读取本机Git仓库。

## 安装与状态

```bash
agentxctl validate
agentxctl install
agentxctl status --output json
agentxctl doctor
agentxctl validate --values values/local.yaml
agentxctl validate --values values/production.yaml --cluster --output json
agentxctl render --values values/production.yaml > artifacts/production-render.yaml
```

安装顺序固定为Values/工具/集群/Secret校验、Namespace、local Secret、ingress-nginx、Dependencies、Control、Runtime、Observability和Helm Doctor。Helm发布使用`--atomic --wait --wait-for-jobs`。

`--target control|runtime|observability|dependencies|all`默认为`all`。单Target操作只检查依赖，不隐式升级其他Release。

## Upgrade、Rollback 与 Migration

```bash
agentxctl upgrade --values values/local.yaml --target runtime
agentxctl rollback --values values/local.yaml --target runtime --revision 2
agentxctl migrate --values values/local.yaml --target runtime --phase expand
agentxctl migrate --values values/local.yaml --target runtime --phase contract
```

Upgrade保留集群中的显式副本数。Rollback必须指定一个Target和Revision，并在完成后运行Doctor。Contract Migration在旧ReplicaSet未就绪时拒绝执行。

## Secret

local/test首次安装生成RSA、Ed25519、TLS、数据库和依赖密码；重复安装复用权威Secret。production只验证预创建Secret。

```bash
agentxctl sync-secrets --values values/production.yaml
agentxctl rotate-egress-keys --values values/production.yaml --action plan
agentxctl rotate-egress-keys --values values/production.yaml --action rotate
```

Egress轮换执行双公钥重叠、Gateway Ready、调用方逐个切换、旧公钥删除；任一步失败恢复权威、Gateway和调用方Secret。

## 备份与恢复

Provider Adapter必须是可直接执行的程序，接收语言无关参数并在stdout只返回五字段JSON Receipt。`agentxctl`不识别脚本后缀，也不调用语言解释器。

```bash
agentxctl backup --values values/production.yaml \
  --data-target control-mysql --backup-id release-20260823 --adapter ./provider-adapter

agentxctl restore --values values/production.yaml \
  --data-target control-mysql --backup-id release-20260823 \
  --restore-target control-db-restore.example.internal --adapter ./provider-adapter
```

证据默认写入当前目录的`artifacts/data-operations/`。原地恢复必须额外提供`--allow-in-place-restore`。

## 卸载

```bash
agentxctl uninstall --target observability
agentxctl uninstall --target all --purge-data --yes
```

普通卸载保留Namespace、PVC和外部资源。数据清理只允许local/test、`--target all`并要求`--yes`；production永远拒绝。Runtime仍存在时拒绝单独卸载Dependencies。

## 开发与 E2E

本地完整重建和升级：

```bash
uv run --frozen --group test python tools/scripts/dev/local_upgrade.py
```

该入口构建 11 个正式镜像（包含 Migration、Bootstrap、Doctor），导入本地 Kubernetes，执行 Helm 升级并重启使用 `dev` 标签的应用 Deployment。部署失败或 rollout 未就绪时返回失败。默认不构建 Echo E2E 镜像。

正式本地部署通过 LoadBalancer 暴露 `http://agentx.localhost`；Runtime API 使用 `http://run.agentx.localhost`，其根路径无业务路由时返回 404 属于正常行为。临时 E2E 保持 ClusterIP 和独立 port-forward。

开发期统一命令：

```bash
cargo xtask images --values deploy/values/local.yaml
cargo xtask check --fast
cargo xtask check
uv sync --frozen --group test
uv run --frozen --group test pytest tests/e2e --values deploy/values/local.yaml -m infrastructure
```

pytest负责临时Kubernetes环境、port-forward、故障注入、证据收集和调用TypeScript Playwright。Python不是部署依赖，也不进入Agentx业务容器。
