# Plan3 可领取原子任务清单

本清单把 [分阶段重构计划](../03-refactor-phases.md) 拆为可领取、实现和验收的原子任务。P3A、P3S、P3R、P3T 和 P3C 任务已完成；P3M 主链已实现，P3M-008 的 Kubernetes 全量证据仍在收口。

## 1. 使用规则

1. 状态只使用 `planned`、`in_progress`、`blocked` 和 `done`。
2. 领取前检查 Git 工作树；已有修改默认属于用户，不得覆盖、回退或删除无关内容。
3. 依赖未完成时不得提前接生产路径；允许任务明确列出的只读调查和 Spike。
4. 一项任务必须同时完成代码、Schema、测试、文档和证据才能标记 `done`。
5. 替代通过后立即删除旧路径，不留 Feature Flag、fallback、兼容 DTO 或旧 Fixture。
6. 新增生产文件不得超过 2000 行；Agent Core 按 message/context/loop/state/compaction/tool/event 模块拆分。
7. 跨 Runtime 边界任务必须说明 Authority、Idempotency、Lease/Fencing、Cancel、Backpressure、Unknown Outcome、Query、Trace 和 Recovery。
8. “参考 Pi”只表示冻结行为基线；不得擅自引入 Pi CLI、npm Runtime、JSONL/SQLite 格式或未完成 Harness API。

## 1A. 执行属性

`human_required` 是与四态任务状态正交的执行属性，不是新的状态值。标记为
`human_required` 的任务必须由维护者在本地 Kubernetes、真实 Provider 或浏览器中执行并
确认，提交可复现证据后才能把正式状态改为 `done`。当前标记如下：

| 任务 | 执行属性 | 人工需要完成的内容 | 当前阻碍 |
|---|---|---|---|
| P3M-007 | `human_required` | 使用当前 Control 镜像复核 Diagnostics 列表/详情、Clear、Memory Audit 的权限、键盘和中英文状态 | 需要真实集群和浏览器会话；代码单测已通过 |
| P3M-008 | `human_required` | 在临时 Namespace 执行 Session、Compaction、Subject Memory、并发恢复、队列上限和清理矩阵 | 首次 Model Call `AGENT_MODEL_ERROR`、Admission prerequisite/Workflow deployment 仍未稳定 |
| P3X-004 | `human_required` | 执行 P3-E2E-001～012 产品矩阵并保存 JUnit、Trace、DB 和清理证据 | 依赖 P3-05 完成及稳定的真实发布矩阵 |
| P3X-005 | `human_required` | 执行扩缩容、Drain、PDB、背压、容量和两小时稳定性验证 | 依赖空库重建和 P3X-004 前置完成 |
| P3X-006 | `human_required` | 执行 NetworkPolicy、Secret、Prompt Injection、Workspace、Subject/Tenant 和供应链攻击矩阵 | 依赖 P3-06 删除审计和可部署的干净版本 |
| P3X-007 | `human_required` | 维护者完成最终发布审查、文档签字和敏感证据清理确认 | 依赖 P3X-004～006 全部有证据 |

P3X-001～003 不标记为纯人工验收：它们仍包含旧实现删除、静态扫描、空库 Schema/Bundle
重建和代码门禁，必须先由开发任务完成，再进入上述人工验收。

## 2. P3-00 参考审计与契约

| ID | 状态 | 依赖 | 实施步骤 | 交付物 | 可验证验收 |
|---|---|---|---|---|---|
| P3A-001 | done | 无 | 盘点 Agent Manifest、Studio、Compiler、Bundle、Worker Loop、Ledger、Session、Trace 和 Sandbox；逐文件登记保留/替换/删除 | [代码地图与删除台账](../p3-00/current-code-map.md) | 所有 `ai_model`、`execute_agent`、首 MCP、旧 State/Fixture 有唯一处置 |
| P3A-002 | done | 无 | 固定 `earendil-works/pi` Commit/版本，审计 License、归因、Agent/loop、Harness、Session、Compaction、四工具实现成熟度 | [参考审计报告](../p3-00/reference-audit.md) | 明确已实现代码、规范 scaffold、仿造项、调整项和排除项 |
| P3A-003 | done | P3A-002 | 将 Message/Turn/Tool/Event、Context、Compaction、工具 Schema/错误/截断固化为输入输出 Fixture | [参考行为 Fixture](../fixtures/agent-core/README.md) | Fixture 可离线复现并绑定 Commit Hash，不依赖真实 Provider |
| P3A-004 | done | P3A-003 | 建立 Rust Core Spike：两 Turn、Tool Result、Context Projection、steer/follow-up、cancel、序列化状态 | `agentx-agent-core` 原型 | 只通过 trait 完成 loop；无 SQL/HTTP/宿主文件依赖 |
| P3A-005 | done | P3A-004 | Spike durable Operation State 与 Intent→Effect→Settlement，在 Model/Tool 每个边界强退 | [恢复原型与状态图](../p3-00/architecture-decisions.md) | safe/never Replay、Unknown、Fencing/CAS 均收敛 |
| P3A-006 | done | P3A-001～005 | 冻结 Definition/Manifest/Bundle/Worker Task 2.0、Core Contract/Trace 1.0/2.0 和 Cargo 边界 ADR | [Schema/Rust 类型/ADR](../p3-00/frozen-contracts.md) | Round trip、未知枚举、非法状态、依赖边界和漂移测试通过 |
| P3A-007 | done | P3A-001～006 | 同步产品、架构、数据、部署与 PlanV2 映射文档 | [P3-00 证据](../evidence/p3-00.md) | 文档不再声称存在远程 Pi Runtime/第八产物，事实检查通过 |

## 3. P3-01 Definition、Manifest 和 Studio

| ID | 状态 | 依赖 | 实施步骤 | 交付物 | 可验证验收 |
|---|---|---|---|---|---|
| P3S-001 | done | P3A-006 | 实现必选 Model、可选 Workspace Sandbox 内置 Reference、显式 Session/Core Policy 和四类 Attachment | Definition 6.0 | Model 缺失、Sandbox 重复、错误类型/操作、隐式默认和旧 Role 全拒绝 |
| P3S-002 | done | P3S-001 | Manifest Slot 增加 `placement`，Registry 发布 Agent v2 | Manifest 2.0/Registry | Studio、Compiler、Publisher 读取同一 Slot 定义 |
| P3S-003 | done | P3S-002 | Compiler 校验基数、Workspace Sandbox→四工具派生、工具名、Replay/Side Effect、Skill 依赖和 Grant | Compiler/IR | 无 Sandbox 的 Agent 合法且四工具为空；错误定位具体依赖 |
| P3S-004 | done | P3S-003 | Bundle 固化 Core Contract/参考版本、Model、可选 Workspace Sandbox、工具策略和 Attachment 闭包 | Bundle 2.0 | Hash 确定；两类 Sandbox Binding 不混淆 |
| P3S-005 | done | P3S-002 | Inspector 编辑必选 Model、可选 Workspace Sandbox 和跨 Execution Session Policy | Node Inspector UI | 无 Sandbox 明示四工具不可用；授权六态、中英文、键盘通过 |
| P3S-006 | done | P3S-002、005 | 画布移除 Model Port，保留四类 Attachment；卡片显示 Model/Core/基础工具摘要 | Agent Node UI | Model Node 无 Agent 兼容端口，截图与语义一致 |
| P3S-007 | done | P3S-001、006 | Serializer/Clipboard/Layout/Undo/Redo 区分内置 Reference 与 Attachment | Editor round trip | 无 `bindingId` 不物化附件；复制后 ID/引用正确 |
| P3S-008 | done | P3S-003～007 | 重建 Schema、Fixture、类型、静态门禁和 Studio E2E | [P3-01 证据](../evidence/p3-01.md) | Definition 6.0 发布、重开和只读版本查看通过 |

## 4. P3-02 Agent Core 最小垂直切片

| ID | 状态 | 依赖 | 实施步骤 | 交付物 | 可验证验收 |
|---|---|---|---|---|---|
| P3R-001 | done | P3A-004～006 | 收口 `agentx-agent-core` Port，并在 Worker 侧建立唯一生产入口 | Core 构建产物与 Worker 依赖 | Core 无基础设施客户端；Agent 不再从生产入口进入旧 Loop |
| P3R-002 | done | P3R-001 | Worker `AgentModelAdapter` 将 Context Projection 映射到现有 Provider Runtime | Model Adapter | OpenAI-compatible Model 请求只由 Core Context 生成，Credential 不进入 Core |
| P3R-003 | done | P3R-001/002 | Durable StatePort、CAS/Fencing 与双 Session Policy | Session State Schema/Repository | invocation 隔离；application_session 使用可信 Session ID 恢复 |
| P3R-004 | done | P3R-002/003 | Worker Agent 入口切换到 Core，发布阶段拒绝 Attachment/Sandbox | Core Worker Adapter | 无 fallback/Feature Flag；非法能力在 Model Effect 前失败 |
| P3R-005 | done | P3R-003/004 | Runtime Call Ledger 对齐 Intent/Effect/Settlement 与 Unknown Outcome | Recovery Harness | 强退不盲目重放，Usage/Cost/Provider Request ID 唯一 |
| P3R-006 | done | P3R-004/005 | Deadline、Cancel、Budget、背压和 Trace 映射 | Budget/Trace Adapter | 稳定终态、脱敏 Trace 和 Artifact 外置 |
| P3R-007 | done | P3R-001～006 | Echo Model E2E、静态门禁和阶段证据 | [P3-02 证据](../evidence/p3-02.md) | 双 Session、重启恢复、离线 Bundle 与发布拒绝矩阵通过 |

## 5. P3-03 OpenSandbox 内置工具

| ID | 状态 | 依赖 | 实施步骤 | 交付物 | 可验证验收 |
|---|---|---|---|---|---|
| P3T-001 | done | P3-02、P3-01 | 冻结 Workspace Lease、Sandbox Port、文件/命令请求、错误和清理契约 | Workspace Sandbox Contract | Tenant/Session/Node 隔离；MCP Runtime Sandbox 不可混用 |
| P3T-002 | done | P3T-001 | Worker Sandbox Manager Adapter、Workspace 生命周期、Lease/Fencing 和 Egress | Workspace Manager Adapter | Control 离线可按 Bundle 恢复；Lease/Unknown Outcome 有稳定终态 |
| P3T-003 | done | P3T-001/002 | Core Tool Registry、四工具 Schema、参数校验、Replay/截断和结果类型 | Core Tool Contract | 无 Sandbox 零注册；有 Sandbox 恰好四工具；名称冲突拒绝 |
| P3T-004 | done | P3T-003 | `read` 文件读取、范围、编码、截断和 Artifact 外置 | Sandbox Read Adapter | 二进制、大文件、路径逃逸、取消和超时通过 |
| P3T-005 | done | P3T-003/004 | `write`/`edit` 原子写入、旧 Hash/Patch 条件更新和冲突处理 | Sandbox File Adapter | 重启不重复副作用；部分写入和并发冲突可解释 |
| P3T-006 | done | P3T-002/003 | `bash` 受控 argv、资源限制、stdout/stderr、超时和 Artifact | Sandbox Command Adapter | 无宿主进程；命令注入、网络和资源越权拒绝 |
| P3T-007 | done | P3T-004～006 | Tool Intent/Settlement、Ledger、Trace、safe/never 恢复和清理 | Durable Core Tool Runtime | Unknown Outcome 不盲重试；双 Worker 不产生重复副作用 |
| P3T-008 | done | P3T-001～007 | Fixture、静态门禁、临时 Kubernetes Namespace 和完整 E2E | [P3-03 证据](../evidence/p3-03.md) | 隔离、取消、重启、Lease 过期、Artifact 和清理矩阵通过 |

## 6. P3-04 外挂能力

| ID | 状态 | 依赖 | 实施步骤 | 交付物 | 可验证验收 |
|---|---|---|---|---|---|
| P3C-001 | done | P3A-006、P3S-004 | 扩展 MCP Server Version 为 tagged transport；stdio 创建/更新必选 Runtime Sandbox | MCP Control Contract/UI | tagged transport、结构化 Credential 行、Owner Department Control Options 与授权六态通过；配置 Grant 不进入 Runtime Admission |
| P3C-002 | done | P3A-006、P3C-001 | 独立扩展 Sandbox Manager 长驻 Process Session：start/stdin/stdout/stderr/wait/interrupt/terminate/reconcile | Sandbox Process Contract | 不依赖 Agent Workspace；有界 Frame、stderr 隔离、Lease/Fencing 和强退通过 |
| P3C-003 | done | P3C-002 | 用 Process Session 完成 stdio MCP initialize、tools/list、Health、Debug 和版本发现 | stdio MCP Discovery | Tool 继承 Server/Sandbox Version，不逐 Tool 配置 |
| P3C-004 | done | P3C-003、P3R-004、P3T-005 | 将 HTTP/SSE/stdio MCP Binding 统一注册为 Core Tool，按 Server 复用进程 | MCP Runtime Adapter | 无 Agent Sandbox 仍能调用 stdio；同 Run 调多 MCP；名称冲突拒绝 |
| P3C-005 | done | P3C-004、P3T-003 | 按参考 Prompt/Resource 语义加载签名 Skill 指令、资产、递归依赖 | Skill Adapter | 不读本地 Pi/Worker 目录；篡改 Hash 拒绝 |
| P3C-006 | done | P3R-004、P3T-005 | 将 Knowledge Binding 注册为检索工具并生成 Citation | Knowledge Adapter | 多知识库、Top K、来源、脱敏、Token 预算通过 |
| P3C-007 | done | P3R-004、P3T-005 | 将 Memory Binding 按 Read/Write Grant 注册 recall/写工具 | Memory Adapter | Read-only 不暴露写；Memory 严格限定 Agent Run Scope |
| P3C-008 | done | P3C-004～007 | 统一 Tool Registry 名称、Schema、Replay/Side Effect、数据边界和预算 | Capability Registry | 未授权能力不进入模型 Tool 列表或 Context |
| P3C-009 | done | P3C-001～008 | 运行前与 Effect/Process Start 前验证 Grant/Policy Epoch，覆盖撤权 | Revocation Integration | Tool→Server→Sandbox→Credential 逐项校验；Control 离线符合 LKG |
| P3C-010 | done | P3C-001～009 | Fixture、静态门禁、临时 Kubernetes/OpenSandbox E2E 与证据收口 | [P3-04 证据](../evidence/p3-04.md) | Runtime Slice、Process Session 与发布 Bundle→Workflow Worker→Agent Core→Attachment Kubernetes 主链均通过且零残留 |

## 7. P3-05 Session、Context、Compaction 和长期记忆

| ID | 状态 | 依赖 | 实施步骤 | 交付物 | 可验证验收 |
|---|---|---|---|---|---|
| P3M-001 | done | P3A-006、P3R-005 | 增加 Agent Session Entry/Register/Usage/Operation Schema、Repository 和 CAS | Runtime Repository | Runtime Slice、Core Session Store 和 Schema Drift 测试通过 |
| P3M-002 | done | P3M-001 | 按 Session Policy + stable Node Key 加载/创建 `main` Lane，当前 Message 去重 | Session Loader | Runtime Slice 覆盖 application_session/invocation、版本漂移和可信 Session ID |
| P3M-003 | done | P3M-001/002 | 实现 append-only Entry、完整 Operation State、Terminal Transaction、Busy/Queue 和 pending wakeup | Durable Session | CAS/Fencing、32 条上限、幂等队列和 recovery `resume_execution` 命令已实现 |
| P3M-004 | done | P3M-003 | 实现最新 Compaction + retained tail + recent 的 Context Projection | Context Runtime | Core threshold/overflow、Summary/tail、当前消息去重测试通过 |
| P3M-005 | done | P3M-003/004 | 实现 threshold/overflow Compaction、Intent/Settlement、Usage、Trace 和恢复 | Compaction Runtime | Core 14 个 driver fixture 与 Runtime 100 项库测试通过 |
| P3M-006 | done | P3M-003、P3C-007 | 实现 durable steer/follow-up、取消、Retry、Retention/GC 和可信 Subject Memory | Session/Memory Runtime | Subject Scope、Clear tombstone、队列幂等和隔离逻辑已实现 |
| P3M-007 | in_progress | P3M-004～006 | 提供受权的 Session/Compaction/Memory 查询、清除和审计 UI/API | Session Diagnostics（`human_required`） | Runtime Query API、Clear/Memory audit schema、Diagnostics UI、2 个页面单元测试和 Playwright 场景已实现；仍需维护者在当前 Control 镜像上复核浏览器和权限矩阵 |
| P3M-008 | in_progress | P3M-001～007 | 运行 Core/Runtime/Kubernetes E2E、静态门禁并形成阶段证据 | [P3-05 证据](../evidence/p3-05.md)（`human_required`） | 最新镜像上的 invocation Session 与完整 Runtime 基线已通过；持久 application_session、Compaction、Subject Memory、并发恢复和清理矩阵仍待维护者执行，且当前存在真实集群集成阻碍 |

## 8. P3-06～07 切换与验收

| ID | 状态 | 依赖 | 实施步骤 | 交付物 | 可验证验收 |
|---|---|---|---|---|---|
| P3X-001 | planned | P3S/R/T/C/M 全部 | 将生产/Debug/Evaluation Agent Fixture 切到 Agent Core 路径 | 新 Fixture/生成文件 | 全 Workspace 不再引用旧 Agent Contract |
| P3X-002 | planned | P3X-001 | 删除剩余旧 State/Slot/UI 与远程 Pi Runtime 残留，并复核 P3-02/P3-04 已删路径 | 删除提交与静态规则 | `rg` 不存在旧符号、远程 Pi 协议、fallback 或双模 |
| P3X-003 | planned | P3X-002 | 重建空库 Migration、Schema、Bundle、OpenAPI 和部署清单 | Plan3 Candidate | 空库 Bootstrap、编译、lint、单元/契约全通过 |
| P3X-004 | planned | P3X-003 | 运行 P3-E2E-001～012 临时 Kubernetes 产品矩阵 | E2E Run/JUnit/Trace/DB 证据（`human_required`） | 全场景通过且 Namespace 清理 |
| P3X-005 | planned | P3X-003 | Worker 双副本、扩缩容、Drain、PDB、背压、容量与稳定性 | Capacity/Failure 证据（`human_required`） | 无 Session 粘性、重复 Effect 或状态漂移 |
| P3X-006 | planned | P3X-003 | 执行 Core 依赖、两类 Sandbox、stdio 进程、NetworkPolicy、Subject、Workspace、Secret、Prompt Injection、供应链矩阵 | Security 证据（`human_required`） | 越权全拒绝；Agent/MCP Sandbox 无隐式共享；Core 只有冻结 trait 出口 |
| P3X-007 | planned | P3X-004～006 | 更新设计文档、PlanV2 映射、追踪矩阵和发布审查 | Plan3 Final Review（`human_required`） | 所有任务/阶段/矩阵一致且全部 `done` |

## 9. 单任务完成定义

- 依赖为 `done`，没有未记录架构变化。
- 正向、拒绝、重复、乱序、取消、超时、响应丢失和强退按风险覆盖。
- 每个外部 Effect 的 Intent/Settlement/Replay Policy 有测试。
- Schema/Migration 有空库和生成漂移测试。
- UI 有 loading、empty、error、permission、中文、英文和键盘测试。
- `cargo xtask check`、Rust Core 指定测试和前端检查通过。
- 证据包含 Commit、参考 Pi Commit、镜像 Digest、Schema Hash、环境、命令、结果和敏感数据清理。
- 替代后旧文件和兼容路径已经删除。
- 同步更新本清单、Plan3 README、阶段文档和追踪矩阵。
