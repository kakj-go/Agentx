# Plan3 分阶段重构计划

## 1. 实施原则

- 参考 [earendil-works/pi](https://github.com/earendil-works/pi) 的基础行为，不照搬 CLI、npm Runtime、存储格式或未完成的 Harness scaffold。
- 先冻结行为矩阵、Core trait 和完整 Operation State，再接真实 Provider、Sandbox 和外挂能力。
- 每阶段保持 Workspace 构建、空库 Migration、Schema 生成和已迁移能力测试可运行。
- Agent Core 必须是独立 Rust crate；不得把新 loop 继续堆入现有 `worker_runtime.rs`。
- P3-06 前允许开发分支存在尚未接线的新模块，但生产路径不得长期保留新旧双模。
- 外部 Effect 先持久化 Intent，再执行，再持久化 Settlement；故障测试必须覆盖两个提交之间的未知窗口。
- 现有用户工作树修改默认保留；实际实施任务开始前重新检查重叠文件。

## 2. P3-00：参考审计、ADR、Spike 和契约冻结

状态：`done`。验收证据见 [P3-00 验收证据](evidence/p3-00.md)。

进入条件：当前代码、PlanV2 架构和参考 Pi Commit 可读取。

实施顺序：

1. 固定参考 Commit、包版本、License 和源码归因。
2. 逐模块审计 `Agent`/loop、Message/Context、Compaction、Session、四工具和 Harness 规范的实现成熟度。
3. 形成“仿造/调整/排除”行为矩阵，不把未实现 scaffold 当成可用能力。
4. 盘点当前 Agent Definition、Manifest、Studio、Compiler、Bundle、Worker Loop、Ledger、Session、Trace 和 Sandbox 路径。
5. 建立最小 Rust Core Spike：两 Turn、Tool Result、Context Projection、cancel 和序列化状态。
6. 建立 durable Effect Spike：Intent → 模拟 Effect → Settlement，并在每个边界强退恢复。
7. 冻结 Definition 6.0、Manifest 2.0、Bundle 2.0、Worker Task 2.0、Agent Core Contract 1.0 和 Trace 2.0。
8. 冻结 Cargo 依赖边界、Worker 内存/并发/升级/容量 ADR 和旧实现删除台账。

退出门禁：

- 参考行为矩阵区分已实现代码、规范草案和明确排除项。
- Core crate Spike 不依赖 SQL/HTTP/文件系统客户端，只通过 trait 完成两 Turn 和 Tool loop。
- Intent/Settlement 每个强退位置可恢复，无不可逆 Effect 盲重放。
- Contract 生成 Rust 类型且无漂移；旧字段与文件有唯一删除任务。

## 3. P3-01：Definition、Manifest、Compiler 和 Studio

依赖：P3-00 `done`。

状态：`done`。验收证据见 [P3-01 验收证据](evidence/p3-01.md)。

实施顺序：

1. 切换 Workflow Definition 6.0 和 Agent Manifest 2.0。
2. 实现 `placement=inspector|canvas` Resource Slot。
3. Compiler 校验一个 Model、最多一个 Workspace Sandbox、显式 Session Policy、四工具派生规则和四类 Attachment。
4. Publisher/Bundle Builder 固化 Core Profile、参考行为版本和完整资源闭包。
5. Studio Inspector 提供必选 Model、可选 Workspace Sandbox Resource Picker 与“跨执行保留上下文”设置，并说明未选择 Sandbox 时四工具不可用。
6. 删除底部 Model Port，保留 MCP/Memory/Knowledge/Skill Attachment。
7. Serializer 区分无 `bindingId` 的内置 Reference 与有 `bindingId` 的 Attachment。
8. 更新 Node Card、Creator、复制粘贴、自动布局、Undo/Redo、国际化和测试。

退出门禁：

- UI 无法创建 Model Attachment 或把 Model Node 连到 Agent。
- Agent 缺 Model、Session Policy 缺失、资源未授权或 Slot 错误时保存/发布明确失败；缺 Workspace Sandbox 是合法配置，但 Bundle/Core Tool Registry 不得包含四工具。
- Model Node 单次调用行为不变。
- Definition/Editor round trip 和 150 节点性能门禁通过。

## 4. P3-02：Agent Core 最小垂直切片

依赖：P3-01 `done`。

状态：`done`。生产入口已经切换到 `agentx-agent-core`；本阶段同时实现 `invocation` 与 `application_session`，但只保留单 Lane、无外挂、无 Workspace Sandbox 的最小能力边界。证据见 [P3-02](evidence/p3-02.md)。

最小范围：一个 Agent、一个内部 Model、无外挂、无 Sandbox Tool、两种 Session Policy。

实施顺序：

1. 收口 `agentx-agent-core` 生产 Port，并保持 Core 零基础设施依赖。
2. Worker 注入 `AgentModelAdapter`，将 Context Projection 映射为 OpenAI-compatible Chat Completions。
3. 通过初始 Runtime Schema 的 `agent_session_entries`、`agent_session_registers`、`agent_session_operations` 和 `agent_session_usages` 实现 CAS/Fencing Durable StatePort。
4. `invocation` 使用 Attempt 派生 Session；`application_session` 使用可信 Application Session ID。
5. Agent Worker 入口只调用 Core；带 Attachment 或 Workspace Sandbox 的 Bundle 在 Model Effect 前拒绝。
6. 将 Model Effect 对齐 Runtime Call Ledger，支持 deadline、cancel、budget、unknown outcome。
7. 物理删除旧 Loop；生产路径不存在 fallback、Feature Flag 或影子执行。

退出门禁：

- Control 全部缩容为零后，已发布 Agent 使用真实模型完成至少两个 Turn。
- Core crate 无基础设施客户端依赖，Worker 镜像不增加 Node/npm/Pi CLI。
- Worker 强退后从完整 Operation State 恢复，模型调用与成本不重复。
- 生产 Agent 路径不再进入旧临时 Loop；旧 Loop 已在 P3-02 从 Worker 物理删除，删除台账已关闭。

## 5. P3-03：OpenSandbox 四个内置工具

状态：`done`。详细任务与实现边界见 [P3-03 Workspace Sandbox 与四个内置工具](p3-03-workspace-sandbox.md)，真实 OpenSandbox、Manager、隔离、恢复和清理结果见 [P3-03 证据](evidence/p3-03.md)。

依赖：P3-02 `done`，现有 Sandbox Manager 可用。

实施顺序：

1. 以参考 Commit 建立 `read/write/edit/bash` Schema、错误、截断和结果差分 Fixture。
2. 冻结 `CoreToolContractV1`、Tool Context、Replay Policy、Side Effect 和 Execution Mode。
3. 将可选 Workspace Sandbox Profile 写入 Bundle；未选择时冻结空 Core Tool Registry，选择时建立 Attempt/Session Workspace Lease 并注册四工具。
4. 实现 read、write、edit、bash 的 Worker OpenSandbox Adapter。
5. 在 Core 中实现 Tool Batch Plan、参数验证、顺序/并行、进度和 Tool Result Entry。
6. 对每个 Tool Effect 实现 Intent/Settlement 与稳定参数/结果 ID。
7. 实现路径逃逸防护、输出上限、Artifact 外置、取消、TTL、Reaper 和 Workspace Checkpoint。
8. 对 `safe` 与 `never` Replay 在每个强退边界运行恢复矩阵。

退出门禁：

- 选择 Workspace Sandbox 的无外挂 Agent 可以在 OpenSandbox 内完成读、写、编辑和命令执行；未选择的 Agent 模型请求中不存在四工具。
- Worker 宿主文件系统不可见且不可写。
- 不同 Tenant/Session/Agent Node 的 Workspace 攻击矩阵全部拒绝。
- `read` 可安全恢复；`write/edit/bash` 未知结果不盲目重放且产生一致 Tool Result/终态。

## 6. P3-04：外挂能力

状态：`done`。三种 MCP、Skill、Knowledge、Run-scoped Memory 的生产实现已进入统一 Registry，MCP 配置页授权六态、真实 OpenSandbox Process Session 和 Workflow→Bundle→Worker→Agent Core→Attachment Kubernetes 主链均已通过。证据见 [P3-04](evidence/p3-04.md)。依赖：P3-02、P3-03 `done`。

实际完成顺序：

```text
MCP stdio Server + Sandbox Process Session
  → HTTP/SSE/stdio 多工具统一 Adapter
  → Skill 指令/资产/递归依赖
  → Knowledge 检索
  → Long-term Memory read/write
  → 撤权和 Runtime Policy Epoch 回归
```

stdio MCP 先完成 Server Version 创建时必选 Runtime Sandbox、发现/调试 Process Session 和传递依赖闭包；之后每类能力依次完成 Manifest/Compiler → Bundle → Core Tool/Context → Worker Adapter → Replay/Side Effect → Ledger/Trace → 授权/撤权 → E2E。

Agent Core Contract 已破坏性升级为 `1.1`。Bundle Entry 固化统一 Attachment Registry、External Context、Tool Schema/Origin/Replay Policy 与逐项授权证据；Worker 使用同一 Router 注入 MCP、Skill Resource、Knowledge 和 Run-scoped Memory，不保留 P3-02 的 Attachment Runtime 拒绝或首 MCP 路径。

退出门禁：

- stdio MCP 缺 Runtime Sandbox 时无法创建；HTTP/SSE 不要求 Sandbox。
- Agent 可同时使用 HTTP/SSE/stdio MCP，名称冲突在发布时拒绝；Agent Workspace Sandbox 与 MCP Runtime Sandbox 不共享。
- Skill 只从签名 Runtime Object 加载，不读取本地 Pi/Worker 目录。
- Knowledge 与 Memory 具有来源、脱敏、Token 预算和 Prompt Injection 防护。
- 未绑定、未授权、已撤权或版本不匹配的能力不会进入 Core Tool Registry/Context。
- Streamable HTTP、Legacy SSE 和 stdio 的协议、恢复、撤权与清理自动化通过；stdio Process Session 的真实 OpenSandbox 临时 Kubernetes E2E 通过且无残留。

## 7. P3-05：持久 Session、自动压缩和长期记忆

状态：`in_progress`。P3M-001～P3M-006 的 Core/Runtime 主链、durable pending wakeup、P3M-007 的诊断 API/UI、页面单元测试和 Playwright 场景已实现；当前源码已重建并导入最新 Runtime/Control 镜像，Invocation Session E2E 和完整 Runtime 基线已通过，但 Compaction、Subject Memory、并发恢复和 Diagnostics 浏览器全量矩阵尚未完成，因此阶段门禁尚未关闭。证据见 [P3-05](evidence/p3-05.md)。

依赖：P3-02、P3-04 `done`；Workspace 连续性依赖 P3-03。

实施顺序：

1. 增加 Agent Session Entry/Register/Usage/Operation Schema、Repository、CAS 和 Retention Reference。
2. 按 Application Session + stable Node Key 创建/加载 `main` Lane，接入当前 Message 去重。
3. 实现 append-only Entry、完整 Operation State、Terminal Transaction 和 Session Busy/队列。
4. 实现 Context Projection：最新 Compaction Summary + retained tail + 之后的消息。
5. 实现 threshold/overflow Compaction、模型 Effect、Usage、Trace 和强退恢复。
6. 实现 steer/follow-up durable queue、取消、Retry、pending input wakeup、Fork/Retention/GC 保护。
7. 冻结可信 Subject Contract，接入跨 Session Long-term Memory。
8. 提供受权的 Session/Compaction/Memory 查询与清除入口。

首期不实现参考 Harness 的多 Lane、Tree Navigation、Branch Summary、子 Agent 或 Pi 存储格式兼容。

退出门禁：

- `application_session` 第二轮使用前一轮 Context；不同 Node/Session 隔离。
- `invocation` 下一次 Execution 不加载历史；持久模式缺 Session ID 明确失败。
- 超长 Session 在 threshold/overflow 两条路径自动压缩后继续，完整历史仍可审计。
- Compaction 任意强退边界可恢复，Usage/成本唯一。
- 同可信 Subject 新 Session 只有绑定长期记忆才可召回；跨 Application/Tenant 与伪造 ID 全拒绝。

人工执行标记：`P3M-007` 和 `P3M-008` 为 `human_required`。它们需要维护者使用当前
Control/Runtime 镜像在临时 Kubernetes 中完成 Diagnostics 浏览器、真实 Provider、
Compaction、Subject Memory、并发恢复、pending queue 和清理矩阵。当前仍受首次
Model Call `AGENT_MODEL_ERROR`、Admission prerequisite 和 Workflow deployment
稳定性问题阻塞，正式状态保持 `in_progress`。

## 8. P3-06：破坏性切换和清理

依赖：P3-01～P3-05 `done`。

剩余删除与全仓审计范围（旧 Loop、首 MCP、Attachment Runtime 拒绝和旧 MCP Transport 已分别在 P3-02/P3-04 删除）：

- `ai_model/ai_tool/ai_memory/ai_retriever/ai_skill` Slot 与翻译。
- Model Attachment Node、Binding Edge、Clipboard/Layout/Test Fixture。
- 旧 Agent State JSON、循环停止指纹和旧 Schema。
- 任何远程 `pi-agent-runtime`、Rust↔TypeScript 协议、Node Runtime 设计残留。
- 所有 Feature Flag、双模部署、兼容 Adapter 和 fallback。

同时重建空库 Migration、Workflow/Bundle/OpenAPI/JSON Schema、Fixture、示例 Workflow、E2E 数据以及相关架构文档。

退出门禁：

- 删除台账全部关闭，静态扫描没有旧符号或兼容例外。
- 空库 Bootstrap 后只能创建 Definition 6.0 Agent。
- Agent Core crate 边界扫描与全 Workspace 快速门禁通过。

## 9. P3-07：最终验收

依赖：P3-06 `done`。

范围：

- 临时 Kubernetes 完整产品 E2E。
- Control 离线、Redis/ClickHouse/OSS/Vault/Provider/Worker/Agent Sandbox/MCP Process Sandbox 故障。
- 双副本竞争、Worker 扩缩容、Drain、PDB、背压、Context/Operation State 容量。
- 双租户、Subject、Workspace、Prompt Injection、Secret 和 NetworkPolicy 攻击矩阵。
- 参考行为差分 Fixture、Rust Core 状态机 crash matrix 和 Runtime Adapter 集成矩阵。
- 中英文、浅深主题、键盘操作和 Trace/Session 调试 UI。

退出门禁见 [E2E 验收](04-e2e-acceptance.md)。全部场景有可复现证据后，P3-07 和 Plan3 才能标记 `done`。

其中 `P3X-004`、`P3X-005`、`P3X-006` 和 `P3X-007` 标记为 `human_required`：前 3 项
需要真实集群故障/容量/安全操作，P3X-007 需要维护者进行最终发布审查、文档签字和
敏感证据清理。P3X-001～003 仍是代码删除、空库重建和静态门禁工作，不属于纯人工验收。

## 10. 状态同步规则

- 开始任务时只把一个原子任务标记为 `in_progress`。
- 阶段第一项任务开始后，README 对应阶段同步为 `in_progress`。
- 完成任务时同步更新任务表、阶段状态和 [追踪矩阵](99-traceability.md)。
- 发现架构偏差先更新 `00`/`01` ADR，再调整任务；不能只在代码中形成新边界。
- 证据目录约定为 `docs/plan3/evidence/p3-00.md`～`p3-07.md`，原始结果保存到 `artifacts/plan3/<run-id>/`。
