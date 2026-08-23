# Agentx SaaS 受控公网出口验收证据

> 历史命令说明：本文中的 PowerShell、Shell、deployment Profile 和核心 Kustomize 命令只记录当时验收事实，相关入口已经删除，不得作为当前操作入口；当前部署使用 `agentxctl`，仓库门禁使用 `cargo xtask check`，集群验收使用 `pytest tests/e2e`。

> 历史环境快照：本文中的 `agentx-v2-*` Namespace 仅用于记录当时的验收环境，不代表当前部署规范。

状态：本地功能、部署生命周期、密钥轮换和稳定性验收已完成；生产 NetworkPolicy enforcement 门禁仍为 `in_progress`，不得把当前 Docker Desktop `kindnet` 环境视为生产强隔离证据。

## 1. 已实现范围

- 部署 Profile 已升级为破坏性的 `agentx.io/deployment/v2alpha3`；`v2alpha2` 及更早版本明确拒绝。
- `agentx-egress-gateway` 以独立 Deployment、ServiceAccount、Secret、Service、PDB和 NetworkPolicy 部署在 Dependencies Namespace，默认 1 副本。
- Runtime Gateway、Workflow Runtime、Workflow Worker使用目标绑定、角色绑定、最长 60 秒的 RS256 CONNECT Token；Sandbox使用独立 KID、TLS入口和 TTL/并发/连接次数/累计时长预算。
- Model、MCP、Memory、RAG、HTTP Request、Remote Action、Poll和 Lifecycle使用统一 `ProviderHttpClient`；基础设施 Client仍走固定内部依赖。
- Gateway逐次解析并固定已验证 IP，拒绝私网、集群、Kubernetes API、Metadata、回环、链路本地、保留地址、非法端口、错误角色、过期或重放 Token。
- Sandbox Profile与 Code节点使用 `none|public_https` 强类型契约，默认 `none`；只有双重开启时才注入受限 HTTPS代理。
- 部署器支持 Gateway安装、升级、状态、Doctor、回滚、卸载保护和四个调用身份的新旧公钥重叠轮换。
- E2E将短时功能矩阵与长稳测试分离：功能矩阵使用固定摘要的 Cloudflare Quick Tunnel；长稳测试通过 `StabilityOnlyEndpoint` 直接连接稳定公共 HTTPS Endpoint，避免匿名 Tunnel可用性污染 Gateway稳定性结论。

## 2. Kubernetes 实测

### 2.1 公网功能矩阵

Run `egress-b3a84194ab` 为 `passed`：

```text
concurrency                  16
rounds                        1
transientFixtureRetries       0
positive                      Model、MCP JSON、MCP SSE、Memory、RAG、
                              HTTP Request、Remote Action、Poll、Lifecycle
negative                      private-ip、metadata
```

该 Run使用临时公共 HTTPS Fixture；结束后 Job、Pod、本机 Fixture和唯一命名 Tunnel容器均已清理。

### 2.2 30 分钟并发稳定性

Run `egress-2ba735d993` 为 `passed`：

```text
mode                          stability-only
endpoint                      https://www.cloudflare.com/cdn-cgi/trace
duration                      30 minutes
concurrency                   16
rounds                        148
stabilityIntervalSeconds      10
transientFixtureRetries        2
```

Run结束时没有 Smoke Job失败、Pod重启或测试资源残留。受限重试只覆盖连接建立失败、超时和 Fixture 5xx，不重放已经发送的业务 HTTP请求。

### 2.3 Drain 与滚动升级

Run `egress-e93c3cae4c` 在 16 并发 Tunnel期间执行 `rollout restart deployment/agentx-egress-gateway`，结果为 `passed`：

```text
duration                       2 minutes
rounds                        10
transientFixtureRetries       44
gateway after rollout          desired=1 ready=1 updated=1 available=1
gateway pod restarts           0
```

新 Pod Ready后旧 Pod完成 Drain并删除；Rollout期间的连接建立失败由有界重试吸收，Job持续运行并正常完成。

### 2.4 三 Namespace 生命周期

临时 RunId `egress0818` 已验证：

- 三个临时 Namespace完成 Install、Doctor All、Status和公网快速矩阵。
- `Uninstall Observability` 未删除 Runtime四个 Deployment、Runtime MySQL/Redis或共享 Runtime Namespace，随后 Observability Upgrade恢复成功。
- Runtime仍引用 Gateway时，`Uninstall Dependencies` 被正确拒绝。
- 完整清理后不存在带 `egress0818` 的 Namespace、IngressClass、ClusterRole、ClusterRoleBinding、Webhook或 Pod残留。

## 3. 真实模型与密钥轮换

- 模型 Alias：`gpt-5.6-sol`
- Endpoint：`https://chat.ekti.cc/v1`
- 连接状态：`healthy`
- 延迟：约 `4829 ms`
- 实际链路：Control → Runtime → Egress Gateway → Provider

密钥轮换 `egress-20260818115755-f4f99b1b` 已成功执行。Runtime Gateway、Workflow Runtime、Workflow Worker和 Sandbox Manager均切换到各自的新 KID；Gateway最终只保留这四把当前公钥，没有保留旧私钥或旧公钥。证据不保存 Provider Key、JWT私钥或 Secret明文。

## 4. 正式开发集群状态

- 物理 Namespace仅为 `agentx-v2-control`、`agentx-v2-runtime`、`agentx-v2-deps`。
- 8 个 Agentx常驻 Deployment均为 `1/1`；专用 ingress-nginx Controller另为 `1/1`。
- 8 个 Agentx PDB齐全。
- Agentx三 Namespace中没有 HPA/KEDA；全集群未发现 Prometheus、Prometheus Adapter、Metrics Server或 KEDA Deployment/CRD。
- `deploy-v2.ps1 -Action Doctor -Target All` 返回 `healthy`。
- 正式 PVC、数据库、业务 Secret和三个 Namespace在专项验收中均未删除。

## 5. 自动化门禁

以下最终验证通过：

```text
scripts/check.ps1
scripts/v2-profile-tests.ps1
scripts/v2-07-profile-tests.ps1 -SkipWebSourceBaseline
cargo fmt --all -- --check
cargo clippy -p agentx-v2-runtime --bin egress-smoke -- -D warnings
git diff --check
```

`scripts/check.ps1` 同时覆盖 Gateway 10 项地址/JWT/重放/预算测试、Runtime Egress与 Sandbox契约、边界检查、Workspace测试、185项 Web测试、前端生产构建、Contracts/OpenAPI漂移、Profile/Kustomize和2000行门禁。

## 6. 尚未关闭的生产门禁

当前 Docker Desktop Kubernetes使用 `kindnet`，不执行 Kubernetes NetworkPolicy。严格 E2E Run `egress-906cca4175` 实际观测到 Runtime Pod可绕过 Gateway连接 `1.1.1.1:443`，因此本地长稳 Run只能显式使用 `-SkipDirectNetworkPolicyAssertion`；该开关不改变 Gateway应用层的私网和 Metadata拒绝测试。

生产发布必须在 Calico、Cilium或等价支持 NetworkPolicy enforcement的 CNI 上运行不带跳过开关的严格矩阵，并验证 Runtime直连公网失败、Gateway只访问公共 HTTPS、Sandbox不能绕过代理。生产私有 LoadBalancer、来源 CIDR、云内部 LB Annotation，以及 gVisor/Kata Sandbox强隔离也必须在目标生产环境完成实测；本地代码和静态清单通过不等于这些外部环境门禁已经关闭。
