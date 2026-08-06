# M5 Agent 运行实施任务清单

状态：当前 Kubernetes 部署验收 `done`。AGT-001～013、固定协议门禁、资源 Runtime、Agent Ledger/预算/循环、Rust OpenSandbox Adapter、Sandbox Manager、Agent/Code API 与 Workbench、固定版本 LightRAG/Mem0 和临时 Kubernetes Runtime E2E 均已完成。AGT-010 的 Docker+runc 功能基线保持 `done`；gVisor/Kata、CPU/PID/磁盘实际强制、双栈 egress、生产 Vault、镜像签名和生产级跨租户攻击隔离标记为“暂不重试（deferred）”，不在当前环境继续复跑，统一转入 M7 的 INT-006/010/011/014。OpenSandbox 选择及本机实测结论见 [可行性评估](opensandbox-feasibility.md)。

## 1. 目标和完成边界

M5 完成后，平台必须能够：

1. 由 Workflow Service Identity 在运行时二次校验并调用已授权 Model、MCP Tool、Skill、RAG 和 Memory。
2. 运行带多轮模型调用和 Tool Call 的 Agent 节点，记录有序 Iteration、Token、成本、错误和停止原因。
3. 在达到迭代、调用、Token、成本、时间、重复错误或 A-B-A-B 循环阈值时立即停止后续外部调用，并写入可解释结果。
4. 通过 `sandbox-manager` 的稳定 API 创建、执行、上传、收集和销毁 OpenSandbox；Worker 不直接调用供应商 SDK。
5. 让 Python、JavaScript、Shell、文件处理、浏览器和动态代码只在 OpenSandbox 中运行，标准化为 Item/Artifact。
6. 强制 CPU、内存、PID、磁盘、网络、TTL、Credential 和租户并发边界，超限或服务故障不改变 Execution 权威状态。
7. 在 Trace 页面还原 Model、Tool、RAG、Memory、Agent Iteration 和 Sandbox 过程，并通过临时 Kubernetes Namespace E2E 证明完整链路。

M5 不实现插件市场、任意未授权外网访问、Worker 进程内动态代码执行、生产强隔离基础设施的安装自动化或所有模型 Provider 的私有协议兼容。`openai_compatible` 是首个可验收 Model Provider；`custom_http` 在版本化请求/响应映射契约冻结前必须保持不可运行且明确返回 `RUNTIME_UNAVAILABLE`。

OpenSandbox 首期只承诺 create/get/kill、Endpoint、Command SSE、interrupt、文件上传/下载、Metrics 和网络策略。Python、JavaScript、Shell 及 Browser 首期都通过 Sandbox Profile 指定 tag 镜像中的 execd Command API；完整 Jupyter Context、PTY WebSocket、Pool、Snapshot 和交互式浏览器会话延期。

## 2. 进入检查（已完成）

以下前置检查已经完成；它们属于 AGT-001、AGT-008 和 AGT-010 的交付边界，不新增任务编号：

- `execution_snapshots.resource_snapshot_json` 已固化 Model、Tool、Skill、RAG、Memory、Credential、Sandbox Profile 的版本和授权快照，Worker 只读 Snapshot。
- capability 已扩展为代码目录，旧 Worker 不订阅 M5 capability。
- Migration `0014_m5_agent_runtime.sql` 已将四处 capability ENUM 改为受格式约束的 `VARCHAR(64)`。
- `sandbox-manager` 已实现配置、Readiness、OpenSandbox Client、内部 gRPC、Lease 和 Reaper。
- `agentx-infrastructure` 的 Rust `OpenSandboxAdapter` 已实现 `SandboxRuntime` Port；生产链路不引入 Go/Python Sidecar、供应商 SDK 或 CLI 子进程。
- 协议基线固定为 OpenSandbox Commit `e95681e791b33b3893033940cbeaa5ab192bf21b`、Lifecycle Spec `0.1.0`、execd Spec `1.0.0` 以及 [可行性评估](opensandbox-feasibility.md#5-rust-sdk-缺口和-adapter-方案) 中的 SHA-256；组件和镜像版本必须一起进入兼容矩阵。
- Agent Runner、Budget/Tool Loop 和资源 Runtime 已拆分模块，后端文件保持小于 2000 行。
- Skill 的递归依赖已在发布和运行时逐项授权，并覆盖 RAG/Memory 依赖。

## 3. 依赖图和并行边界

```text
M5-0 Rust OpenSandbox 协议 Spike（AGT-008 前置门禁）
  -> M5-1 Runtime Contract、Resource Snapshot、Capability Migration
       ├─ 资源分支
       │    M5-2A Model + MCP/RAG/Memory（AGT-001/002/004）
       │      -> M5-3A Skill（AGT-003）
       │        -> M5-4A Agent Loop（AGT-005）
       │          -> M5-5A Budget + Loop Detection（AGT-006/007）
       └─ Sandbox 分支
            M5-2B Adapter + Manager（AGT-008）
              -> M5-3B Command Runner（AGT-009）
                -> M5-4B Quota/Network/TTL（AGT-010）
                  -> M5-5B Credential（AGT-011）

M5-5A + M5-5B
  -> M5-6 Trace（AGT-012）
    -> M5-7 Node/API/UI/Kubernetes E2E（AGT-013）
```

M5-0 不新增原子任务编号，属于 AGT-008 的前置门禁。它通过后，M5-1 才冻结跨分支 Runtime Context；资源 Runtime 和 OpenSandbox 两条分支随后可以并行，但必须在 AGT-012 前汇合。任何阶段不得通过 Fake Provider、伪 Sandbox 或人工写入 Trace 标记完成。

## 4. 实施批次

| 批次 | 任务 | 主要交付物 | 退出门禁 |
|---|---|---|---|
| M5-0 Rust 协议 Spike | AGT-008 前置部分 | 固定 Spec/组件矩阵、DTO 生成校验、Lifecycle 核心调用、Command SSE、Endpoint 安全层、差分 Fixture | 核心子集与官方 Go SDK/CLI 一致；Spec 漂移使 CI 失败；不兼容返回 `SANDBOX_PROTOCOL_UNSUPPORTED`；未通过不得开发 Runner |
| M5-1 Runtime 契约和迁移 | AGT-001、002、004、008、010 的前置部分 | Resource Snapshot Schema、Runtime Context、capability 扩展、Sandbox Profile、内部 Manager API、Readiness/降级状态 | 空库与现有 M4 数据可迁移；Execution 固化资源版本/授权快照；旧 Worker 不领取新 capability |
| M5-2A 资源 Runtime | AGT-001、002、004 | `openai_compatible`、MCP 会话/Schema、LightRAG/Mem0 Adapter、价格和错误映射 | Provider 流中断保留部分输出；Token/价格可追溯；未授权请求前置失败；Fake 契约和真实 echo/Addon 通过 |
| M5-2B Sandbox 基础 | AGT-008 | Rust `OpenSandboxAdapter`、sandbox-manager、生命周期/命令/文件/Metrics、Lease/Reaper | Docker+runc 健康、创建、命令、文件、终止、取消和 Manager 重启测试通过；API Key 只在 Manager |
| M5-3A～5A Skill/Agent | AGT-003、005～007 | 递归依赖、Agent State/Iteration/Tool Loop、预算、指纹和循环检测 | 未授权依赖不能加载；多轮/并行 Tool 顺序稳定；预算命中不再外调；停止原因可查询 |
| M5-3B～5B Runner/安全 | AGT-009～011 | Command 模式 Python/JavaScript/Shell/Browser、资源/网络/TTL、短期 Credential | 代码不在 Worker 执行；超限、默认拒绝网络、白名单、TTL、取消和 Secret 脱敏通过 |
| M5-6 Trace | AGT-012 | Trace Schema、写入/降级/查询和成本聚合 | 一次执行可还原资源调用、Agent Iteration、Sandbox 流和部分输出；ClickHouse 故障不改变权威状态 |
| M5-7 节点和发布 | AGT-013 | Agent/Code Node、API、前端 Trace、Playwright、Kubernetes E2E 和验收证据 | 临时 Namespace E2E 通过并清理 Sandbox/Namespace；M5 13 项和追踪矩阵全部同步 |

M5-0 的固定退出检查：

1. 固定 Commit、Spec SHA-256、组件版本和镜像 digest，生成物可复现。
2. create/get/kill、Endpoint、Command SSE/interrupt、文件上传/下载、Metrics、网络策略在真实 OpenSandbox 跑通。
3. Endpoint scheme/Host/Port/CIDR 与返回 Header 严格校验；API Key 和 execd Token 不跨 Origin、不进入重定向或日志。
4. SSE 支持分片/多行事件、单事件/总量上限、有界背压、空闲/总超时、取消和部分输出。
5. 自动重试只覆盖只读或协议明确幂等操作；create/command/upload 不隐式重放。
6. 同一 Fixture 的 Rust 结果与官方 Go SDK 或 `osb` CLI 一致；Spec Hash 或生成物漂移使 CI 失败。
7. 必需字段、事件或版本不兼容稳定返回 `SANDBOX_PROTOCOL_UNSUPPORTED`。

## 5. 任务完成要求

| 任务 | 实施重点 | 必须留下的证据 |
|---|---|---|
| AGT-001 | Provider 只通过 `ModelRuntime`；冻结 `openai_compatible` 映射和价格版本，流式响应可取消 | Provider 契约测试、Token/价格快照、超时/中断 Trace |
| AGT-002 | MCP Server、Tool、Credential 三段 Grant 逐项检查；Schema、幂等和副作用策略沿用 M4 | 未授权前置失败、参数 Hash 审计、重复调用测试 |
| AGT-003 | 从 Skill Version 解析直接和递归依赖，建立授权解释和稳定错误码 | 依赖图、循环依赖、撤权和多租户测试 |
| AGT-004 | LightRAG/Mem0 使用统一 Port，真实 Addon 可选，未配置时不伪造成功 | Fake 契约、Addon 健康、read/write Scope 测试 |
| AGT-005 | Agent State 通过 Artifact Reference 保存大内容；每轮写事件并支持取消/恢复边界 | 多轮 Agent Fixture、状态 Hash、Iteration Trace |
| AGT-006 | 预算在模型/Tool 调用前预留，在结果提交时结算，超额不可继续调用 | 并发预算、Token/成本/时间阈值和幂等结算测试 |
| AGT-007 | 指纹基于规范化 Tool 名称和参数；检测重复、错误循环和 A-B-A-B，保留误杀基线 | 正例/反例 Fixture、停止原因和部分结果测试 |
| AGT-008 | Rust 直接 Adapter 使用固定 OpenAPI DTO/普通 HTTP，加手写 SSE、Endpoint、重试和错误层；sandbox-manager 独占 API Key/execd Token | Spec Hash/生成物、官方 Go SDK/CLI 差分、恶意 Endpoint/Header、SSE 背压/取消、协议不兼容、孤儿回收测试 |
| AGT-009 | Runner 只生成标准 Sandbox Request；首期语言和 Browser 都走 Command API，输出限流并转为 Item/Artifact | Python/JavaScript/Shell/Browser、stdout/stderr、文件、退出码、部分输出和取消测试 |
| AGT-010 | Agentx 配额和 OpenSandbox 限制双重设置；读取 Metrics；网络默认 deny，白名单显式声明 | CPU/内存/PID/磁盘/TTL/并发、Metrics、网络拒绝和跨租户测试 |
| AGT-011 | Credential Handle 短期、按 Execution/操作绑定，优先 Vault，销毁后立即失效 | Secret 不入 argv/stdout/Trace/镜像，过期和撤销测试 |
| AGT-012 | 统一 Trace Event Schema，关联 Execution/Node/Attempt/Iteration/Sandbox，ClickHouse 失败不阻塞状态 | Span 树、成本聚合、降级/补投和脱敏查询测试 |
| AGT-013 | Manifest 驱动 Agent/Code 节点参数和 API；Trace 页面展示授权、预算、循环和 Sandbox 错误 | 前端加载/错误/空状态/权限、生成 Client、Playwright E2E |

## 6. OpenSandbox E2E 规范

`agentx-e2e` Namespace 只承载 Agentx 和测试依赖，OpenSandbox Docker Server 运行在 Docker Desktop 主机或由脚本探测。脚本必须：

1. 检查 OpenSandbox `/health`、固定 Spec/组件兼容矩阵、API Key 和本次测试前 Sandbox 清单。
2. 使用版本化 Fixture 创建包含 Agent、MCP Tool、Code 和 Approval 的 Workflow，运行一次成功和一次失败分支；只通过 Studio UI 创建和配置该 Workflow 的验收归入 M6 STU-002/005/008～011/016。
3. 证明 Python/JavaScript/Shell 只在 Sandbox 执行，并验证文件 Artifact、stdout/stderr 和退出码。
4. 证明默认拒绝网络、允许域名、资源超限、TTL、取消、Worker 强退和 Sandbox Manager 重启后的回收。
5. 断言每个测试 Sandbox 已销毁，保存 OpenSandbox 和 Kubernetes 日志，最后删除 Namespace；失败时也执行清理，`KeepNamespace` 只保留调试资源。

Kubernetes E2E 之前必须先运行不依赖集群的协议测试：固定 OpenAPI Fixture、Spec Hash/生成物校验、Rust 与官方 Go SDK/CLI 差分、SSE 分片/慢消费者/取消、Endpoint SSRF/Header 注入和不兼容版本。Go 工具仅存在于该测试 Job 或开发工具链，不进入 Agentx 镜像和生产部署。

后续生产强化另需在具备 RuntimeClass 的集群执行，检查实际 gVisor/Kata 隔离；该项不阻塞当前 Docker Desktop Kubernetes+runc 基线验收。

## 7. M5 功能完成门禁

以下功能门禁已经全部成立，AGT-001～013 标记为 `done`：

1. AGT-001～013 全部为 `done`，M5 声明可运行的 Provider/Runner 不存在 Fake、伪成功或 `RUNTIME_UNAVAILABLE` 旁路；未纳入首版的 `custom_http` 不在可运行选项中。
2. 所有 Model、Tool、Skill、RAG、Memory 和 Credential 在发布、启动和每次调用前均完成 Grant 校验。
3. Agent 预算和循环策略在并发、重试、取消、超时和服务重启后仍保持幂等。
4. Rust OpenSandbox Adapter 的固定版本、差分契约、SSE、Endpoint、Header、重试和 `SANDBOX_PROTOCOL_UNSUPPORTED` 门禁全部通过，生产链路没有 Go/Python Sidecar。
5. OpenSandbox Sandbox 生命周期、资源 Profile、网络、Credential、Artifact 和强制回收有自动化证据；RuntimeClass 的强制隔离效果列为后续生产强化。
6. M5 临时 Kubernetes E2E 默认清理 Namespace 和 Sandbox，最终 MySQL 状态、Trace 和 Artifact 引用可复核。
7. Agent/Code 节点、Trace 页面和 API 使用生成契约，中文/英文、浅色/深色、加载/错误/空状态和权限状态完整。
8. `docs/plan/99-feature-traceability.md`、路线图、阶段文档和验收证据同步标记，且 M6/M7 只消费冻结后的 Node/Runtime/Trace 契约。

按当前验收范围，阶段 10 已在 Docker Desktop Kubernetes+runc 基线下完成。gVisor/Kata 或等价 RuntimeClass、CPU/PID/磁盘强制、IPv4/IPv6 egress、生产 Vault、镜像签名/供应链和跨租户攻击隔离暂不重试，不重新打开 AGT-001～013；这些内容并入现有 M7 INT-006/010/011/014，不增加原子任务数量。

## 8. 任务完成状态

| 任务 | 状态 | 完成证据 |
|---|---|---|
| AGT-001 | done | OpenAI-compatible 流/非流 Adapter、Credential/价格快照、Tool Call delta、流中断 partial 和 USD/缺价格门禁均有契约测试。 |
| AGT-002 | done | Streamable HTTP/Legacy SSE、Schema/Grant/指纹、unknown-outcome 策略和 Agent E2E 已覆盖。 |
| AGT-003 | done | Skill Artifact/Hash/递归依赖、路径/循环、逐项授权、撤权和跨租户数据库集成测试已覆盖。 |
| AGT-004 | done | 固定 LightRAG 1.5.5/Mem0 1.0.0 Addon 通过真实 Worker Runtime Port；read/write Scope 拒绝在零写入条件下通过。 |
| AGT-005 | done | Run/Iteration/Call Ledger、State Artifact、有序 Tool Loop、多轮 E2E 和跨 Attempt Ledger 恢复测试已覆盖。 |
| AGT-006 | done | 调用前预算预留、并发结算、Token/成本/时间上限、稳定幂等键和重启恢复集成测试已覆盖。 |
| AGT-007 | done | 重复指纹、重复错误、A-B-A-B、State Stall、正反例和 `limitAction` 矩阵已覆盖。 |
| AGT-008 | done | 固定 Spec Hash、官方 Go Oracle、Lifecycle/execd、SSE/Endpoint/Header、协议不兼容、创建结果未知标签对账、Lease/Reaper 和 Manager 重启均有自动化证据。 |
| AGT-009 | done | Python/JavaScript/Shell/Browser 固定 argv Runner、上传/下载、退出码、取消、输出上限和 partial Artifact/Trace E2E 已覆盖。 |
| AGT-010 | done | Sandbox Profile/digest/CPU/内存/PID/磁盘契约、Metrics、内存、TTL、网络 deny/allow、租户并发和回收已实现。当前 Docker+runc 功能基线不重开；CPU/PID/磁盘实际强制、双栈 egress、RuntimeClass 和跨租户攻击隔离暂不重试，转入 M7 INT-006/010/011/014。 |
| AGT-011 | done | Execution/Attempt/Sandbox 绑定 Handle、临时 secret 文件、跨分片脱敏、expiry/replay 和终止撤销已覆盖；生产 Vault 列为后续生产强化。 |
| AGT-012 | done | ClickHouse M5 Schema、MySQL Ledger、Trace Outbox、成本、partial Artifact、ClickHouse 中断补投和查询脱敏已覆盖。 |
| AGT-013 | done | Manifest/编译器、API/生成 Client、Runtime Status、Workbench、loading/error/empty/权限组件测试以及桌面/移动/深浅色真实页面验收已完成；画布配置仍按边界属于 M6。 |

## 9. 后续补全归属

本节只分配尚未完成的强化和产品闭环，不重新打开 AGT-001～013：

| 待补内容 | 当前处理 | 后续任务 | 完成证据 |
|---|---|---|---|
| Agent/Code 节点画布搜索、拖拽和 Manifest 参数表单 | 移交 M6 | STU-002、STU-005、STU-008 | Node Manifest 驱动，不在画布硬编码节点协议 |
| Model/MCP/Skill/RAG/Memory/Credential/Sandbox Profile 统一资源选择 | 移交 M6 | STU-009 | 用户可见性和 Workflow Grant 同时通过 |
| 仅通过 UI 创建 Agent、MCP Tool、Code、Approval Workflow 并运行 | 移交 M6 | STU-005、STU-008–011、STU-016 | Playwright 真实拖拽、连线、Manifest 配置、保存和 Draft Revision 运行，不依赖 Fixture 写入被测业务数据 |
| Agent/Tool/Sandbox Trace 在画布中的定位和调试联动 | 移交 M6 | STU-013 | 从 Trace 定位 iteration、runIndex、Attempt、Artifact 和失败节点 |
| CPU、PID、磁盘和 TTL 的实际强制及租户配额无相互影响 | 暂不重试，移交 M7 | INT-006、INT-010 | 生产候选集群故障注入、最终 MySQL 状态和残留 Sandbox 断言 |
| gVisor/Kata RuntimeClass、IPv4/IPv6 egress 和跨租户攻击隔离 | 暂不重试，移交 M7 | INT-010、INT-011 | 实际 Sandbox Pod RuntimeClass、网络策略和两租户安全矩阵 |
| 生产 Vault、镜像签名、digest 供应链和发布证据 | 暂不重试，移交 M7 | INT-011、INT-014 | Secret/Vault、签名验证、镜像清单和可重复发布证据 |

进入 M6 前还需要修复两个验收工具问题：`scripts/check.ps1` 的生成契约比较必须忽略 CRLF/LF 差异；Playwright/JUnit 输出必须按阶段和运行 ID 隔离，避免有效报告被无环境运行覆盖。这里不要求重跑 AGT-010；完整生产候选复验和 `skipped=0` 的持久化证据由 M7 INT-014 统一生成。这些修正不新增业务任务编号。

## 10. 向 M6/M7 输出

- Model、MCP Tool、Skill、RAG、Memory、Agent 和 Code Node Manifest/Schema。
- 资源授权解释、预算、循环、Sandbox Profile 和 OpenSandbox Adapter 稳定 API。
- Agent/Tool/Sandbox Trace、成本和 Artifact 引用。
- 可由 Studio 配置、由 Application/Evaluation 复用的真实 Execution 运行边界。
