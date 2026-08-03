# 阶段 10：Agent 与 CubeSandbox

## 1. 目标与用户价值

让 Agent 在明确模型、MCP Tool、Skill、RAG、Memory、成本和循环限制内运行，并将所有不可信代码交给 CubeSandbox 隔离执行。

## 2. 当前状态和进入条件

- 状态：`planned`。
- 进入条件：[阶段 04](04-resource-center.md) 提供资源版本和授权，[阶段 08](08-workflow-runtime-core.md) 提供 Node Manifest、Action/Lifecycle Protocol 和运行 Adapter，[阶段 09](09-checkpoint-wait-recovery.md) 提供等待和恢复。
- CubeSandbox 必须按官方要求独立安装，Agentx 只通过稳定 Adapter 连接。

## 3. 范围和不做内容

实现 Model、MCP Tool、Skill、LightRAG、Mem0、Agent Tool Loop、成本、循环控制和 CubeSandbox Runner。不实现插件市场、任意未授权外网访问或在 Worker 直接运行动态代码。Sandbox Profile 与 CubeSandbox Connection 是独立资源，不复用 MCP Server、连接测试或 Tool Policy。

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
- `SandboxRuntime`：create、execute、upload、collect、terminate。

所有 Port 接收 Tenant、Workflow Service Identity、Execution、Node Attempt、Resource Version 和预算上下文。

## 7. 服务和前端改动

- Worker 增加 Agent Runner 和资源 Runtime Adapter 调用，不直接理解 Provider 私有响应。
- Sandbox Manager 隔离 CubeSandbox API、模板、配额、预热、上传和回收。
- Resource Authorizer 在每次模型、Tool、RAG、Memory 和 Skill 加载前二次校验。
- Trace Writer 接收 Agent Iteration、Model Call、Tool Call 和 Sandbox Event。
- Execution/Trace 页面展示 Agent 循环、参数指纹、Token、成本、Sandbox 输出和停止原因。
- 资源页面增加运行健康、价格、配额和版本引用信息。

## 8. 实施任务

| 编号 | 状态 | 依赖 | 交付物 | 验收条件 |
|---|---|---|---|---|
| AGT-001 | planned | RES-002、RUN-012 | Model Runtime、统一请求/响应和流式事件 | Provider 错误映射稳定，Token 与价格版本可追溯 |
| AGT-002 | planned | RES-003、RUN-012 | MCP Tool Runtime、会话、Schema、权限、超时和幂等 | 未授权 MCP Tool 在发起外部请求前失败 |
| AGT-003 | planned | RES-004、AGT-001–002 | Skill Loader、Manifest 和依赖校验 | Skill Grant 不绕过任一直接或间接依赖权限 |
| AGT-004 | planned | RES-005–006、RUN-012 | LightRAG 与 Mem0 Runtime Adapter | 读写操作遵循 Resource Scope 和 Workflow Grant |
| AGT-005 | planned | AGT-001–004 | Agent State、Iteration 和 Tool Loop | 多 Tool 循环产生完整有序 Trace 和最终 Item |
| AGT-006 | planned | AGT-005 | Token、成本、时间和调用预算 | 任一阈值命中立即阻止后续调用并记录停止原因 |
| AGT-007 | planned | AGT-002、AGT-005 | 调用指纹、重复错误和 A-B-A-B 循环检测 | 固定 Fixture 能触发各类循环策略且无误杀基线 |
| AGT-008 | planned | FND-004–005、RUN-012 | CubeSandbox Adapter 和 Sandbox Manager API | Worker 不依赖 CubeSandbox 私有对象，超时可强制回收 |
| AGT-009 | planned | AGT-008 | Python、JavaScript、Shell、文件和浏览器 Runner | 代码只在 Sandbox 运行，输出统一转换为 Item/Artifact |
| AGT-010 | planned | AGT-008–009 | CPU、内存、磁盘、网络、TTL 和租户并发配额 | 超限 Sandbox 被终止且不影响其他租户 |
| AGT-011 | planned | RES-001、AGT-008–010 | 短期 Credential 注入和清理 | Secret 不进入命令行、stdout、Trace 或持久镜像 |
| AGT-012 | planned | AGT-001–011、OBS-005 | Model/Tool/RAG/Memory/Agent/Sandbox Trace | 一次 Agent 执行可还原每轮调用、成本和错误 |
| AGT-013 | planned | AGT-001–012 | Agent/Code 节点、API 和前端 Trace 展示 | 授权、预算、循环和 Sandbox 错误均有明确 UI |

## 9. 失败、安全和幂等边界

- Provider 流中断保存部分输出和明确错误，不伪装为完整回复。
- Tool 副作用重试遵循阶段 09 的 Side Effect Policy。
- Agent State 大对象通过 Artifact 引用，重复调用指纹基于标准化 Tool 名称和参数。
- Sandbox 默认拒绝未声明网络，使用受控模板和只读基础镜像。
- 短期 Credential 限定资源、操作、Execution 和 TTL，Sandbox 销毁后立即失效。
- CubeSandbox 不可用只影响需要 Sandbox 的节点，不能使 Coordinator 状态不一致。

## 10. 测试

- Model/Tool/RAG/Memory Adapter 契约测试，使用可控 Fake Provider。
- Skill 依赖图和多层授权测试。
- Agent 多轮、并行 Tool、成本阈值、重复错误和循环 Fixture。
- CubeSandbox 超时、资源超限、网络拒绝、文件传输和强制回收测试。
- Secret 注入、日志脱敏和跨租户 Sandbox 隔离安全测试。
- Trace 页面还原 Agent Iteration 和成本端到端测试。

## 11. 验收门禁

- Agent 只调用授权资源，Skill 依赖分别校验。
- Token、成本、时间、循环和 Tool 错误限制可以终止或进入错误分支。
- Python、JavaScript、Shell 和动态代码只在 CubeSandbox 执行。
- Sandbox 超限、服务中断和回收不破坏 Execution 权威状态。
- Trace 能完整还原模型、Tool、RAG、Memory、Agent Iteration 和 Sandbox 过程。

## 12. 对后续阶段的稳定输出

- Agent、Model、Tool、Skill、RAG 和 Memory Node Runner。
- CubeSandbox Runtime 和安全策略。
- 预算、成本、循环检测和 Agent Trace。
- Studio 可配置的 AI 资源端口和参数 Schema。
