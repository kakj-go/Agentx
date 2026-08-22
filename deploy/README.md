# Agentx Helm + Python 部署手册

Agentx 使用四个独立 Helm Release 管理核心资源，并以 Python 3.12 CLI 在 Windows 和 Linux 上提供相同命令。旧脚本、JSON Profile 和核心 Kustomize 清单已经删除，不提供兼容入口。

## 1. 前置条件

部署主机必须预先安装：

- Python `3.12.x`；
- uv；
- Helm 3；
- kubectl；
- 可访问的 Kubernetes 集群。

CLI 只校验这些工具和版本，不下载 Helm/kubectl，也不修改用户机器。Docker 仅在执行 `build-images` 时需要。Python 和 uv 不进入任何 Agentx 业务容器。

```bash
uv sync --frozen
uv run --frozen agentx-deploy validate --values deploy/values/local.yaml
```

`validate --cluster` 额外读取集群能力和 existing Secret，不创建资源。`render` 只运行 Helm 本地渲染，不访问集群。

## 2. 目录边界

```text
deploy/
├── helm/
│   ├── agentx-control/
│   ├── agentx-runtime/
│   ├── agentx-observability/
│   └── agentx-dependencies/
├── values/
│   ├── local.yaml
│   ├── dockerhub-beta.yaml
│   ├── production.example.yaml
│   └── values.schema.json
├── python/agentx_deploy/
├── tests/
├── kustomize/
│   ├── addons/
│   └── e2e-fixtures/
├── ingress-nginx/
├── opensandbox/
└── release/
```

核心 Agentx 资源只能出现在四个 Chart 中。`deploy/kustomize/addons` 存放 LightRAG/Mem0，`deploy/kustomize/e2e-fixtures` 存放 Echo Provider 等临时测试资源；两者不得覆盖 Helm 的资源名称或 Selector。

## 3. Release 与 Namespace 所有权

| Target | Release | Namespace | 所有资源 |
|---|---|---|---|
| `dependencies` | `agentx-dependencies` | `global.namespaces.dependencies` | Egress Gateway、local/test Vault/MinIO |
| `control` | `agentx-control` | `global.namespaces.control` | Web、Platform Control、Control MySQL、Migration/Bootstrap/Doctor |
| `runtime` | `agentx-runtime` | `global.namespaces.runtime` | Gateway、Runtime、Worker、Sandbox Manager、Runtime MySQL/Redis |
| `observability` | `agentx-observability` | Runtime Namespace | Observability、ClickHouse、Migration/Bootstrap/Doctor |
| Ingress | `agentx-ingress-nginx` | Dependencies Namespace | 固定上游 ingress-nginx Chart |

Observability 与 Runtime 共用物理 Namespace，但使用不同 Release、ServiceAccount、Secret、标签、NetworkPolicy 和数据权限。OpenSandbox Server、Controller、RuntimeClass 与计算节点不属于这些 Release。

## 4. Values

四个 Chart 使用完全相同的顶层 Values 与 JSON Schema：

| 顶层字段 | 用途 |
|---|---|
| `global` | 环境、Namespace、镜像、Ingress、依赖、网络、Secret 和备份约束 |
| `control` | Web/Platform Control 副本、角色和连接池 |
| `runtime` | Gateway/Runtime/Worker/Sandbox Manager 副本、角色和连接池 |
| `observability` | Observability 副本与角色 |
| `dependencies` | Egress Gateway 副本 |

所有命令必须显式提供 `--values`，没有隐式环境默认值。

- `local.yaml`：本地镜像、bundled MySQL/Redis/ClickHouse/MinIO/Vault、自动生成 Secret。
- `dockerhub-beta.yaml`：公开 Beta 镜像，其余本地依赖与 `local` 相同。
- `production.example.yaml`：外部状态依赖、HTTPS/私有 CA、existing Secret 和镜像摘要示例。

production 强制使用外部 MySQL、Redis、ClickHouse、S3、Vault 和 OpenSandbox；MySQL 必须 `verify_identity`，Redis 必须 `rediss://`，其他 Endpoint 必须 HTTPS，并为每项配置 `caSecretName`。Sandbox Egress 只接受 AWS/Azure/GCP 的已知内部 LoadBalancer Annotation。

## 5. 安装

完整安装顺序固定为：

1. 校验 Values、Python/uv/Helm/kubectl、集群连接、Namespace 和生产门禁。
2. production 在创建资源前验证权威 Secret、工作负载 Secret、CA/TLS 和镜像拉取 Secret的名称及 Key。
3. 创建所选 Target 对应的三个物理 Namespace及安全标签。
4. local/test 查询或创建权威 Secret，并只发布所选 Target 所需镜像 Secret；重复安装不会更换持久密钥。
5. 安装固定 Release `agentx-ingress-nginx`。
6. 安装 Dependencies，等待 Egress Gateway 和 bundled 依赖 Ready。
7. 依次安装 Control、Runtime、Observability。
8. 等待 Migration Job、Bootstrap Job、Init Container、Deployment、StatefulSet 和 PDB。
9. 主动执行每个 Release 的 Helm Test/Doctor。
10. 输出 Release Revision、镜像引用、Namespace 和访问入口；production 另外写入 `artifacts/releases/` Release Manifest。

```bash
# local
uv run --frozen agentx-deploy install --values deploy/values/local.yaml

# Docker Hub Beta
uv run --frozen agentx-deploy install --values deploy/values/dockerhub-beta.yaml

# production：先复制示例并替换所有 Endpoint、摘要、CA 和 Secret
uv run --frozen agentx-deploy validate --values deploy/values/production.yaml --cluster
uv run --frozen agentx-deploy render --values deploy/values/production.yaml > artifacts/production-render.yaml
uv run --frozen agentx-deploy install --values deploy/values/production.yaml --output json
```

`--target control|runtime|observability|dependencies|all` 默认 `all`。独立 Target 只验证其依赖；例如 Runtime 要求 Dependencies Release 已存在，不会自动安装或升级 Dependencies。

## 6. 命令契约

所有命令支持 `--output text|json`，失败返回非零退出码；JSON 模式的错误也输出 JSON。命令、输出和异常会脱敏 Password、Token、Secret、私钥和带凭据 URL。

### Validate 与 Render

```bash
uv run --frozen agentx-deploy validate --values deploy/values/local.yaml
uv run --frozen agentx-deploy validate --values deploy/values/production.yaml --cluster --output json
uv run --frozen agentx-deploy render --values deploy/values/local.yaml --target runtime
```

`render` 不访问集群。`validate --cluster` 只读集群，不 Apply 资源。

### Status 与 Doctor

```bash
uv run --frozen agentx-deploy status --values deploy/values/local.yaml --output json
uv run --frozen agentx-deploy doctor --values deploy/values/local.yaml --target all
```

Status 聚合 Helm Revision/状态、Deployment/StatefulSet/Job/PDB、配置镜像和 Ingress Endpoint。Doctor 运行对应 Release 的 Helm Test Job，检查适用的 MySQL、Redis、ClickHouse、对象存储、Vault、Egress Gateway，以及带 API Key 与可选 CA 的 OpenSandbox `/health` 链路。

### Upgrade 与 Rollback

```bash
uv run --frozen agentx-deploy upgrade --values deploy/values/local.yaml --target runtime
uv run --frozen agentx-deploy rollback --values deploy/values/local.yaml --target runtime --revision 2
```

Upgrade 在调用 Helm 前读取当前 Deployment 副本数，并作为本次显式 Helm Override保留。Rollback 必须指定单一 Target 和明确 Revision；完整回滚由操作者按 Target逐个执行。Rollback 完成后自动运行该 Release 的 Doctor。

### Uninstall

```bash
# 保留 Namespace、PVC和外部资源
uv run --frozen agentx-deploy uninstall --values deploy/values/local.yaml --target observability

# 只允许 local/test 且必须为 all
uv run --frozen agentx-deploy uninstall --values deploy/values/local.yaml --target all --purge-data --yes
```

Runtime Release 仍存在时拒绝单独卸载 Dependencies。IngressClass 仍有使用者时保留 ingress-nginx；数据清理模式遇到使用者会失败。production 永远拒绝 `--purge-data`。

## 7. Migration 与 Bootstrap

Migration 是带 Helm Release Revision 后缀的普通 Job，不使用安装前 Hook，因此升级不会修改不可变 Job。Job先等待数据库，Rust Migration 使用数据库锁保证并发唯一性；应用 Pod使用轻量 Init Container 等待目标 Schema Version。

Helm 始终使用 `--atomic --wait --wait-for-jobs`。Bootstrap 每个首次安装只创建一个 Job；Job在 Migration 完成前有界重试，幂等双重执行由 `tests/e2e/infrastructure` 验证。

手工 Expand 和 Contract 门禁：

```bash
uv run --frozen agentx-deploy migrate --values deploy/values/local.yaml --target runtime --phase expand
uv run --frozen agentx-deploy migrate --values deploy/values/local.yaml --target runtime --phase contract
```

Contract 会拒绝仍有不可用旧 ReplicaSet 的发布。失败不得继续发布。

## 8. Secret 与密钥轮换

`global.secrets.dependencies` 是共享签名材料的权威 Secret。local/test 使用 `cryptography` 生成 RSA、Ed25519、密码和 Egress TLS，先读取已有权威 Secret，重复 Install/Upgrade 保持原值。Helm模板只引用 Secret 名称与 Key。

production 必须预先创建：

- Dependencies 权威 Secret；
- `global.secrets.workloads.*` 最小权限工作负载 Secret；
- 外部依赖 CA Secret（Key 为 `ca.crt`）；
- Ingress TLS、Egress TLS/CA 和可选镜像拉取 Secret。

管理员修改权威值后执行：

```bash
uv run --frozen agentx-deploy sync-secrets --values deploy/values/production.yaml --target all
```

同步只覆盖镜像 Secret 中与权威 Secret 同名的共享 Key，保留工作负载本地凭据，然后滚动所有消费者并等待 Ready。普通 Install/Upgrade 不轮换持久密钥。

Egress 轮换：

```bash
uv run --frozen agentx-deploy rotate-egress-keys --values deploy/values/production.yaml --action plan
uv run --frozen agentx-deploy rotate-egress-keys --values deploy/values/production.yaml --action rotate
```

Rotate 使用集群互斥锁，执行“发布新旧双公钥 → Gateway Ready → 四个调用方逐个切换 → 删除旧公钥 → Gateway Ready”。任一步失败会恢复权威、Gateway 和所有调用方 Secret并重新滚动。

## 9. 备份与恢复

CLI 不嵌入云厂商 SDK。外部 Adapter 接收语言无关参数并只返回五字段 JSON Receipt；Python Adapter 会由当前 Python 解释器执行，因此 Windows/Linux 命令一致。

```bash
uv run --frozen agentx-deploy backup \
  --values deploy/values/production.yaml \
  --data-target control-mysql \
  --backup-id release-20260822 \
  --adapter tools/provider-adapter.py

uv run --frozen agentx-deploy restore \
  --values deploy/values/production.yaml \
  --data-target control-mysql \
  --backup-id release-20260822 \
  --restore-target control-db-restore.example.internal \
  --adapter tools/provider-adapter.py
```

Receipt 会校验字段白名单、RPO/RTO 和 `deploy/release/backup-manifest.schema.json`，证据默认写入 `artifacts/data-operations/`。原地恢复必须额外提供 `--allow-in-place-restore`。

## 10. 镜像构建

```bash
# 构建全部 Agentx 镜像并导入本地 Kubernetes
uv run --frozen agentx-deploy build-images --values deploy/values/local.yaml

# 只构建并推送指定服务
uv run --frozen agentx-deploy build-images --values deploy/values/local.yaml \
  --service platform-control --service runtime-gateway --push --skip-kubernetes-import
```

production Values 禁止本地构建。Docker Desktop 使用 control-plane containerd导入；其他本地集群使用短生命周期、唯一命名的 loader Pod，结束后强制清理。

## 11. Kustomize Addon 与 Fixture

Addon 和 Fixture 不随核心 Install 自动部署：

```bash
kubectl -n agentx-deps apply -k deploy/kustomize/addons/lightrag
kubectl -n agentx-deps apply -k deploy/kustomize/addons/mem0
kubectl -n agentx-deps apply -k deploy/kustomize/e2e-fixtures/runtime-providers
```

pytest 的 `e2e_providers` Fixture 会按需安装最后一项。静态测试会同时渲染 Helm/Kustomize，并拒绝资源名称或 Helm 所有权重叠。

## 12. E2E

领域 Marker：`infrastructure`、`publishing`、`gateway`、`runtime`、`observability`、`security`、`upgrade`、`product`。每次运行生成 Run ID、三个 Namespace、独立 IngressClass、命令时间线、资源/事件/日志和 Playwright 报告。

```bash
uv run --frozen pytest tests/e2e --values deploy/values/local.yaml -m infrastructure
uv run --frozen pytest tests/e2e --values deploy/values/local.yaml -m "security or upgrade"
uv run --frozen pytest tests/e2e --values deploy/values/local.yaml -m product --keep-on-failure
```

`--scale-down-development` 会暂时把常驻开发 Deployment缩容为 0，并在 `finally` 恢复原副本。默认无论成功失败都清理临时 Namespace；只有失败且显式提供 `--keep-on-failure` 才保留现场。后台 port-forward 使用跨平台进程组管理并在结束时停止。

严格 NetworkPolicy 验收必须使用 Calico、Cilium 或等价执行策略的 CNI。Docker Desktop 不支持策略执行时只能运行明确标注的非生产子集，不能形成生产安全证据。

## 13. 统一门禁与 CI

```bash
uv sync --frozen --extra test
uv run --frozen agentx-check --fast
uv run --frozen agentx-check
```

`agentx-check` 聚合 uv 锁文件、ruff、pytest、四 Chart lint/template、Kustomize 渲染、2000 行限制、Rust fmt/clippy/test、架构边界、Web lint/test/build 和 `git diff --check`。GitHub Actions在 Windows/Linux 运行 Python/CLI/Helm矩阵；带 `agentx-e2e` 标签的受管 Runner运行完整 Linux严格 CNI和 Windows Docker Desktop闭环。
