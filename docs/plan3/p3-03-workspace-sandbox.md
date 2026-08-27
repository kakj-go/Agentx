# P3-03 Workspace Sandbox 与四个内置工具

状态：`done`。实现与真实 OpenSandbox 验收见 [P3-03 证据](evidence/p3-03.md)。

P3-03 是 P3-02 之后的下一阶段。目标是在不改变 Agent Core Loop、Session Policy 和 Model Adapter 的前提下，为 Agent Workspace Sandbox 接入真实的 `read`、`write`、`edit`、`bash` 工具。

## 前置条件

只有满足以下条件才允许开始生产实现：

- P3-02 的真实 Model、`invocation`/`application_session`、CAS/Fencing 和 Unknown Outcome 门禁已关闭。
- Definition 6.0、Manifest 2.0、Bundle 2.0 中 Workspace Sandbox 是 Agent Inspector 内部 `0..1` Reference。
- Agent Workspace Sandbox 与 MCP Runtime Sandbox 的 Binding、Lease、Credential、Workspace 和 NetworkPolicy 已保持独立。
- Core 仍不依赖 SQL、HTTP、Kubernetes、宿主文件系统或进程 API。

## 阶段边界

本阶段包含：

- Agent Workspace Sandbox Profile 的运行时解析、Lease 和生命周期；
- 四个 Core Tool 的 Schema、授权、参数校验、结果规范化和恢复策略；
- 文件路径安全、输出上限、Artifact 外置、取消、超时和清理；
- Worker Adapter → Sandbox Manager → 隔离 Workspace 的端到端执行。

本阶段不包含：

- MCP HTTP/SSE/stdio、长驻 MCP Process Session；
- Skill、Knowledge、Long-term Memory；
- Agent Workspace 与 MCP Runtime Sandbox 的复用；
- 多 Lane、Branch、Fork、长期记忆或新的 Compaction 算法；
- Worker 宿主文件系统访问、宿主进程启动或本地目录兜底。

## 固定运行模型

```text
Agent Bundle
  └─ Workspace Sandbox Version（可选）
       └─ Worker 申请 Workspace Lease
            └─ Sandbox Manager 创建隔离 Workspace
                 └─ Core Tool Adapter 执行 read/write/edit/bash
                      └─ Intent → Effect → Settlement → Artifact/Trace
```

- 未配置 Workspace Sandbox：Bundle 的 `coreTools=[]`，模型请求不得出现四个工具，运行时调用直接返回 `AGENT_WORKSPACE_SANDBOX_RUNTIME_UNAVAILABLE`。
- 已配置 Workspace Sandbox：四工具全部注册，并且每次调用都携带 tenant、execution、agent session、attempt、fencing token 和 Workspace Lease。
- 四工具只能通过 Sandbox Port 执行；Worker 不解析路径、不读写本地文件、不启动本地进程。
- `read` 声明为可证明安全的只读操作；`write`、`edit`、`bash` 默认 `never` Replay，未知结果不得盲目重试。

## 任务拆分

### P3T-001：Workspace Sandbox Runtime Contract

依赖：P3-02、P3-01。

冻结 `WorkspaceLeaseV1`、`SandboxFileRequestV1`、`SandboxCommandRequestV1`、结果、错误和清理契约。明确 Workspace 的创建、复用、心跳、TTL、取消、终止、过期清理、Lease/Fencing、NetworkPolicy 和 Credential Scope。

验收：不同 Tenant、Application、Session、Agent Node 的 Workspace 不可互见；MCP Runtime Sandbox 的 Lease/Credential/NetworkPolicy 无法注入；缺失、过期或撤销 Lease 在 Effect 开始前拒绝。

### P3T-002：Sandbox Manager Worker Adapter

依赖：P3T-001。

在 Worker 中实现 Sandbox Port Adapter，复用现有 Sandbox Manager Egress 和一次性命令能力，不在 Worker 中增加 Kubernetes SDK 或宿主进程调用。建立稳定 `workspace_id`、`lease_id`、`operation_id` 和 `idempotency_key`。

验收：Control Plane 离线时可仅凭 Bundle 和 Runtime Snapshot 创建/恢复 Workspace；Sandbox Manager 超时、拒绝、取消、Lease Lost 和未知结果均映射为稳定 Worker 终态。

### P3T-003：Core Tool Registry 与参数 Schema

依赖：P3T-001、P3T-002。

在 `agentx-agent-core` 中只增加领域 Tool Definition、参数校验和 Tool Result 类型；Core 不实现文件或进程效果。固定四工具名称、描述、输入 Schema、Replay Policy、输出字段、截断规则和错误码。

验收：有 Sandbox 时恰好注册四个工具；无 Sandbox 时四工具不可见且无法通过伪造 Tool Call 调用；Attachment 工具名冲突、未知工具、越权路径和非法参数在 Port 调用前拒绝。

### P3T-004：read Tool

依赖：P3T-003。

支持相对路径、文本/二进制读取、编码声明、行/字节范围、最大返回字节数和大结果 Artifact 外置。禁止绝对路径、`..` 逃逸、符号链接越界、设备文件和 Workspace 外对象。

验收：UTF-8、二进制、空文件、大文件、截断、文件不存在、权限不足、路径逃逸、取消和超时均有确定结果；同一只读操作在可证明条件下可用相同 Idempotency Key 恢复。

### P3T-005：write 与 edit Tool

依赖：P3T-003、P3T-004。

`write` 支持新建/覆盖策略、父目录创建策略、内容大小限制和原子提交；`edit` 支持基于旧内容 Hash 或明确 Patch 的条件更新，避免并发覆盖。两者均不得直接写 Worker 文件系统。

验收：同一 Effect 不会因 Worker 重启被无条件重复；版本/Hash 冲突返回稳定错误；部分写入、响应丢失、取消、Lease Lost 和 Sandbox 清理不会留下未登记副作用。

### P3T-006：bash Tool

依赖：P3T-002、P3T-003。

通过 Sandbox Manager 的受控命令接口执行命令，固定 argv、工作目录、环境变量白名单、CPU/内存/PID/TTL/输出上限和网络策略。支持 stdout/stderr 分离、非零退出、超时、取消和 Artifact 外置。

验收：不存在 Worker 宿主命令执行；命令注入、路径逃逸、未授权环境变量、网络越权和资源超限均被拒绝；Effect 发送后未知结果不会自动重跑命令。

### P3T-007：Tool Operation State、Trace 与恢复

依赖：P3T-004～006。

将四工具接入 Core Operation State、Runtime Call Ledger、Intent/Effect/Settlement 和 Agent Trace。覆盖 Intent 前、发送前、发送后响应前、Effect 完成后 Settlement 前、State Commit 前、取消、Lease Lost、Worker 重启和 Sandbox 过期清理。

验收：`read` 只在可证明安全时重试；`write/edit/bash` 的 Unknown Outcome 进入暂停/失败并产生领域事件；Usage、Cost、Artifact、Trace 和 Tool Result 不重复。

### P3T-008：Fixture、静态门禁与 E2E

依赖：P3T-001～007。

建立 Tool Schema/错误/截断 Fixture、Core Fake Sandbox 测试、Worker Adapter 测试和临时 Kubernetes Namespace E2E。静态检查禁止 `std::fs`、`Command`、直接 Sandbox/MCP 混用、无 Lease 调用和四工具 fallback。

E2E 至少覆盖：

1. 无 Sandbox 的 Agent 不注册四工具；
2. 有 Sandbox 的 Agent 完成 read/write/edit/bash；
3. 多 Tenant/Session/Agent Node Workspace 隔离；
4. Worker 重启、双 Worker 竞争、Lease 过期和 Sandbox Manager 重启；
5. 大文件/大输出 Artifact 外置；
6. 取消、超时、资源超限、路径逃逸和 Unknown Outcome；
7. Control Plane 缩容后使用已发布 Bundle 执行；
8. 测试成功或失败后均清理 Namespace、Workspace 和临时 Artifact。

## 执行顺序

1. P3T-001；
2. P3T-002 与 P3T-003 并行；
3. P3T-004 与 P3T-006 并行；
4. P3T-005；
5. P3T-007；
6. P3T-008。

## P3-03 完成门禁

- Workspace Sandbox 是唯一 Agent 文件/进程效果出口，Worker 宿主不可访问。
- 无 Sandbox 的 Agent 仍然是合法配置，但四工具零注册、零授权、零调用。
- 有 Sandbox 的 Agent 只注册 `read/write/edit/bash` 四个 Core Tool，且所有调用均绑定正确 Workspace Lease。
- MCP Runtime Sandbox 与 Agent Workspace Sandbox 没有任何 Lease、Credential、Workspace 或 NetworkPolicy 复用。
- `read`、`write`、`edit`、`bash` 的参数、错误、截断、Artifact、取消和恢复 Fixture 全部通过。
- 副作用工具 Unknown Outcome 不会盲目重试，双 Worker 竞争不会产生重复副作用。
- Core 依赖边界、Worker 文件行数、Schema 漂移和静态宿主访问门禁通过。
- P3-04 可以在此基础上接入 MCP/Skill/Knowledge/Memory，而无需重新定义 Sandbox 或 Tool Operation 契约。
