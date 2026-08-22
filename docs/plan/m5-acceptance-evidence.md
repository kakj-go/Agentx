# M5 验收证据

> 历史命令说明：本文中的 PowerShell、Shell、deployment Profile 和核心 Kustomize 命令只记录当时验收事实，相关入口已经删除，不得作为当前操作入口；请使用 `agentx-deploy`、`agentx-check` 与 `pytest tests/e2e`。

状态：当前 Kubernetes 部署验收 `done`；AGT-010 的生产强化子项暂不重试并转入 M7。当前复核日期：2026-08-05。

M5 已交付 Runtime Port 与快照、资源 Adapter、Agent Ledger/预算/循环、Rust `OpenSandboxAdapter`、`sandbox-manager`、Agent/Code API 和 Execution Runtime Workbench。固定 Commit/Spec Hash 的官方 Go SDK Oracle，Agent+MCP、重复 Tool 停止，Skill 递归授权，固定版本 LightRAG/Mem0，Python/JavaScript/Shell/Browser Code，部分输出 Artifact/Trace、Credential 临时文件、网络 deny/allow、内存限制、自然 TTL、租户 Sandbox 并发配额、取消、Manager 重启/Reaper 和 ClickHouse 中断补投均已通过，AGT-001～013 标记为 `done`。当前 Docker Desktop Kubernetes+runc 部署基线验收已完成；生产 RuntimeClass、Vault、镜像签名和生产级跨租户攻击隔离列为后续生产强化。

## 1. 已实现能力

- `agentx-application` 提供统一 `RuntimeContext`、稳定 `RuntimeError` 和 Model、MCP Tool、Skill、RAG、Memory、Sandbox Port；供应商 DTO 留在 Infrastructure。
- Migration `0014_m5_agent_runtime.sql` 增加 capability 字符串约束、Sandbox Profile/Version、Agent Run/Iteration、Runtime Call、Sandbox Lease、Execution Token 汇总和 Invocation Handle 绑定；ClickHouse `0002_m5_agent_trace.sql` 追加 M5 Trace 维度。
- Execution 创建时固化版本化资源、Workflow Identity/Grant 证据、Credential Version、价格、Skill Artifact/Hash/递归依赖和 Sandbox Profile；Worker 只消费 Snapshot。
- `OpenAiCompatibleRuntime`、MCP Streamable HTTP/Legacy SSE、LightRAG、Mem0 和 Skill Loader 已接入 Worker。Skill 递归依赖逐项重新授权，并拒绝绝对路径、反斜杠、重复/空段和 `..`。
- Agent Runner 持久化 Run/Iteration/Call Ledger 与 State Artifact，调用前预留预算、结果后结算，并实现重复指纹、重复错误、A-B-A-B、State Stall 和稳定幂等键。
- `sandbox-manager` 暴露带内部 Token 拦截器的独立 gRPC，持久化 Lease/加密 Endpoint，执行 Reaper；Worker 只依赖 `SandboxRuntime`。
- Rust Adapter 实现 create/get/list/kill、Endpoint、Command SSE/interrupt、上传/下载、Metrics 和网络策略。默认启用 Server Proxy；无 scheme Endpoint 只继承 Lifecycle scheme，Proxy 必须同 Origin 且路径绑定当前 Sandbox，API Key 不进入 Endpoint 文档或跨 Origin 请求。
- `scripts/opensandbox-contract.ps1` 校验固定 Commit 和两份 Spec Hash，使用官方 Go SDK 对与 Rust E2E 相同的 Python 源码、stdout 和 Artifact 内容执行 create/get/Endpoint/upload/command/download/Metrics/kill，并在成功或失败路径回收 Sandbox；Go 不进入生产构建或镜像。
- Code Runner 使用带 tag 的 Sandbox Profile 和 argv Command，先上传脚本再执行；输出和下载文件转换为 Item/Artifact，终止时撤销 Sandbox 绑定 Handle。
- Browser Runner 固定 `opensandbox/playwright@sha256:09709684c785db3107fc3357e7af5b921f5d5a60e75071601122a473d344b475`，使用镜像的 Python Playwright/Chromium 和 `/home/playwright` 工作根目录；其他 Runner 保持 `/workspace`，Adapter、Manager 和下载校验只允许这两个根目录。
- Worker 取消时并行监听 Runtime Cancellation 与 Command Stream，先请求 interrupt，再在收尾路径 terminate。Manager 的清理鉴权仍校验 Tenant/Execution/Node/Attempt/Worker Lease/Sandbox Lease Token 的完整绑定，但允许绑定执行已终态或 Lease 已过期，并对重复 terminate 幂等返回。
- OpenSandbox 幂等 terminate 对可重试错误执行最多 3 次有限重试；Reaper 为前台 TTL 清理保留 5 秒窗口，只接管卡住 30 秒的 `terminating` Lease，且失败状态不能覆盖已完成的 `terminated` 终态。
- Sandbox Manager 在创建 Lease 前按 Tenant 行加锁并原子统计活跃 Lease；`AGENTX_SANDBOX_MAX_ACTIVE_PER_TENANT` 达限时返回 `SANDBOX_TENANT_CONCURRENCY_EXCEEDED`，不写 `sandbox_leases`，也不向 OpenSandbox 发起 create。
- Platform API 提供 `/executions/{id}/runtime-details` 和 Sandbox Profile 管理；Runtime Status 展示兼容状态与活跃 Sandbox；生成前端 Client 已同步。
- Execution Workbench 展示 Agent Run、Iteration、Runtime Call、Token/成本、指纹、停止原因、State Artifact 和 Sandbox Lease；Manifest/Schema 已注册 M5 capability 和 `sandbox_profile`。

## 2. 快速门禁

执行：

```powershell
.\scripts\check.ps1
git diff --check
```

当前结果：通过。

- Rust fmt、Clippy `-D warnings`、Workspace Tests 和 Doc Tests 通过。
- Web Oxlint、Vitest、TypeScript 和 Vite 生产构建通过。
- OpenAPI、Node Manifest、Workflow Definition Schema、生成 TypeScript Client 和 Kustomize 渲染无漂移。
- Skill 最终授权补丁另以 `cargo test -p agentx-infrastructure skill_runtime::tests` 和 `cargo clippy -p agentx-infrastructure --all-targets -- -D warnings` 复核通过。

关键单元/契约覆盖包括 Snapshot/Capability、Model/MCP/RAG/Memory 请求映射、Skill 路径和依赖、预算/指纹/循环、SSE 分片/多行/上限、Endpoint/Origin/Header、下载路径与跨分片 Secret 脱敏。

## 3. Kubernetes E2E

最终功能复跑：

```powershell
.\scripts\e2e.ps1 -SkipBuild
```

结果：2026-08-05 当次最终复跑通过并返回 0。套件先回归 M2.1、M3、M4 及 Worker/Coordinator/Redis/ClickHouse 故障恢复，再执行 7 个 M5 Playwright 场景和 M5 Sandbox 故障套件；当次 M5 JUnit 为 7 tests、0 failures、0 skipped，总耗时 76.298 秒。

| 场景 | 自动化断言 | 结果 |
|---|---|---|
| Agent + MCP | 两轮模型、一次授权 MCP Tool、3 条有序 Runtime Call、64 位请求指纹和持久化 Ledger | 通过 |
| 循环停止 | 重复 Tool 调用在第二次后以 `repeated_tool_call` 停止，不再发起下一次外部请求 | 通过 |
| RAG + Memory | LightRAG 1.5.5 和 Mem0 1.0.0 由 Worker Snapshot/Grant/Runtime Port 调用，查询结果与 Trace 关联 | 通过 |
| RAG Scope | 只读 Grant 的写操作在请求 Addon 前返回 `RAG_WRITE_DENIED`，Addon 数据未发生变化 | 通过 |
| Python Code | 真实 OpenSandbox create、文件上传、Command、stdout、下载 Artifact、Lease 终止和 Handle 撤销 | 通过 |
| Runner/Credential/Network | JavaScript、Shell 与 Browser Command；Browser 加载本地页面并下载 Chromium 截图；Credential 临时文件注入且 stdout 不泄露 Secret；默认 deny 和 `example.com` allowlist | 通过 |
| 部分输出 | stdout/stderr 超限后 Item 与 Trace 标记 `partial`，可见内容受限，完整内容写 Artifact 并由 `contentRef`/`artifactRefs` 关联 | 通过 |
| 内存限制 | 64 MiB Profile 中的 256 MiB 分配失败；OpenSandbox 若发送非零命令终态则为 `SANDBOX_COMMAND_FAILED`，若 OOM 在终态事件前中断 SSE 则保留 partial 语义并返回 `SANDBOX_STREAM_INCOMPLETE`；两种路径的活跃 Lease 均为 0 | 通过 |
| 自然 TTL | 长命令达到 60 秒 Profile TTL 后返回 `SANDBOX_TTL_EXPIRED`，幂等清理与 Reaper 不覆盖主错误，活跃 Lease 为 0 | 通过 |
| 取消 | 长命令运行中取消，Worker interrupt 后 terminate，Execution 为 `cancelled` 且活跃 Sandbox Lease 为 0 | 通过 |
| 租户并发配额 | 配额为 1 时第二个并发 Code Execution 返回 `SANDBOX_TENANT_CONCURRENCY_EXCEEDED`，拒绝路径不写 Lease、不调用 OpenSandbox | 通过 |
| Manager 重启/Reaper | 长命令运行中强杀 Manager，重启后强制 Lease 过期并由 Reaper 回收，Execution 进入失败类终态 | 通过 |
| ClickHouse 中断补投 | ClickHouse 停止期间 Agent Execution 和 3 条 Runtime Call Ledger 提交成功、模型结算完成且无 `reserved/sent` 残留；恢复后 Trace Outbox 清零 | 通过 |

OpenSandbox 实际返回为：上传 200、Command 200、Sandbox DELETE 204。应用已在正常路径主动销毁 Sandbox，因此最终清理证据为 `created=0`、`remaining=0`；OpenSandbox Server 日志保留了本次 create/upload/command/delete 链路。

证据入口：

- [M5 Playwright](../../apps/e2e/tests/m5-agent-sandbox.spec.ts)
- `apps/e2e/test-results/junit.xml`
- `apps/e2e/playwright-report/index.html`
- `apps/e2e/test-results/artifacts/`
- `apps/e2e/test-results/kubernetes/m5-sandbox-cleanup.txt`
- `apps/e2e/test-results/kubernetes/m5-database-evidence.txt`
- `apps/e2e/test-results/kubernetes/m5-opensandbox-go-oracle.json`
- `apps/e2e/test-results/kubernetes/sandbox-manager.log`
- `apps/e2e/test-results/kubernetes/workflow-worker.log`
- `.local/opensandbox-restart.out.log`

默认清理已确认 `agentx-e2e` Namespace 不存在、OpenSandbox 列表为空，开发 `agentx` Namespace 原副本数已恢复。

当前 Kubernetes 部署基线（2026-08-05 复核）：

- Docker Desktop Kubernetes 节点 `desktop-control-plane` 为 `Ready`，Kubernetes `v1.36.1`，containerd `2.3.1`。
- 旧的未纳管 `agentx` Namespace 和 PVC 已清理，并从空状态通过 `deploy.ps1 -Action Install -Profile Full -Namespace agentx -NonInteractive` 重建；`agentx-deployment-state` 保存了规范化 Profile、Hash 和 `lastTarget=all`。
- `agentx` Namespace 中 Platform API、Gateway、Coordinator、Worker、Trace Writer、Web、MySQL、Redis、ClickHouse、MinIO、LightRAG、Mem0 和本地 Addon 使用的 Echo MCP 运行型 Pod 均为 `1/1 Ready`；Migration/初始化 Job 为 `Completed`。
- Full 默认 Sandbox disabled，因此当前基线不部署 Sandbox Manager，也不注入 Manager URL/Token；OpenSandbox 仍是独立安装、按需 remote 接入的 Provider。
- `agentx-e2e` 与 `agentx-e2e-deps` Namespace 不存在，未发现残留 `kubectl port-forward` 进程；`agentx-ingress` 因 `agentx.localhost` 仍在使用而保留。

以上是当前 M5 的 Kubernetes 部署验收边界；不包含生产 RuntimeClass、Vault、镜像签名或跨租户攻击隔离认证。

### 可组合部署与外部依赖

最终执行：

```powershell
.\scripts\deploy-tests.ps1
.\scripts\deploy-distributed-e2e.ps1
.\scripts\tls-integration.ps1
.\scripts\check.ps1
git diff --check
```

- Profile 单元与渲染测试覆盖 576 种模式组合，缺失字段、Secret 脱敏、Profile Hash、非法状态型依赖切换和卸载所有权均通过。
- Full Profile 由统一脚本创建 bundled MySQL、Redis、ClickHouse、MinIO、LightRAG、Mem0、核心服务和专用 ingress-nginx；默认 Sandbox disabled，只有 Custom remote Profile 才部署 Sandbox Manager。Doctor 在 Migration 前完成真实连接验证。
- `sandbox-manager doctor-opensandbox` 已对本机真实 Provider 返回 `status=ready`、`sandboxCount=0`，并输出固定 Commit、Lifecycle/execd Spec Hash、Server `0.2.2` 与 execd `v1.0.21` 兼容信息。
- 分散部署 E2E 将四项依赖放在 `agentx-e2e-deps`，Agentx 放在 `agentx-e2e` 并使用跨 Namespace FQDN；Install、Upgrade 和卸载后返回 `externalResourcesPreserved=true`、`upgradeDataPreserved=true`。
- 同一分散部署 E2E 完成真实 Bootstrap 和 M5 Code Fixture，证明 Sandbox disabled 时 Execution 为 `failed`、错误码为 `RUNTIME_UNAVAILABLE`、`sandboxLeases=0` 且 `managerDeployed=false`；失败路径由 Worker 明确提交，不会让队列永久等待。
- TLS 常规测试 2/2、Docker 专项测试 1/1 通过，覆盖系统 CA、私有 CA、错误 CA、MySQL/Redis mTLS、认证失败及 S3 Session Token。
- 本地 LoadBalancer finalizer 慢清理路径已自动处理；最终 `agentx-e2e` 与 `agentx-e2e-deps` 均不存在，外部依赖未被 Agentx Uninstall 修改。`agentx-ingress` 仍服务当前受管 `agentx`，因此卸载扫描发现使用者后按设计保留。

## 4. 功能缺口关闭情况

此前记录的 M5 功能缺口已经关闭：

1. Model 契约测试覆盖流中断 partial、Tool Call delta、价格计算、缺价格和非 USD 拒绝。
2. MCP 覆盖 Streamable HTTP/Legacy SSE、Schema、授权、side-effect unknown-outcome；Skill 覆盖撤权、跨租户和递归依赖。
3. LightRAG/Mem0 固定版本 Addon 已进入真实 Worker E2E，并验证 RAG read/write Scope 在零外部写入条件下失败。
4. Agent Ledger 数据库测试覆盖跨 Attempt 恢复、并发预算预留/结算、幂等重用和 `limitAction`；创建结果未知由标签对账测试证明只恢复一个 Sandbox。
5. Credential Handle 覆盖 expiry/replay、临时文件、正常终止撤销和跨分片脱敏；部分输出、大 State/响应 Artifact、Trace 引用和查询脱敏均有自动化证据。
6. M5 专属 ClickHouse 中断场景证明 Execution/MySQL Ledger 不回滚，恢复后 Trace Outbox 补投完成。
7. Runtime Panel 覆盖 loading/error/empty/full-data 组件测试，Execution 权限测试沿用统一 Guard；真实页面已检查 1280x720 与 390x844、浅色/深色、成功 Agent、RAG 失败和 Sandbox 明细，无页面级横向溢出。节点画布配置按既定边界属于 M6。

## 5. 后续生产强隔离强化

当前机器仅有 Docker+runc；它已证明 Rust Adapter、OpenSandbox Docker Runtime、资源/网络配置链路和当前 Kubernetes 功能 E2E 可运行，但不代表生产多租户强隔离。AGT-010 的生产强化子项标记为“暂不重试（deferred）”，当前环境不再复跑。生产发布前由 M7 INT-006/010/011/014 在具备 gVisor、Kata 或经安全评审等价 RuntimeClass 的集群补充真实 Sandbox Pod、CPU/PID/磁盘强制效果、IPv4/IPv6 egress、Credential Vault、镜像签名与 digest 供应链、节点隔离、跨租户攻击面和故障回收证据。该强化不阻塞当前 M5，也不重新打开已经完成的 AGT-001～013。

## 6. 复核后证据债务

- 当前本地 `apps/e2e/test-results/junit.xml` 已被一次无完整环境的运行覆盖，内容为全量 skipped，不能替代上文记录的当次 M5 验收结果。M6 开始前应先按阶段和运行 ID 隔离报告路径，避免再次覆盖；当前不因此重跑 AGT-010。
- Windows checkout 下 `scripts/check.ps1` 会因 CRLF/LF 原始字符串差异误报 Platform OpenAPI 漂移；规范化换行后契约一致。M6 开始前应改为结构化 JSON 比较或统一换行后比较。
- 上述两项只影响当前证据可复核性，不否定已经保存的 M5 Runtime、数据库、Go Oracle、Addon 和 Sandbox 故障证据。报告隔离和 CRLF 修正不能留到 M7；完整生产候选 E2E 与 `failures=0`、`skipped=0` 的持久化证据由 M7 INT-014 生成。
