# Plan3 基线、架构决策与删除边界

## 1. 当前基线

当前 V2 已具备可复用的外围能力：

- Workflow Definition/IR/Manifest、Execution Snapshot 和 Bundle。
- Runtime 本地 Resource Binding、Grant Projection、Vault Handle 和受控 Egress。
- Agent Run/Iteration/Runtime Call Ledger、预算、Trace 和 Artifact。
- OpenSandbox Adapter、Sandbox Manager、Lease、TTL、配额和 Network Policy。
- Application Session、Message、Invocation、Session Context 和 Retention。
- Studio Attachment、Resource Picker、资源六态授权和 Editor/Definition 分离。

当前 `agent` 实现与目标存在以下偏差：

| 维度 | 当前事实 | Plan3 目标 |
|---|---|---|
| Agent Loop | Rust Worker 中的临时顺序循环 | 在 Worker 内运行、仿照参考 Pi Agent 的 Agentx 原生 Agent Core |
| Model | 必需 `ai_model` 画布 Binding Slot | Inspector 内部必选 Model Reference |
| MCP | Manifest 允许多个，Runtime 只选择首个 | 所有已绑定 MCP Tool 注册为 Agent Core Tool |
| Skill | 资源可冻结，Agent Loop 不消费 | 以冻结指令、资产和依赖注入 Agent Session |
| Memory | 独立节点可调用，Agent Loop 不消费 | 短期状态内置；长期 Memory 作为显式外挂 |
| Knowledge | 独立 RAG 调用，Agent Loop 不消费 | 外挂后注册为受权检索能力 |
| Context | 单次输入对象替代消息上下文 | AgentMessage + context projection/transform + compaction |
| File tools | Agent 不具备 | read/write/edit/bash 经 OpenSandbox |
| Recovery | Agent Run Ledger 存在，完整 Core Operation State 不存在 | Intent/Effect/Settlement/Compaction 边界持久化并恢复 |

当前 MCP 控制面只接受 Streamable HTTP 和 Legacy SSE，显式拒绝 stdio；现有 Sandbox Adapter 只覆盖 create/get/kill、一次性 Command SSE、interrupt、文件上传下载和 Metrics。支持 stdio 前必须新增受 Lease 管理的长驻 Process Session，不能用一次性 `bash` 调用伪装。

资源 Attachment 不是 Workflow 主执行节点。前端 Serializer 已将 Binding Edge 保存为 Node `resourceReferences`，而不是 Definition `connections`；Plan3 保留这个正确边界，只调整 Model 的展示和 Agent Runtime 消费方式。

## 2. 参考 Pi Agent 基线

Plan3 的行为参考是 [earendil-works/pi](https://github.com/earendil-works/pi)，不是此前文档引用的 `badlogic/pi-mono`，也不是把 Pi CLI 或 npm 包作为 Agentx 的远程运行服务。

本次调查固定到：

- Git Commit：`a69bef789bc95abf0acee16f7b4660b70b650bb9`。
- `@earendil-works/pi-agent-core`：`0.84.2`。
- License：MIT；P3-00 仍需完成正式 License Notice、源码归因和供应链审查。

选择性仿造的基础实现：

- `packages/agent/src/agent.ts` 与 `agent-loop.ts`：AgentMessage、Agent State、Turn/Tool loop、事件、steering/follow-up、`transformContext` 和 `convertToLlm` 边界。
- `packages/agent/src/harness/compaction/**`：阈值/overflow 压缩、结构化 Summary、retained tail 和 Token 预算语义。
- `packages/agent/src/harness/session/**`：会话条目、Context 投影和恢复语义。
- `packages/agent/src/harness/tools/read.ts|write.ts|edit.ts|bash.ts`：四个基础工具的 Schema、错误、截断和结果呈现语义。
- `packages/agent/docs/harness.md`：不可变 Entry、完整 Operation State、Usage Ledger、effect intent/settlement 和恢复状态机的设计约束。

参考仓库当前 Commit 中新的 `AgentHarness` 仍是 scaffold，大量执行 API 会抛出 `HarnessNotImplemented`；`docs/harness.md` 也是正在落地的实现规范。因此 Plan3 不把它描述成可直接接入的成熟远程 Runtime。Agentx 要在 Rust 中实现自己的 `agentx-agent-core`，以冻结的行为矩阵和差分 Fixture 验证所选语义，而不是追求源码、存储格式或 API 的逐行兼容。

明确不仿造：Pi CLI/TUI、本地 JSONL/SQLite 权威路径、用户目录资源发现、Extension/Package 自动加载、Provider Credential 发现、直接宿主文件访问、多 Lane、分支导航、子 Agent 和 Pi 自带部署方式。

## 3. ADR-P3-001：Model 是 Agent 内部构成依赖

结论：接受。

理由：

- Agent 没有 Model 就无法执行，而没有 MCP/Skill/Memory/Knowledge 仍是合法 Agent。
- Model 驱动每一轮 Agent Loop，不是模型可自行选择调用的 Tool。
- 将 Model 作为画布 Attachment 会误导用户把它理解为可插拔工具或执行边。
- 独立 Model Node 和 Agent 内部 Model Reference 具有不同执行语义，不应通过连线建立父子关系。

约束：

- Definition 中 Model 仍是结构化 Resource Reference，继续参与授权、发布冻结、撤权和 Trace。
- 内置不表示秘密写入参数，也不允许绕开 Resource Grant。
- 首期恰好一个 Model，不做 fallback、router 或动态 Model Selector。

## 4. ADR-P3-002：仿照 Pi 行为实现 Agentx 原生 Agent Core

结论：接受。

`agentx-agent-core` 负责：

- 消息顺序、Turn 和 Tool Loop。
- Tool argument validation 后的执行调度语义。
- steering/follow-up 队列。
- Context projection/transform、阈值判断和 compaction 结果语义。
- Agent/Turn/Message/Tool 生命周期事件。

Agentx 负责：

- Workflow Node Attempt 的 Claim、Lease、Fencing、Deadline、Retry 和终态。
- Bundle、Model/Tool/Skill/Memory/Knowledge/Sandbox 精确版本和 Grant。
- Credential Handle、Provider Egress、Sandbox 和 Artifact。
- Runtime Call 幂等、成本、配额、Trace 和审计。
- Agent Session State 的权威持久化、Retention 和跨租户隔离。

实现约束：

- Agent Core 是 Rust Library，随现有 Workflow Worker 构建和部署，不新增常驻服务。
- Core 通过 Rust trait 接收 Model Stream、Tool Effect、State Store、Clock 和 Event Sink；不得直接依赖 SQL、Redis、OSS、Vault、HTTP Provider、Kubernetes 或宿主文件 API。
- 参考 Pi 的行为通过测试 Fixture 冻结；参考仓库后续变化不会自动改变 Agentx 生产语义。
- Pi 的本地文件、内存对象、JSONL/SQLite 格式和 npm API 都不是 Agentx 运行契约。

## 5. ADR-P3-003：Agent Core 内嵌现有 Workflow Worker

结论：接受；撤销独立 `pi-agent-runtime`、Node/TypeScript 工具链、Rust↔TypeScript gRPC 和第八构建产物方案。

理由：

- 用户要求仿造参考 Agent 的基础实现，而不是把参考实现作为外部执行节点。
- Agent loop、Context、Compaction 和基础工具共同组成一个有状态内核；跨进程拆开会额外引入事件确认、状态同步和双重恢复协议。
- 当前 Workflow Worker 已持有 Attempt Lease、Fencing、Runtime Call Ledger、Checkpoint 和外部能力 Adapter，原生内核可直接复用这些权威边界。
- 单一 Rust 进程避免新增 Node Runtime、镜像、Service、PDB、NetworkPolicy 和滚动协议窗口。

固定限制：

- 新建独立 Rust crate `agentx-agent-core`，`agentx-v2-runtime` 只能通过公开接口驱动它，禁止继续在 `worker_runtime.rs` 堆叠循环分支。
- Worker 继续 Claim Agent Attempt 并持有 Lease；Core 不拥有调度、租约或数据库连接。
- Model、MCP、Memory、Knowledge 和 Sandbox 都由 Worker Adapter 实现 Core trait；Core 只看到已授权的能力描述和调用结果。
- Core 的每个外部副作用采用 durable intent → effect → settlement；恢复依据完整 Operation State，不凭缺失字段猜测进度。
- Worker 多副本仍按现有 Claim/Fencing 模型扩缩容，不增加 Session 粘性或新的服务发现。

## 6. ADR-P3-004：四个基础工具属于内核，但执行属于 Sandbox

结论：接受。

首期内置工具固定为：

- `read`
- `write`
- `edit`
- `bash`

工具 Schema、错误、截断和结果呈现以固定参考 Commit 为起点，由 Agentx Core Tool Contract 冻结；执行实现替换为 Agentx OpenSandbox Adapter。`bash` 不是普通文件读取，必须继续受 CPU、Memory、PID、Disk、Timeout、Output Limit 和 Network Policy 约束。

参考实现的工具上下文绑定、取消、流式更新和执行模式需要保留。`read` 标记为可安全重放；`write/edit/bash` 默认不可盲目重放并串行执行。外挂工具必须显式声明 replay/side-effect/execution mode，不能由 Core 按名称猜测。

Agent Workspace Sandbox 是 `0..1` 的内部 Sandbox Profile Reference，不显示为画布 Attachment，也不注入隐藏默认值。未选择时 Core Tool Registry 不注册 `read/write/edit/bash`；选择该 Reference 是启用四工具的显式动作。

## 7. ADR-P3-005：Agent Sandbox 与 stdio MCP Sandbox 分离

结论：接受。

- Agent `workspaceSandbox` 属于 Agent Definition，`0..1`，只服务四个内置工具和 Agent Workspace。
- stdio MCP 的 `runtimeSandbox` 属于 MCP Server Version，创建或更新 stdio Server 时恰好一个；HTTP/SSE Server 不要求 Sandbox。
- 平台通过 `initialize`/`tools/list` 自动发现 Tool，因此不是每个 Tool 重复选择 Sandbox；Tool Version 继承冻结的 Server Version 与 Sandbox Profile Version。
- Agent 连接 stdio MCP Tool 时，Publisher 将 `Tool → Server Version → Sandbox Profile Version → Credential` 依赖闭包固化进 Bundle，但不会把它物化为 Agent Workspace Sandbox。
- 同一 Agent Run 中，同一 stdio MCP Server 的多个 Tool 共享一个受控进程和独立 Sandbox Lease；不同 Server 默认不共享 Sandbox、Workspace、Credential 或网络策略。
- 没有 Agent Workspace Sandbox 仍可调用 stdio MCP；stdio MCP 自己有 Sandbox 也不会向 Agent 开放四个内置工具。

stdio MCP Server Version 必须使用结构化 `command + args`、固定镜像/Profile Version 和 Credential Reference，禁止任意 Shell 字符串、`npx -y`/动态安装、宿主进程和隐式网络放行。发现、调试和运行都走同一 Sandbox Process Session Contract。

## 8. ADR-P3-006：四类状态严格分离

| 状态 | Scope | 权威 | 用途 |
|---|---|---|---|
| Workflow Execution Context | Execution Tree | Execution Snapshot/Runtime State | 业务变量和执行元数据 |
| Workflow Session Context | Application Deployment + Session | Runtime MySQL | 显式声明和 Context Write 的结构化状态 |
| Agent Session State | Session + stable Node Key | Runtime MySQL 元数据 + Runtime OSS Payload | 消息、Tool Result、Compaction 和 Agent 连续性 |
| Long-term Memory | Tenant + Application + verified Subject + Namespace | 外部 Memory + Runtime Binding | 跨 Session 用户事实和偏好 |

禁止用 Workflow Session Context 保存无限消息数组；禁止把 Compaction Summary 当作长期用户记忆；禁止把 Long-term Memory 内容直接作为授权或系统指令。

## 9. 保留、替换和删除

### 9.1 保留

- Workflow 图状态机、Item/Lineage、Expression 和 Context CAS。
- Bundle/Work Package、Runtime Binding、Grant 和 Credential Handle。
- Worker Claim/Lease/Fencing、Outbox、Recovery 和 Runtime Query。
- Agent Run/Iteration/Runtime Call 数据模型，可按新事件语义扩展。
- OpenSandbox、Sandbox Manager、Trace、Artifact、Quota 和 Retention 基础设施。
- Studio Attachment 与 Resource Picker 组件。

### 9.2 替换

- Agent Node Manifest、参数和 UI Schema。
- Agent Model 请求组装。
- Agent State/Checkpoint Payload。
- Agent Tool Registry 和能力适配。
- Agent 节点 Trace 映射与成本汇总。
- Agent Session 查询与调试 UI。

### 9.3 删除

- `ai_model` Binding Slot、端口、Attachment Fixture、翻译和测试。
- 当前 Rust `execute_agent` 临时循环及其状态 Hash/循环指纹实现；由独立 `agentx-agent-core` 取代。
- 只选择首个 MCP Tool 的路径。
- Agent 将首个 Input JSON 直接作为全部内部状态的行为。
- 任何 Pi CLI、本地 Session、Provider Credential、宿主文件工具和 Extension 自动发现路径。
- 独立 `pi-agent-runtime`、Rust↔TypeScript 协议和第八构建产物设计。
- 所有 Plan3 临时双模开关、旧/新 Agent 分流和兼容 DTO。

## 10. 开始实施前的阻断门禁

以下任一项未冻结，P3-01 及后续任务不得开始：

- 参考 Pi Commit、选择性行为清单、License/归因和差分 Fixture。
- `Agent`/loop、Context、Compaction、Session、四工具与 `AgentHarness` scaffold 的成熟度审计。
- Rust Agent Core trait 边界、durable operation state 和 effect intent/settlement Spike。
- Core crate 不直接依赖 Provider/数据库/宿主文件系统的静态边界验证。
- Definition 6.0、Manifest 2.0、Bundle 2.0 和删除台账。
- Worker 内嵌 Core 对并发、内存、背压、滚动升级和容量预算的 ADR。
- Agent 可选 Workspace Sandbox、stdio MCP 必选 Runtime Sandbox、Process Session 和传递 Grant/Bundle 闭包契约。
