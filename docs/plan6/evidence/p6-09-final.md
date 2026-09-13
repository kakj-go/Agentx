# P6-09 最终完成与本地交付证据

状态：已撤销完成结论。本文保留 2026-09-06 上一轮运行结果；严格源码与场景复核发现证据扩大解释，当前完成度与最终证据见 [`p6-10-final-remediation.md`](p6-10-final-remediation.md)。

## 1. 源码与协议结论

- 源码基线为 `master@e774d272e3fff2b3432e4ad856654d0f1ee68691`；plan6 位于当前工作树，尚未创建提交。实施前存在的其他修改没有被回退或删除。
- Workflow Definition 为 8.0，Node Manifest 为 3.0；Canvas Plugin Package、SDK API、Runner RPC、Worker Protocol、Runtime Internal API 和 Trace Envelope 首版均为 1。未知版本直接拒绝，不保留 migration、shim、别名、双读或旧执行路径。
- Node.js 固定为 24.20.0，pnpm 固定为 11.9.0。Worker 以 `tini` 为 PID 1，通过 Tokio 管理有界 Node 进程池；一个进程同一时间只处理一个调用。
- Workflow 按节点冻结包 ID、SemVer、ZIP 摘要、运行源、有效端口和输出 Schema。同一 Workflow 不允许混用同一包的不同版本。Runtime/Worker 使用冻结 Bundle 或 Work Package，不回查 Control 可变配置。
- Control 的插件管理、动态契约解析、发布门禁、引用锁和对象 GC，Runtime 的设计时调用、Node 执行、宿主桥接、Lease/Fencing/恢复，以及 Observability 的动态 Trace 内容已贯通。

## 2. 产品闭环

以下连续路径已在临时 Kubernetes 和正式本地安装各运行一次：

`下载模板 → 仓库外开发变体 → check/test/build/pack:plugin → 页面真实上传 → 画布拖拽 → React Panel 配置 → 动态 Provider/端口 → Undo/Redo → 保存重开 → Debug/发布 → Node.js 执行 → 下游引用 → 标准与插件 Trace → 停用/历史读取/Fork 门禁 → v2 默认与显式切换 → 下载和引用保护删除`

关键实现结果：

- “资源 → 画布插件”提供列表、来源/状态筛选、详情、版本、使用情况、开发说明、审计、导入预览、启停、默认版本、下载、引用检查和删除。
- 导入支持正确包、失败记录、查询、取消、重试、TTL、同摘要幂等、同版本异摘要冲突和后台对象补偿。
- 动态 `resolveDefinition` 使用纯 JSON、Hash 缓存和 complete/incomplete/invalid 状态；草稿允许 incomplete，invalid 不推进 Revision，发布与执行要求 complete。
- Provider 支持参数、搜索、分页、AbortSignal、晚到响应抑制和 Runtime 设计时宿主边界。插件 Panel 按包摘要稳定挂载，参数变化不会清空插件局部状态。
- 所有内置节点 UI 通过统一包注册入口。Set/List 使用 `agentx/data` TypeScript Runtime，HTTP 使用 `agentx/http` 和 `ctx.http`；Loop、Approval、Sub-workflow、Model、Agent、Code 与边界语义继续由 Rust 内核执行。
- Runner 清理插件计时器、监听器和模块状态，限制 8 MiB I/O、64 Span、16 层深度和每 Span 内容/事件/属性数量；插件明确的 `retryable/details` 保留到 Worker 结果。
- Linux 进程组、Windows Job Object、容器 init、取消、超时、Runner 崩溃、后台任务、孤儿进程、Worker 重启、迟到结果和冷恢复均有自动化证据。
- 平台始终生成 Node/Attempt Trace；插件子 Span、同类型多内容、嵌套 Promise、宿主调用父子关系、计量来源和大内容 Artifact 外置已接入。历史 renderer 按执行授权和冻结摘要加载。

## 3. 40 项场景映射

[`06-e2e-acceptance.md`](../06-e2e-acceptance.md) 的 E2E-01 至 E2E-40 在上一轮曾标记通过；下表仅记录当时的证据映射，不能替代当前严格验收：

| 场景 | 直接证据 |
|---|---|
| E2E-01–19 | `canvas-plugins.spec.ts` 的管理、上传、动态契约、Provider、编辑、发布、权限、停用、历史、引用和版本生命周期；Control/Compiler 契约测试补充错误包与锁竞态。 |
| E2E-20 | `node-panel-registry`、内置 TypeScript 包测试以及完整产品套件对 If/Merge/Loop/Approval/Sub-workflow/Model/Agent/Code/Start/Exit 的回归。 |
| E2E-21–29 | Runner 9 项测试、Rust Worker 插件测试、`tests/e2e/runtime/test_plugins.py` 和完整 Runtime/OpenSandbox/Agent 回归。 |
| E2E-30–35 | 插件实时 Trace、多个内容、嵌套 Span、大 Artifact、历史 renderer、聚合乱序测试，以及 pytest 编排的 ClickHouse 下线与恢复。 |
| E2E-36–37 | `test_canvas_plugin_template.py` 将下载 ZIP 解压到仓库外，修改运行/UI/协议测试，执行五条开发命令并通过真实页面上传同一产物。 |
| E2E-38–39 | 中英/浅深主题与可访问性截图；100/300 插件节点和 500/1000 既有性能门禁；Runner 热循环与 K8s 进程/内存预算。 |
| E2E-40 | 完整 `pytest tests/e2e`、Helm 安装/升级/回滚、安全与清理回归，最终恢复正式开发环境。 |

## 4. 自动化结果

| 命令或层 | 结果 |
|---|---|
| `cargo xtask check` | 通过；包含 fmt、clippy `-D warnings`、串行 workspace 测试、边界检查、Helm/Values、Python、Web lint/test/build 和 `git diff --check`。 |
| Web Vitest | 86 个文件、394 项通过；生产构建通过。 |
| Monorepo TypeScript | SDK、UI SDK、Runner、模板、内置 data/http 的 check 通过。 |
| Plugin Runner | 9/9 通过，包含双向 RPC、Outcome Unknown、取消、上下文清理、Promise 父子关系、预算和 50 次热调用。 |
| Python acceptance | 19/19 通过；包含 SDK/模板声明同源漂移检查。 |
| 插件聚焦 Kubernetes E2E | Run `2dbcbb6379`，7/7 通过，403.36 秒。 |
| 完整 Kubernetes E2E | Run `21cefd9702`，26/26 通过，1263.87 秒。 |
| 正式安装后 Playwright | Run `formal/plan6-formal`，1/1 主插件闭环通过，21.9 秒。 |

Rust 测试中的 2 个真实钉钉/飞书租户探测继续按仓库既有约定 ignored，因为本机没有对应外部凭据；它们不属于 plan6 插件协议和本地交付范围。

## 5. 性能与资源

完整 Run 的产物位于 `apps/e2e/test-results/helm-agentxctl/21cefd9702/workflow-performance/`：

| 节点数 | 节点类型 | 首次可交互 | 中位 FPS | p95 输入延迟 | 结果 |
|---:|---|---:|---:|---:|---|
| 100 | 真实安装插件 | 493 ms | 59.9 | 30.7 ms | 通过 |
| 300 | 真实安装插件 | 744 ms | 59.9 | 31.0 ms | 通过 |
| 500 | 既有大图门禁 | 722 ms | 59.9 | 44.3 ms | 通过（FPS ≥ 50，p95 ≤ 50 ms） |
| 1000 | 既有大图门禁 | 831 ms | 59.9 | 39.6 ms | 通过（FPS ≥ 30，p95 ≤ 100 ms） |

Kubernetes Worker 实测 `pluginMaxProcesses=8`、`pluginMemoryMb=96`；进程数不超过 8，单进程 RSS 门禁为 192 MiB。Runner 后台任务、超时和进程树测试结束后无残留 Node 子进程。

## 6. 制品摘要

插件包：

- `acme/json-mapper@1.0.0`：`sha256:b9e95ff88c574d668652bb4e10b429b4e112af2971e9b257b82d854dec58c7d1`
- `acme/json-mapper@2.0.0`：`sha256:f1fa916780252a2e5e283c6a67a7c7556e453c56623ce4f1d78dd247a5caa71c`
- `acme/race@1.0.0`：`sha256:5f647d5d9ee36984e5acf1d24a5b4547ec59cb71b13e3b7cab65a16bb785851c`

最终本地 OCI 镜像 ID：

| 镜像 | ID |
|---|---|
| agentx-migrate | `sha256:3de45fbe26a5f7def3eaf3d8f566be3f0fcbad7e38e99edcaf3109d436aceae4` |
| agentx-bootstrap | `sha256:3ac1f4556d72fc8224c63c94e7ba3dff340c828e7e1496ed1379baefd19d6da6` |
| agentx-doctor | `sha256:aff83f91d8044efd3a0c0f879b86a4c044cd3770ed3ddade5acfd6e4eebbb785` |
| platform-control | `sha256:20e5a9d566e239895a574a0c5c5030f73a373976da38cba947d395d6c80fa0b0` |
| web-console | `sha256:2a91d98d26d80f825da1b6fb41930a69e8be7b82736f1d1c7e37b911ce0f1b03` |
| runtime-gateway | `sha256:8a813cdec550a0e6e2e5b42520f202356f7ec5521ca8d14b2374133911328c3d` |
| workflow-runtime | `sha256:60261bb9383a80989209fb1b56cea1d92b68c09e07785256ce251adca4f90c6d` |
| workflow-worker | `sha256:4d2fa147d16e0dc1cb6651d165be5def1b9566b755012d608baebff3d4f2c9b0` |
| sandbox-manager | `sha256:b0d2152fdfbfa868374131cb40af60d126740b228365a2d36157a3e6f9013fd5` |
| agentx-egress-gateway | `sha256:92a0ef6da659fd96251616094871894153f757b1f154a5864c8b0478c0e7fe0a` |
| observability | `sha256:57f60e216a68fcded395472761b91221a39d5de919224d39374bd377af2becef` |

## 7. 正式本地安装与清理

- 已按授权对 `agentx-control`、`agentx-runtime`、`agentx-deps` 执行 `agentxctl uninstall --purge-data`，随后全新 `agentxctl install`；安装回执为 `status: ready`。
- Control 的 2 个 Deployment、Runtime 的 5 个 Deployment、3 个数据库/队列 StatefulSet，以及 Dependencies 的 egress、Vault、Object Storage 均为 `1/1 Ready`。
- Worker 容器内 PID 1 为 `tini`，Node 为 `v24.20.0`，Runner 文件存在，`AGENTX_CONTROL_*` 环境变量数量为 0，内存预算为 96 MiB。
- 正式环境通过端口转发访问 Web 与 Runtime Gateway，并完成真实上传、画布、发布、Node 执行和 Trace。执行证据为 `artifacts/plan6/formal-plugin-execution.json`，Execution ID `01a074eb-db97-7b22-b853-a1415416241d`。
- 本地 ingress-nginx 在 local 环境按 `agentxctl` 设计使用 ClusterIP；浏览器验收使用临时 `kubectl port-forward`，验收后已停止端口转发进程。
- Run `2dbcbb6379` 与 `21cefd9702` 的三个临时 Namespace 均已清理；不存在 `agentx-e2e-*` Namespace或 OpenSandbox 测试容器。`argus-*`、`p4-*` 和共享基础设施未修改。

该轮运行记录不能证明 plan6 已全部完成。严格复核整改完成后将生成新的最终证据；仓库工作树尚未提交。
