# 阶段 10：Agent 与 OpenSandbox

## 1. 目标与用户价值

让 Agent 在明确模型、MCP Tool、Skill、RAG、Memory、成本和循环限制内运行，并将所有不可信代码交给 OpenSandbox 隔离执行。

## 2. 当前状态和进入条件

- 状态：当前 Kubernetes 部署验收 `done`。AGT-001～013 已完成；AGT-010 的生产强化子项暂不重试，gVisor/Kata、生产 Vault、镜像供应链和生产级跨租户强隔离统一由 M7 INT-006/010/011/014 验收，见 [M5 验收证据](m5-acceptance-evidence.md)。
- 进入条件：[阶段 04](04-resource-center.md) 提供资源版本和授权，[阶段 08](08-workflow-runtime-core.md) 提供 Node Manifest、Action/Lifecycle Protocol 和运行 Adapter，[阶段 09](09-checkpoint-wait-recovery.md) 提供等待和恢复。
- OpenSandbox 本地可使用 Docker Runtime，生产使用官方 Kubernetes Runtime；兼容性和隔离边界见 [OpenSandbox 可行性评估](opensandbox-feasibility.md)。
- M5 的执行顺序、现存缺口和分批门禁见 [M5 任务清单](m5-task-list.md)。
- 当前 Kubernetes 证据已覆盖 Agent+MCP、Skill/RAG/Memory、循环停止、Python/JavaScript/Shell/Browser、部分输出 Artifact/Trace、Credential 临时文件、网络 deny/allow、内存限制、自然 TTL、租户并发配额、运行中取消、Sandbox Manager 强杀重启后的 Reaper，以及 ClickHouse 中断补投。CPU/PID/磁盘的 RuntimeClass 强制效果和生产强隔离列为后续生产强化。

## 3. 范围和不做内容

实现 Model、MCP Tool、Skill、LightRAG、Mem0、Agent Tool Loop、成本、循环控制和 OpenSandbox Runner。不实现插件市场、任意未授权外网访问或在 Worker 直接运行动态代码。Sandbox Profile 与 OpenSandbox Connection 是独立资源，不复用 MCP Server、连接测试或 Tool Policy。首个可验收模型协议为 `openai_compatible`；`custom_http` 在版本化请求/响应映射契约冻结前不得作为可运行 Provider 暴露。

M5 的 OpenSandbox 必需子集为 create/get/kill、Endpoint、Command SSE、interrupt、文件上传/下载、Metrics 和网络策略。Python、JavaScript、Shell 以及首期 Browser Runner 统一通过 execd Command API 和固定 digest 镜像执行；完整 Jupyter Context、PTY WebSocket、Pool、Snapshot 和交互式浏览器会话延期，不属于 AGT-008～009 完成门禁。

## 4. 领域对象、状态和不变量

- Model Call 固化 Provider、Deployment、Alias、参数、价格版本、Token、成本和停止原因。
- Tool Call 固化 MCP Server Version、MCP Tool Version、标准化参数指纹、结果、错误和副作用幂等键。
- Skill Loader 固化 Skill Version，并分别检查其 MCP Tool、Model、Credential 和 Artifact 依赖。
- Agent Iteration 包含模型请求、回复、Tool 请求、Tool 结果、Token、成本和 Agent State 引用。
- Agent 限制可配置最大迭代、模型调用、工具调用、Token、成本、时间、单 Tool 错误和重复指纹。
- Sandbox Execution 使用短期凭证、受限网络和资源配额，标准化返回 stdout、stderr、exitCode 和 Artifact。

## 5. 数据和存储

MySQL 保存运行摘要、资源版本引用、价格快照和 Sandbox 元数据；ClickHouse 保存 Model/Tool/RAG/Memory/Agent/Sandbox Trace；MinIO 保存大响应、文件和 Agent State。

Redis 保存租户并发配额、短期 Sandbox Lease 和运行事件，但不保存权威执行结果。

## 6. Port、命令和 Trace

- `ModelRuntime`：流式/非流式调用、Token 和错误标准化。
- `McpToolRuntime`：MCP 会话、Schema 校验、权限、副作用、超时和结果标准化。
- `SkillRuntime`：Manifest 加载、依赖解析和 Prompt/Asset 注入。
- `RagRuntime`：query、retrieve、insert、delete、healthCheck。
- `MemoryRuntime`：get、search、add、update、delete。
- `SandboxRuntime`：create、execute、interrupt、upload、collect、metrics、terminate。该 Port 与供应商无关，由 `OpenSandboxAdapter` 依据固定 Commit/Hash 的官方 Lifecycle/execd OpenAPI 实现；OpenAPI 只负责内部 DTO/普通 HTTP，流、安全和兼容语义由 Rust 手写层实现。

所有 Port 接收 Tenant、Workflow Service Identity、Execution、Node Attempt、Resource Version 和预算上下文。

## 7. 服务和前端改动

- Worker 增加 Agent Runner 和资源 Runtime Adapter 调用，不直接理解 Provider 私有响应。
- Sandbox Manager 隔离 OpenSandbox API、API Key、execd Token、模板、配额、预热、上传和回收；它以 Rust 直接 Adapter 调用 OpenSandbox，Worker 不依赖供应商 SDK 或私有 DTO，生产链路不增加 Go/Python Sidecar。
- Resource Authorizer 在每次模型、Tool、RAG、Memory 和 Skill 加载前二次校验。
- Trace Writer 接收 Agent Iteration、Model Call、Tool Call 和 Sandbox Event。
- Execution/Trace 页面展示 Agent 循环、参数指纹、Token、成本、Sandbox 输出和停止原因。
- 资源页面增加运行健康、价格、配额和版本引用信息。

## 8. 实施任务

| 编号 | 状态 | 依赖 | 交付物 | 验收条件 |
|---|---|---|---|---|
| AGT-001 | done | RES-002、RUN-012 | Model Runtime、统一请求/响应和流式事件 | Provider 错误映射稳定，Token 与价格版本可追溯 |
| AGT-002 | done | RES-003、RUN-012 | MCP Tool Runtime、会话、Schema、权限、超时和幂等 | 未授权 MCP Tool 在发起外部请求前失败 |
| AGT-003 | done | RES-004、AGT-001–002、AGT-004 | Skill Loader、Manifest 和依赖校验 | Skill Grant 不绕过 Model、MCP Tool、Credential、Skill、RAG 或 Memory 的直接/递归依赖权限 |
| AGT-004 | done | RES-005–006、RUN-012 | LightRAG 与 Mem0 Runtime Adapter | 读写操作遵循 Resource Scope 和 Workflow Grant |
| AGT-005 | done | AGT-001–004 | Agent State、Iteration 和 Tool Loop | 多 Tool 循环产生完整有序 Trace 和最终 Item |
| AGT-006 | done | AGT-005 | Token、成本、时间和调用预算 | 任一阈值命中立即阻止后续调用并记录停止原因 |
| AGT-007 | done | AGT-002、AGT-005 | 调用指纹、重复错误和 A-B-A-B 循环检测 | 固定 Fixture 能触发各类循环策略且无误杀基线 |
| AGT-008 | done | FND-004–005、RUN-012 | 固定 Spec 的 Rust Lifecycle/execd Adapter、SSE/Endpoint 安全层和 Sandbox Manager API | 核心子集与官方 Go SDK/CLI 差分一致；Spec 漂移使 CI 失败；协议不兼容明确失败；Worker 不接触供应商对象 |
| AGT-009 | done | AGT-008 | Python、JavaScript、Shell、文件和 Command 模式浏览器 Runner | 代码只在 Sandbox 运行，输出统一转换为 Item/Artifact；不依赖延期的 Jupyter/PTY/Pool/Snapshot |
| AGT-010 | done | AGT-008–009 | CPU、内存、磁盘、Metrics、网络、TTL 和租户并发配额 | Profile/Adapter 契约、内存/TTL/网络/并发/回收已验证；CPU/PID/磁盘的 RuntimeClass 强制效果暂不重试，转入 M7 INT-006/010 |
| AGT-011 | done | RES-001、AGT-008–010 | 短期 Credential 注入和清理 | Secret 不进入命令行、stdout、Trace 或持久镜像；生产 Vault 列为后续生产强化 |
| AGT-012 | done | AGT-001–011、OBS-005 | Model/Tool/RAG/Memory/Agent/Sandbox Trace | 一次 Agent 执行可还原每轮调用、成本和错误 |
| AGT-013 | done | AGT-001–012 | Agent/Code 节点、API 和前端 Trace 展示 | 授权、预算、循环和 Sandbox 错误均有明确 UI |

AGT-008 的前置协议门禁必须先完成，且不新增原子任务编号：

1. 固定源码 Commit、两份 Spec SHA-256 和 Server/execd/egress/模板镜像版本，提交可复现生成/校验方式。
2. 跑通 create/get/kill、Endpoint、Command SSE/interrupt、文件上传/下载、Metrics 和网络策略，不要求覆盖 execd 全部 40 个 Path。
3. Endpoint URL、scheme、Host/Port 和返回 Header 经过严格白名单；API Key 与 execd Token 不跨 Origin、不跟随跨 Host 重定向。
4. SSE 具备分片/多行解析、大小上限、空闲/总超时、有界背压、取消和部分输出语义。
5. 只对只读或协议明确幂等的操作自动重试；create/command/upload 不因网络错误产生隐式重复副作用。
6. 固定 Fixture 与官方 Go SDK 或 `osb` CLI 的差分结果一致，Spec Hash/生成物漂移会使 CI 失败。
7. 不支持的必需字段、事件或组件版本返回 `SANDBOX_PROTOCOL_UNSUPPORTED`。该门禁未通过不得开始 AGT-009。

## 9. 失败、安全和幂等边界

- Provider 流中断保存部分输出和明确错误，不伪装为完整回复。
- Tool 副作用重试遵循阶段 09 的 Side Effect Policy。
- Agent State 大对象通过 Artifact 引用，重复调用指纹基于标准化 Tool 名称和参数。
- Sandbox 默认拒绝未声明网络，使用固定 digest 模板和只读基础镜像。
- 短期 Credential 限定资源、操作、Execution 和 TTL，Sandbox 销毁后立即失效。
- OpenSandbox 不可用只影响需要 Sandbox 的节点，不能使 Coordinator 状态不一致。
- Lifecycle 返回的 Endpoint 视为不可信输入，必须先完成 URL/Origin/Host/Port/Header 校验；API Key 与 execd Token 不得被日志、重定向或错误正文带出。
- SSE 使用有界缓冲和连接/空闲/总超时；取消必须触发 interrupt 和最终回收，流中断不能把部分输出伪装为完整成功。
- 清理 RPC 在执行取消、Attempt 终态或 Lease 过期后仍校验原 Tenant/Execution/Node/Attempt/Worker Lease/Sandbox Lease Token 绑定并允许 interrupt/terminate；普通执行、文件和 Metrics RPC 不放宽活跃 Lease 校验。
- 协议升级只能通过显式更新固定 Spec、生成物、兼容矩阵和差分证据完成；运行时不兼容返回 `SANDBOX_PROTOCOL_UNSUPPORTED`。
- 当前 M5 验收基线为 Docker Desktop Kubernetes + OpenSandbox Docker Runtime + runc；生产 Kubernetes Runtime、gVisor/Kata 或等价 RuntimeClass 由 M7 INT-010/011 验收，当前暂不重试。

## 10. 测试

- Model/Tool/RAG/Memory Adapter 契约测试，使用可控 Fake Provider。
- Skill 依赖图和多层授权测试。
- Agent 多轮、并行 Tool、成本阈值、重复错误和循环 Fixture。
- 固定 OpenAPI Fixture、官方 Go SDK/CLI 差分、Spec Hash 漂移、SSE 分片/背压/取消、恶意 Endpoint/Header 和协议不兼容测试。
- OpenSandbox 超时、资源超限、默认拒绝网络、域名白名单、文件传输和强制回收测试。
- Kubernetes 故障套件已覆盖内存超限、自然 TTL、长命令取消后的 interrupt/terminate，以及 Manager 强杀重启后的强制过期 Reaper；CPU/PID/磁盘已进入 Profile/Adapter 契约，RuntimeClass 下的强制效果列为后续生产强化。
- 租户 Sandbox 并发套件将上限设为 1，证明第二个并发创建稳定失败、拒绝路径不写 Lease 且不调用 OpenSandbox；生产级跨租户攻击面列为后续生产强化。
- Secret 注入、日志脱敏和应用层跨租户授权测试已通过；强隔离集群攻击面验证列为后续生产强化。
- Trace 页面还原 Agent Iteration 和成本端到端测试。
- 临时 Kubernetes E2E 开始前探测 OpenSandbox，结束后断言没有本次测试残留 Sandbox 并删除 Namespace。

## 11. 验收门禁

- Agent 只调用授权资源，Skill 依赖分别校验。
- Token、成本、时间、循环和 Tool 错误限制可以终止或进入错误分支。
- Python、JavaScript、Shell 和动态代码只在 OpenSandbox 执行。
- Rust 直接 Adapter 的版本、SSE、Endpoint、重试和错误门禁通过，生产链路不存在 Go/Python Sidecar。
- Sandbox 超限、服务中断和回收不破坏 Execution 权威状态。
- Trace 能完整还原模型、Tool、RAG、Memory、Agent Iteration 和 Sandbox 过程。
- 当前 Kubernetes+runc 门禁已通过；生产强隔离门禁作为后续部署强化单独记录，不影响当前阶段完成。

## 12. 对后续阶段的稳定输出

- Agent、Model、Tool、Skill、RAG 和 Memory Node Runner。
- OpenSandbox Runtime Adapter、Sandbox Profile 和安全策略。
- 预算、成本、循环检测和 Agent Trace。
- Studio 可配置的 AI 资源端口和参数 Schema。
