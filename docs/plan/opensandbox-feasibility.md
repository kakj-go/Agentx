# OpenSandbox 可行性评估

评估日期：2026-08-04。评估对象为 [opensandbox-group/OpenSandbox](https://github.com/opensandbox-group/OpenSandbox)，源码快照 `e95681e791b33b3893033940cbeaa5ab192bf21b`（2026-08-02）。

## 1. 结论

当前机器支持 OpenSandbox 的本地开发和 M5 功能 E2E：OpenSandbox Server 可以连接 Docker Desktop Linux Engine，并通过 Docker Runtime + runc 创建、执行和销毁 Sandbox。

当前机器不满足生产级强隔离门禁：Docker Engine 仅发现 runc，没有 gVisor 或 Kata。生产必须迁移到 OpenSandbox Kubernetes Runtime，并使用 gVisor、Kata 或经安全评审的等价 RuntimeClass。

因此采用以下分层：

| 场景 | 结论 | 运行形态 |
|---|---|---|
| 本机开发 | 支持 | Windows/WSL2 + Docker Desktop + OpenSandbox Docker Runtime + runc |
| 临时 Kubernetes E2E | 支持 | Agentx 在 Docker Desktop Kubernetes，OpenSandbox Server 在主机，Sandbox 在 Docker Engine |
| 生产多租户 | 当前机器不作为验收环境 | 独立 Kubernetes 计算节点 + OpenSandbox Kubernetes Runtime + 强隔离 RuntimeClass |

## 2. 当前环境

| 项目 | 实测值 |
|---|---|
| 操作系统 | Windows 10 Pro 64 位，Hypervisor 已启用 |
| WSL | WSL2，Linux Kernel `6.6.87.2-microsoft-standard-WSL2` |
| CPU / 主机内存 | AMD Ryzen 7 9800X3D，16 logical CPU，约 32 GB |
| Docker Desktop | 4.77.0 |
| Docker Engine | 29.5.3，Linux/amd64，16 CPU，约 16 GB，Runtime 仅 runc |
| Kubernetes | Docker Desktop，Linux/amd64，16 CPU，约 15.9 GB |
| Python | 3.12 |

OpenSandbox 官方本地要求为 Docker 和 Python 3.10+，并明确支持 Windows + WSL2；当前机器满足要求。`uv` 不是运行前提，可通过 pip/venv 安装，项目后续可在脚本中选择固定安装方式。

## 3. 实机验证

验证安装：

- `opensandbox-server==0.2.2`
- `opensandbox==0.1.15`
- `opensandbox-code-interpreter==0.1.2`
- `opensandbox/execd:v1.0.21`
- `opensandbox/code-interpreter:v1.1.0`
- `opensandbox/egress:v1.1.4`

已通过：

1. OpenSandbox Server 连接 Windows Docker Desktop Linux Engine 并通过 `/health`。
2. 创建 Code Interpreter Sandbox，执行 Python `2+3` 得到 `5`。
3. 文件写入和读取成功，主动销毁后 Sandbox 容器被清理。
4. CPU、内存、PID、capability drop 和 `no-new-privileges` 限制实际进入 Docker 配置；`allowed_host_paths` 必须配置非空专用白名单，因为 0.2.2 中空数组表示不限制。
5. `networkPolicy.defaultAction=deny` 阻止未授权外网；允许 `example.com` 后访问成功，未授权的 `iana.org` 仍被阻止。
6. Docker Runtime 会从配置的宿主端口池为 execd/egress 分配端口。`40000-60000` 与 Windows/Docker 动态端口重叠时实测出现 `address already in use`，Server 会清理半创建资源并返回 500；本机基线改用启动时无监听且避开系统动态范围的 `20000-29999`。Agentx 对该非幂等 create 不自动重放。

验证时观察到的容器边界包括 `NanoCpus=500000000`、`Memory=536870912`、`PidsLimit=4096`、bridge network 和 runc Runtime。它们证明资源与网络策略链路可用，不证明强隔离。

## 4. Agentx 集成决策

- 保留通用 `SandboxRuntime` Port 和独立 `sandbox-manager` 服务。
- 基础设施实现命名为 `OpenSandboxAdapter`，Rust 侧依据官方 `specs/sandbox-lifecycle.yml` 与 `specs/execd-api.yaml` 调用 REST API。
- 不把 OpenSandbox Python SDK 嵌入 Rust Worker；Worker 只接收 Agentx 标准化的 Sandbox Request/Result。
- `AGENTX_OPENSANDBOX_ENDPOINT` 属于普通配置，`AGENTX_OPENSANDBOX_API_KEY` 只通过 Secret 注入 Sandbox Manager。
- OpenSandbox 0.2.2 的 Docker Runtime 会拒绝 `secureAccess=true`；本地和临时 E2E 显式设置 `AGENTX_OPENSANDBOX_SECURE_ACCESS=false`，生产 Kubernetes ingress 保持默认 `true`。两种模式的 Endpoint 文档都由 Sandbox Manager 加密保存并执行 Host、CIDR 和 Header 白名单校验。
- 默认设置 `AGENTX_OPENSANDBOX_USE_SERVER_PROXY=true`。Docker Runtime 实测会返回类似 `198.18.0.1:40686/proxy/44772` 的无 scheme 直接 Endpoint，该地址无法从 Kubernetes Pod 稳定访问；Server Proxy 返回同源 `/v1/sandboxes/{sandboxId}/proxy/44772/`，避免把 Docker 内部地址暴露给 Worker 或 Manager 网络。
- 网络默认拒绝；Credential 使用 OpenSandbox Credential Vault 或 Agentx 短期凭证代理，不能进入命令行、日志、Trace 或持久镜像。
- 官方 Helm/Controller 是生产部署来源，Agentx 不复制非官方简化 Runtime 清单。

## 5. Rust SDK 缺口和 Adapter 方案

源码快照的 `sdks/` 目录提供 Python、Go、Java/Kotlin、JavaScript/TypeScript、C#/.NET 等 SDK，没有官方 Rust SDK；该快照的 `ROADMAP.md` 也没有承诺 Rust SDK 的交付时间。协议本身可以被 Rust 调用，但不能把 OpenAPI 生成器当作完整 SDK：Lifecycle Spec 缺少大多数 `operationId`，execd 的命令输出、取消和文件流还需要 SSE/背压语义。

已固定的协议基线：

| Spec | OpenAPI / Spec Version | Path | operationId | SHA-256 |
|---|---|---:|---:|---|
| `specs/sandbox-lifecycle.yml` | 3.1 / `0.1.0` | 11 | 1 | `da84de4d80cdad83c47d771135645fbeb8d7477bc8f908cc4b374397010ed6d2` |
| `specs/execd-api.yaml` | 3.1 / `1.0.0` | 40 | 47 | `0f03effe1dc5f340d13e39d6e8c815b5bdebb880183db05bea7d592696d5f5e0` |

因此采用“固定 Spec 生成/校验 DTO + Rust 手写 Adapter”方案：

- Workspace 已有 `reqwest 0.12`、rustls/json 和 Tokio，`platform-api` 也有手写 SSE、Origin、超时和大小限制实现可复用设计；M5-0 先验证现有依赖，不预先绑定新的 eventsource/WebSocket 库。
- 生成物只留在 `agentx-infrastructure`，用于 DTO、枚举和普通 HTTP 请求的可复现校验；不向 Worker 或公共 Crate 暴露供应商类型。
- 手写层负责 SSE 分片解析、事件大小和总量限制、有界背压、取消/interrupt、Sandbox 就绪轮询、Endpoint URL/Host/Port 校验、鉴权 Header 白名单、幂等重试和错误标准化。
- 无 scheme Endpoint 只继承 Lifecycle Endpoint 的 scheme。启用 Server Proxy 时要求 Endpoint 与 Lifecycle 完全同 Origin、路径精确绑定当前 Sandbox 和 execd 端口，并只接受 `x-execd-access-token`、`opensandbox-ingress-to`、`opensandbox-secure-access` 三个协议 Header；API Key 仅在同 Origin 请求上附加，不能进入持久化 Endpoint 文档或跨 Origin Redirect。
- 生产链路不增加 Go Sidecar。Go SDK 只在契约测试中作为差分 Oracle；Sidecar 虽可降低少量初始 HTTP 编码，却会增加 RPC/部署/密钥/流取消/观测和版本升级边界，不能降低 M5 的整体故障面。
- 未支持的必需字段、事件或组件版本统一返回 `SANDBOX_PROTOCOL_UNSUPPORTED`；禁止通过忽略字段、自动跟随重定向或伪造空结果继续执行。

风险等级为“中等、可控”。Rust Adapter 已在真实 OpenSandbox 上完成 create、Server Proxy Endpoint、文件上传、Command 和 terminate 链路，且 Endpoint/Header/SSE 单元测试已通过；`scripts/opensandbox-contract.ps1` 固定源码 Commit/Spec Hash，并使用官方 Go SDK 对同一 Python/Artifact Fixture 执行 create/get/Endpoint/upload/command/download/Metrics/kill Oracle。首期只实现 create/get/kill、Endpoint、Command SSE、interrupt、文件上传/下载、Metrics 和网络策略；Jupyter Context、PTY WebSocket、Pool、Snapshot 等完整高级能力延期。

## 6. 同类方案比较

| 方案 | 优点 | 主要代价/风险 | M5 结论 |
|---|---|---|---|
| Rust 直接 Adapter | 单一 Rust 链路、无需新增运行时、API Key 边界清晰 | 需要维护 SSE、Endpoint 和错误适配 | 采用 |
| Go SDK + Go Sidecar | 可复用官方 Go 语义，初始协议代码较少 | 新服务和内部 RPC、流取消/背压桥接、部署和密钥面；两套语言升级 | 不采用，仅作差分 Oracle |
| Python SDK Sidecar | 官方示例覆盖较广 | 引入 Python 运行时和进程，Worker 到 Sidecar 的失败面更大 | 不采用 |
| 仅 OpenAPI 自动生成 Client | DTO 和普通 CRUD 生成快 | 缺少可靠 SSE/取消/Endpoint 安全语义，Lifecycle Spec 元数据不完整 | 不足以单独使用 |
| 更换另一套沙箱平台 | 可能有现成 Rust/HTTP API | 镜像、网络策略、凭证、E2E 和生产 Runtime 全部重做，当前机器兼容性结论失效 | 暂不切换 |

没有发现同时具备“自托管 Docker/Kubernetes Runtime、当前验证过的网络/资源策略、Agentx 所需文件/命令 API、官方 Rust SDK”的开源替代；OpenSandbox 当前缺口通过 Adapter 边界和契约门禁可控，切换平台的迁移成本高于补齐这层协议适配。

## 7. M5 门禁影响

当前 Kubernetes 基线已完成 Agent+MCP、循环停止以及真实 OpenSandbox Python、JavaScript、Shell、Browser Command、文件 Artifact、部分 stdout/stderr Artifact/Trace、Credential、网络 deny/allow、内存限制、自然 TTL、取消、租户并发配额和 Manager 重启/Reaper E2E。Browser 固定 `opensandbox/playwright@sha256:09709684c785db3107fc3357e7af5b921f5d5a60e75071601122a473d344b475`，只使用 `/home/playwright` 工作根目录；其他 Runner 使用 `/workspace`。CPU/PID/磁盘强制效果与跨租户攻击面不属于当前 Kubernetes+runc 验收范围，列为后续生产强化。

后续生产发布前另设强隔离门禁：实际 Sandbox Pod 应使用批准的 RuntimeClass，并验证节点隔离、跨租户攻击面、IPv4/IPv6 egress、Credential Vault、镜像供应链和故障回收。该门禁当前暂不重试，由 M7 INT-006/010/011/014 统一验收；它不阻塞当前 Kubernetes+runc 基线，也不改变 M5 功能完成状态。
