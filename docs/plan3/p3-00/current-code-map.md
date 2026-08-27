# P3A-001 当前实现代码地图与测试缺口

状态：`done`。盘点基于 2026-08-24 工作树；下表同时记录 P3-00 基线与 P3-02 收口后的当前生产事实。

## 1. Agent 生产链路

```text
Workflow Definition 6.0 Inspector/Canvas References
  → agentx-runtime registry/compiler（Manifest 2.0、Agent IR）
  → agentx-bundle-builder（Bundle 2.0 固化 Runtime Work Package）
  → agentx-v2-runtime Worker claim
  → worker_runtime.rs::execute_agent_core
     → agentx-agent-core::AgentCore::run
        → Worker Model/State/Budget/Event Ports
           → Provider Runtime Call Ledger / agent_session_entries + agent_session_registers + agent_session_operations / Trace
```

| 边界 | 当前权威实现 | 当前事实 | P3-00 处置 |
|---|---|---|---|
| Definition/领域对象 | `crates/agentx-domain` | Definition 6.0；Inspector Reference 与 Canvas Attachment 分离 | P3-01 已替换旧语义 |
| Manifest/Registry | `crates/agentx-runtime/src/registry.rs` | Manifest 2.0；Model 为必选 Inspector Slot，外挂能力为 Canvas Slot | P3-01 已替换旧语义 |
| Compiler | `crates/agentx-runtime` | 按 Inspector/Canvas placement 校验资源基数、闭包和派生 Core Tools | P3-01 已冻结 Agent IR |
| Bundle | `crates/agentx-bundle-builder` | 编译 Definition 并固化 Bundle 2.0 资源引用、Hash 与 Agent Core 元数据 | P3-01 已升级 Bundle 2.0 |
| Worker 调度 | `services/agentx-v2-runtime/src/worker_runtime.rs` | `NodeCapability::Agent` 唯一进入 `execute_agent_core` | P3-02 后保持唯一生产入口 |
| Agent loop | `agentx-agent-core::AgentCore::run` + Worker Adapter | Core 通过 Port 驱动模型、状态、预算和事件；Worker 负责基础设施适配 | P3-02 已替换旧临时循环 |
| Effect Ledger | 同上 `runtime_calls` 路径 | 已有稳定调用 ID、请求指纹、响应和 Usage 记录 | 保留并适配 Intent/Settlement |
| Agent Ledger | Runtime Migration + Worker | `agent_runs/agent_iterations` 保存运行和迭代聚合 | P3-02 后按 Operation/Turn 扩展 |
| Trace | `worker_runtime_agent_trace.rs` | Agent Run、Iteration、Runtime Call 映射到 Trace | 保留 Trace Sink，P3-02 升级事件映射 |
| Session | Runtime Session/Execution Context | 当前 Agent State 随执行参数/运行行推进，不是完整 Core Operation State | P3-05 替换为 Entry/Register/Usage |

生产入口唯一性结论：当前只有 Worker 的 `NodeCapability::Agent → execute_agent_core` 驱动 Agent；独立 `model` 节点走普通模型调用，不是 Agent 子节点。P3-02 后 `agentx-agent-core` 已成为 Worker 的生产依赖。

## 2. Studio 和资源连线

- Registry 在 Agent Manifest 2.0 中声明 Inspector Model/Workspace Sandbox 与 Canvas Attachment Slot。
- `apps/web/src/features/workflow-designer` 将 Model 和 Workspace Sandbox 保存在 Agent Inspector；MCP/Skill/Memory/Knowledge 仍物化为 Attachment Node 和 Binding Edge。
- `definition-validation.ts`、Canvas、Clipboard、Layout、Undo/Redo 已按 placement 区分两类引用；Model 不再有 Agent Canvas Port。

## 3. MCP 与 Sandbox

```text
MCP Control create/update
  → mcp_api.rs::validate_config
  → 仅 streamable_http | sse
  → Server Version / discovered Tool

Sandbox Manager
  → 一次性 SandboxExecuteRequestV1（独立 Code Node）
  → Agent Workspace acquire/tool/release
  → OpenSandbox read/write/edit/bash
  → Workspace Lease/Fencing/TTL/Reaper
```

- `services/platform-control/src/mcp_api.rs::validate_config` 明确只接受 `streamable_http|sse`，现有测试明确拒绝 `stdio`。
- MCP Debug Client 只支持 `streamable_http`，SSE 也不由该 Debug Client执行。
- `services/agentx-v2-runtime/src/sandbox_workspace.rs` 已实现 Agent Workspace 的 acquire/tool/release、Application/Invocation 生命周期和四工具；Worker 只通过 Sandbox Manager Egress Adapter 调用。
- `services/agentx-v2-runtime/src/sandbox.rs::SandboxExecuteRequestV1` 仍是独立 Code Node 的一次性执行；现有 Adapter 没有 stdio MCP 所需的 start/stdin/frame/wait/interrupt/terminate/reconcile 长驻会话。
- Agent Workspace Sandbox 和未来 MCP Runtime Sandbox 没有共享当前实现；P3-04 必须继续保持两个独立资源/Lease 边界。

## 4. 当前测试覆盖与缺口

已有覆盖：Agent 预算与迭代、Runtime Call、Trace、Manifest Slot、Studio Binding、MCP transport 拒绝、Sandbox 命令终态和 Outcome Unknown。

P3-00 已补覆盖：纯 Rust 两 Turn/Tool loop、Sandbox 条件工具注册、steering/follow-up、overflow Compaction、取消/预算、CAS/Fencing、Operation 恢复决策、Fixture/Schema 漂移。

P3-03 已补齐四工具 OpenSandbox Adapter、Workspace Lease/Fencing/TTL/Reaper、Artifact 和副作用恢复策略。后续阶段缺口：stdio Process Session、四类外挂能力、完整 Compaction/长期记忆、真实 Grant 撤权及最终容量/安全 Kubernetes 矩阵。
