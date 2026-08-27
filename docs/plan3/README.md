# Agentx Plan3：参考 Pi 的 Agent 内核重构总计划

状态：`in_progress`。P3-00～P3-04 已完成；P3-05 主链已实现并进入 E2E/证据收口，当前阶段仍为 `in_progress`。

## 1. 目标

Plan3 将 Workflow 中的 `agent` 节点重构为参考 [earendil-works/pi](https://github.com/earendil-works/pi) 基础行为的 Agentx 原生智能体，并纠正当前把 Model、MCP Tool、Memory、Knowledge 和 Skill 作为同级画布附件的领域偏差。

目标形态固定为：

```text
Agent 节点
├─ 内核能力（Inspector 配置或平台策略，不显示画布附件端口）
│  ├─ 精确版本 Model：必选
│  ├─ 仿照参考 Pi 的 Agent loop、消息状态、steering/follow-up
│  ├─ 自动上下文压缩
│  ├─ Workspace Sandbox：0..1（Inspector 内部可选）
│  ├─ read / write / edit / bash（仅选择 Workspace Sandbox 后注册）
│  └─ Agent State
├─ 外挂能力（画布 Attachment）
│  ├─ MCP Tool：0..N
│  ├─ Skill：0..N
│  ├─ Long-term Memory：0..1
│  └─ Knowledge：0..N
└─ Workflow 连接
   ├─ main input
   ├─ main output
   └─ error output
```

Plan3 不是在现有 Rust `execute_agent` 上继续追加历史、压缩和更多分支，也不是新增一个远程 Pi Runtime。目标是在独立 Rust crate `agentx-agent-core` 中仿造参考 Pi 的 Agent loop、Context、Compaction、Session 和四个基础工具语义，再由现有 Workflow Worker 注入 Agentx 的状态、模型、OpenSandbox 和外挂能力 Adapter。

## 2. 固定决策

1. `agent` 节点必须且只能选择一个 Model；Model 在 Node Inspector 内配置并授权，不显示为画布 Binding Port。
2. 独立 `model` 节点继续表示一次普通模型调用，不作为 Agent 子节点，也不能连接到 Agent。
3. Agent 内核以固定参考 Commit 的 Agent loop、消息、工具事件、Context Projection 和 Compaction 行为为设计基线，由 Agentx 在 Rust 中实现；不直接运行 Pi CLI/SDK，不保留旧临时 Loop 作为 fallback。
4. Agent Workspace Sandbox 是 Inspector 内部的可选 `sandbox_profile/use` Reference；未选择时 `read/write/edit/bash` 不进入 Tool Registry，模型看不到也不能调用。选择后四工具才按冻结契约注册，并且只能通过 OpenSandbox Adapter 执行。
5. MCP、Skill、Long-term Memory、Knowledge 是可选 Attachment；它们不是 Workflow 主执行边，也不与 Model 使用相同的 UI 表达。stdio MCP Server Version 创建时必须选择自己的 Runtime Sandbox，自动发现的 Tool 继承该依赖，与 Agent Workspace Sandbox 相互独立。
6. Agent 短期消息状态按 Application Session 与稳定 Agent Node Key 隔离；Workflow Execution Context、Session Context、Agent Session State 和 Long-term Memory 是四类不同状态。
7. 单次 Agent Run 内部的消息上下文始终开启，不提供关闭开关；是否跨 Workflow Execution 延续短期历史由 Agent Inspector 中显式物化的 `sessionPolicy.mode=application_session|invocation` 决定，Runtime 不猜默认值。
8. `application_session` 必须有可信的 Application Session ID；`invocation` 每次创建临时 Agent Session，不产生跨 Execution 的短期历史。Draft Debug、Evaluation、Webhook 和 Schedule 必须显式采用适合其入口的策略。
9. 同一用户不等于同一短期会话。跨 Session 的同用户记忆只能来自显式绑定的 Long-term Memory，并使用经过认证的 Subject Scope；客户端任意填写的 `external_user_id` 不能直接成为读取他人记忆的凭据。
10. `agentx-agent-core` 不直接依赖 MySQL、Redis、OSS、Vault、HTTP Provider、Kubernetes 或宿主文件 API；模型、状态和工具调用只通过 Worker 注入的 Rust trait/Adapter 执行并进入现有 Ledger/Trace。
11. Plan3 使用破坏性切换，不保留 `ai_model` 外接端口、旧 Agent 参数、旧 Rust Loop、双读、双写、兼容 Adapter 或 Feature Flag。
12. 当前契约版本为 Workflow Definition 6.0、Node Manifest 2.0、Execution Spec Bundle 2.0 和 Agent Core Contract 1.1；P3-04 通过重新发布 Bundle 从 P3-00 的 Core Contract 1.0 破坏性升级到 1.1，不提供转换器或兼容运行路径。
13. 不新增第八构建产物、Node Runtime 或远程 Pi 服务；Agent Core 随现有 Workflow Worker 构建，但必须保持独立 crate 和基础设施零依赖边界。

## 3. 非目标

- 不让 Agent Core 接管 Workflow Scheduler、IF/Switch/Merge/Loop、Wait、Approval、Fork 或 Composite。
- 不把 Pi CLI/npm Runtime、本地 JSONL/SQLite、用户目录、Extension、自动更新或 Credential 发现带入生产运行面。
- 不允许参考实现的文件工具直接读写宿主机。
- 不在本阶段新增通用多 Agent 协作、子 Agent、多 Lane/导航、Plan Mode 或任意第三方 Pi Package 市场。
- 不把全部会话原文自动写入长期记忆。
- 不为旧 Workflow Definition、旧开发数据或旧 `ai_model` Binding 生成 Migration。

## 4. 文档目录

| 文档 | 内容 |
|---|---|
| [00-baseline-and-decisions.md](00-baseline-and-decisions.md) | 当前偏差、参考 Pi 行为审计、ADR、保留项和删除项 |
| [01-target-architecture.md](01-target-architecture.md) | 目标调用链、服务职责、状态所有权、安全和恢复边界 |
| [02-contracts.md](02-contracts.md) | Definition/Manifest/Bundle/Core State/Effect/Session 契约草案 |
| [03-refactor-phases.md](03-refactor-phases.md) | P3-00～P3-07 阶段、依赖、交付物和退出条件 |
| [04-e2e-acceptance.md](04-e2e-acceptance.md) | 单元、契约、临时 Kubernetes E2E、故障和安全门禁 |
| [tasks/README.md](tasks/README.md) | 可领取的原子任务清单和固定实施顺序 |
| [99-traceability.md](99-traceability.md) | 能力、契约、任务、代码和验收证据追踪矩阵 |

## 5. 阶段状态

状态只允许 `planned`、`in_progress`、`blocked` 和 `done`。阶段必须同时完成代码、自动化、文档和证据才能标记 `done`。

| 阶段 | 范围 | 状态 | 核心退出条件 |
|---|---|---|---|
| P3-00 | 基线、参考行为、ADR 和契约冻结 | done | [P3-00 证据](evidence/p3-00.md) |
| P3-01 | Definition 6.0、Manifest 2.0、Bundle 2.0 和 Studio | done | [P3-01 证据](evidence/p3-01.md) |
| P3-02 | Agent Core 最小垂直切片 | done | [P3-02 证据](evidence/p3-02.md)：内嵌 Rust Core 使用真实 Model 完成多 Turn Loop 与状态恢复 |
| P3-03 | OpenSandbox 四个内置工具 | done | [P3-03 证据](evidence/p3-03.md)：read/write/edit/bash 全部在隔离 Workspace 执行和恢复 |
| P3-04 | MCP、Skill、Memory、Knowledge 外挂能力 | done | [P3-04 证据](evidence/p3-04.md)：统一 Registry、三类 MCP、外挂 Adapter、授权恢复与真实 Attachment Kubernetes 主链通过 |
| P3-05 | Session、压缩和跨 Session 长期记忆 | in_progress | 最新 Runtime/Control 镜像已重建；Invocation Session 与完整 Runtime 基线已通过，仍需完成 Compaction、Subject Memory、并发恢复和 Diagnostics 浏览器全量矩阵 |
| P3-06 | 破坏性切换和旧实现删除 | planned | 清理 P3-02/P3-03 之外的旧端口、旧 Fixture 和残留设计 |
| P3-07 | 全量 E2E、安全、容量和发布审查 | planned | 临时 Kubernetes 全矩阵、控制面离线和最终门禁通过 |

## 人工执行标记

`human_required` 为独立的执行属性，不改变 `planned`、`in_progress`、`blocked`、`done`
四态状态。当前仍需维护者在真实环境执行并提供证据的任务为：

| 任务 | 标记 | 说明 |
|---|---|---|
| P3M-007 | `human_required` | 当前 Control 镜像上的 Diagnostics 浏览器、权限、键盘和中英文复核 |
| P3M-008 | `human_required` | 临时 Kubernetes 中的持久 Session、Compaction、Subject Memory、并发恢复和清理矩阵 |
| P3X-004 | `human_required` | P3-E2E-001～012 产品端到端矩阵 |
| P3X-005 | `human_required` | 扩缩容、Drain、PDB、容量和稳定性 |
| P3X-006 | `human_required` | 安全、NetworkPolicy、Secret、Prompt Injection 和供应链矩阵 |
| P3X-007 | `human_required` | 最终发布审查、文档签字和敏感证据清理 |

这些任务仍保持当前未完成状态。并非所有剩余工作都是人工验证：P3X-001～003 仍需先
完成旧实现删除、静态审计、空库 Schema/Bundle 重建和代码门禁。另有真实集群集成阻碍
尚未收敛，包括首次 Model Call 的 `AGENT_MODEL_ERROR`、Admission prerequisite 和
Workflow deployment 稳定性；因此当前不能将 Plan3 描述为“只差验证”。

## 6. 关键路径

```text
当前实现与删除台账
  → 参考 Pi Commit/License/行为矩阵与 Rust Core Spike
  → Definition 6.0 + Manifest 2.0 + Bundle 2.0
  → Inspector 内置 Model + 可选 Workspace Sandbox + 四类画布 Attachment
  → Worker 内嵌 Agent Core + Model/State Adapter 最小 Loop
  → OpenSandbox 四工具
  → MCP / Skill / Knowledge / Memory Adapter
  → Agent Session State + Compaction + Long-term Memory
  → 删除旧 Loop 与旧端口
  → Kubernetes 故障、安全、容量和发布验收
```

P3-02 之前不得同时改写全部外部能力。必须先证明一个无外挂 Agent 可以在控制面离线时使用冻结 Bundle、真实 Model 和内嵌 Core 完成执行与强退恢复，再逐项接入工具与状态。

## 7. 与现有计划的关系

- `docs/plan/` 保存 V1 历史能力和验收基线，不回写旧阶段状态。
- `docs/planv2/` 继续是控制面、执行面和可观测面分离及生产认证的状态来源。
- `docs/plan3/` 只负责 Agent 内核、Studio 语义、Agent Session 和外挂能力的破坏性重构。
- Plan3 不降低 V2 的控制面离线、Runtime 权威状态、Claim/Lease/Fencing、受控 Egress、Trace 和多租户隔离要求。
- Plan3 增加新的 Core crate 和 Runtime 状态边界时，必须同步更新 `docs/02`～`10`、`docs/12`、`docs/13` 和 `docs/planv2` 中仍然有效的服务/安全/部署事实。

## 8. 全局完成定义

Plan3 只有同时满足以下条件才能完成：

1. Agent 没有内部 Model Reference 时在保存或编译阶段失败；画布不存在 Model Attachment Port。
2. `agentx-agent-core` 是唯一 Agent Loop；静态扫描无法找到旧临时 Loop、旧 `ai_model`、远程 Pi Runtime 或双模开关。
3. Model、MCP、Memory、Knowledge、Skill 和 Sandbox 调用全部来自冻结 Bundle 与 Runtime Grant，没有 Control 回读。
4. Agent Core crate 不直接依赖数据库、Redis、OSS、Vault、Provider HTTP、Kubernetes 或宿主文件 API，所有 Effect 只通过 trait/Adapter。
5. 未选择 Agent Workspace Sandbox 时四工具不会进入模型 Tool Registry；选择后只能操作正确 Tenant/Execution/Agent Session 的 OpenSandbox Workspace。
6. stdio MCP Server Version 必须冻结自己的 Sandbox Profile；其进程、Credential、网络、Lease 和 Workspace 不与 Agent Workspace Sandbox 隐式共享。
7. 同 Session、不同 Agent Node、不同 Session、不同 User、不同 Application 和不同 Tenant 的状态隔离都有自动化证据。
8. `application_session` 与 `invocation` 两种 Session Policy 均有自动化证据；前者缺少可信 Session ID 时明确失败，后者不会在下一次 Execution 加载历史。
9. 自动压缩在阈值和 overflow 两条路径上可重试、可恢复、计费且不删除完整历史。
10. Worker 或 Sandbox 强退后，Attempt 通过完整 Operation State、Effect intent/settlement、Lease/Fencing 和稳定 Effect ID 收敛到唯一结果。
11. Trace 能从 Execution 下钻到 Agent Run、Turn、Model Call、Tool Call、Compaction 和 Sandbox，成本不重复。
12. Studio 使用统一组件和 Manifest 本地化；中英文、浅深主题、键盘焦点和 1440×900 E2E 通过。
13. 临时 Kubernetes Namespace 在成功或失败后均删除，并恢复为测试而缩容的开发服务。
14. 相关设计文档、任务状态、追踪矩阵和证据无矛盾。
