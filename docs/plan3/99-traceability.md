# Plan3 Agent 能力追踪矩阵

状态只允许 `planned`、`in_progress`、`blocked` 和 `done`。P3-00～P3-04 已完成；P3-05 主链已实现，E2E/证据收口中。

执行属性说明：`human_required` 与状态正交，表示必须由维护者在真实 Kubernetes、Provider
或浏览器环境执行并提交证据；它不会把任务状态提前改为 `done`。当前标记为
`human_required` 的追踪任务为 P3M-007、P3M-008、P3X-004、P3X-005、P3X-006 和
P3X-007。P3X-001～003 仍需先完成代码删除、静态审计和空库重建。

| 能力/边界 | 主要契约 | 主要任务 | 权威实现/存储 | 关键验收 | 状态/证据 |
|---|---|---|---|---|---|
| 参考 Pi 行为基线 | Behavior Fixture / Core Contract 1.0 | P3A-002/003 | 固定 Commit + Agentx Fixture | 离线差分与成熟度审计 | done / [P3-00](evidence/p3-00.md) |
| Agent 内置 Model | Definition 6.0 / Manifest 2.0 | P3S-001～006 | Control Definition/Grant；Worker Adapter | 契约/Studio 门禁；运行门禁在 P3-02 | done / [P3-01](evidence/p3-01.md) |
| 独立 Model Node 单次调用 | Model Manifest | P3S-006、P3X-004 | Workflow Worker | P3-E2E-002 | in_progress / P3-01 保持现有行为 |
| Agent Core 唯一 Loop | Core Contract 1.1 | P3R-001～007、P3C-008、P3X-002 | `agentx-agent-core` | P3-E2E-003、删除扫描 | done / [P3-04](evidence/p3-04.md) |
| Core 基础设施零依赖 | Cargo Boundary ADR | P3R-001、P3X-006 | Rust trait boundary | 依赖图/源码静态扫描 | done / `cargo check -p agentx-v2-runtime` |
| 完整 Operation State | Core State V1 | P3A-005、P3R-002/005/006 | Runtime Registers | 全 crash matrix | in_progress / P3-00恢复决策通过 |
| Effect Intent/Settlement | Core Effect Contract | P3R-005/006、P3T-006 | Runtime Transaction/Ledger | P3-E2E-010 | done / [P3-02](evidence/p3-02.md) |
| Model Effect Adapter | Core Effect Contract | P3R-004～007 | Runtime Call Ledger | 真实 Provider、成本唯一、零 Credential in Core | done / [P3-02](evidence/p3-02.md) |
| 可选 Agent Workspace Sandbox | Definition/Bundle 2.0 | P3S-001～005、P3T-001/002 | Control Profile/Runtime Binding | 条件授权、双 Session 生命周期、Lease/Fencing、真实 OpenSandbox | done / [P3-03](evidence/p3-03.md) |
| read/write/edit/bash 条件注册 | Core Tool Contract | P3T-003～006 | Core Registry/OpenSandbox | 无 Sandbox 零注册；有 Sandbox 四工具真实执行 | done / [P3-03](evidence/p3-03.md) |
| Tool Replay Policy | Core Tool/Effect Contract | P3T-003、P3T-007 | Operation State/Ledger | read safe；write/edit/bash sent/unknown 不重放 | done / [P3-03](evidence/p3-03.md) |
| stdio MCP Runtime Sandbox | MCP Server Version / Process Contract | P3C-001～004/009 | MCP Server Binding/Sandbox Lease | 真实 OpenSandbox Process Session、Frame/Fencing/清理与 Agent 冻结 Registry 主链 | done / [P3-04](evidence/p3-04.md) |
| HTTP/SSE/stdio 多 MCP | Agent Tool Registry | P3C-003/004/008/009 | Runtime Binding/Call | 三 Transport Adapter/协议测试、Process E2E 与 Agent Attachment Kubernetes 主链 | done / [P3-04](evidence/p3-04.md) |
| Skill 指令和资产 | Skill Runtime Object V2 | P3C-005/008/009 | Runtime OSS/Binding | 签名、递归闭包、首轮 External Context 与 Agent 主链断言 | done / [P3-04](evidence/p3-04.md) |
| Knowledge 检索和 Citation | Knowledge Effect | P3C-006/008/009 | Runtime Binding/Call | Citation、64 KiB 总边界、Artifact、授权 Registry 与 Adapter 测试 | done / [P3-04](evidence/p3-04.md) |
| Run-scoped Memory Adapter | Memory Effect/Run Scope | P3C-007～009 | Memory Provider/Ledger | recall/write 权限、Run 隔离、Unknown Outcome 与授权 Registry 测试 | done / [P3-04](evidence/p3-04.md) |
| 可信 Subject 跨 Session Memory | Memory Subject Contract | P3M-006/007 | Memory Provider/Ledger + `agent_subject_memory_audit`/clear tombstone | P3-E2E-009/011 | in_progress / [P3-05](evidence/p3-05.md)（`human_required`），新 UI 场景已添加，真实镜像矩阵未执行 |
| Agent Session Entry/Register/Usage | Session State V1 | P3M-001～003、P3R-003 | Runtime MySQL/OSS | P3-E2E-007/010、CAS/Fencing、pending wakeup | done / [P3-05](evidence/p3-05.md) |
| Session/Memory Diagnostics UI/API | Runtime Query/Clear Contracts | P3M-007 | Control BFF + Web Diagnostics | 列表/详情/Usage/Compaction/Recovery、Clear、Memory Audit 权限和键盘/中英文 | in_progress / [P3-05](evidence/p3-05.md)（`human_required`） |
| 显式 Session Policy | Definition 6.0 | P3S-001/005、P3M-002 | Runtime Session Loader | application_session/invocation 加载与隔离 | done / [P3-05](evidence/p3-05.md) |
| Context Projection | Core Contract 1.1 | P3R-002、P3C-005、P3M-004 | Agent Core + Entries | system/external/summary/tail 顺序、当前消息一次 | done / [P3-05](evidence/p3-05.md) |
| threshold/overflow Compaction | Compaction Entry/Effect | P3M-004/005 | Core + Runtime Ledger | P3-E2E-008/010 | in_progress / [P3-05](evidence/p3-05.md)（`human_required`），Core/Runtime 单测通过，Kubernetes 未执行 |
| Agent Session 并发 CAS/Queue | Session Repository | P3M-001/003/006 | Runtime MySQL + durable wakeup command | 并发输入、取消、强退、32 条上限 | done / [P3-05](evidence/p3-05.md) |
| 可信用户跨 Session 记忆 | Subject Contract | P3M-006/007 | Runtime Identity/Memory | P3-E2E-009/011 | in_progress / [P3-05](evidence/p3-05.md)（`human_required`），UI 场景已添加，真实镜像矩阵未执行 |
| Retention/GC/Fork | State Reference | P3M-003/006 | Runtime MySQL/OSS | 引用保护与恢复 | in_progress / P3-05 主链，完整 E2E 未执行 |
| Agent Ledger 和 Trace | Trace/Event 2.0 | P3R-007、P3T-006、P3M-005 | Runtime MySQL/ClickHouse | P3-E2E-012、CH 故障 | in_progress / P3-05 主链，完整 E2E 未执行 |
| Grant 撤销和 Policy Epoch | Bundle/Runtime Grant | P3S-003/004、P3C-009 | 两域各自权威 | 精确资源 Grant、逐工具中途撤权 Runtime Slice 与冻结授权证据主链通过 | done / [P3-04](evidence/p3-04.md) |
| Control 离线运行 | Bundle 2.0 | P3S-004、P3R-004、P3X-004 | Runtime 本地依赖 | Bundle 2.0 完成；离线执行在 P3-02/P3-07 | done / [P3-02](evidence/p3-02.md) |
| Worker 多副本和升级 | Core Version/Worker Contract | P3R-006、P3X-005 | Kubernetes + Runtime State | 2→4→2、PDB、滚动窗口 | planned / `human_required` via P3X-005 |
| Studio 统一视觉与交互 | Manifest 2.0/Editor | P3S-005～008 | Web Editor State | Inspector/Canvas/序列化/键盘/大图门禁 | done / [P3-01](evidence/p3-01.md) |
| 旧路径/远程 Pi 设计删除 | 删除台账 | P3A-001、P3X-001～003 | Repository | 静态扫描、空库、无兼容层 | planned |
| 最终安全/容量/发布 | 全部 | P3X-004～007 | 全部 Runtime 域 | P3-E2E-001～012 | planned / `human_required` |

## 维护规则

- 一项能力只能有一个主要完成任务，其他任务作为依赖。
- 任务 `done` 必须补充测试路径、命令、Run ID 和 `docs/plan3/evidence` 链接。
- 架构边界变化先更新 Plan3 `00`/`01`，再调整任务和本矩阵。
- 新增数据表、端口、Secret、网络出口或 Core 基础设施依赖必须进入相应矩阵。
- Plan3 最终完成时本表不得存在非 `done` 状态。
