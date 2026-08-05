# M4 可靠运行实施任务清单

状态：`done`。RUN-001～014、REC-001～010 和阶段 08/09 门禁均已完成，命令、报告、数据库断言与浏览器检查见 [M4 验收证据](m4-acceptance-evidence.md)。本清单将[阶段 08](08-workflow-runtime-core.md)和[阶段 09](09-checkpoint-wait-recovery.md)的 24 项任务组织为可连续交付的实施批次，并以 n8n Workflow 行为与可实现能力一致作为运行语义目标。

## 1. 完成结果

M4 完成后，平台必须能够：

1. 从不可变 Workflow Version 编译 JSON Fixture，拒绝非法节点版本、端口、表达式、无效拓扑、递归 Sub-workflow 和不可达节点，同时允许带平台激活预算的普通图环。
2. 可靠执行非 AI 节点，正确处理 Item 多来源、IF、Switch、n8n v1 分支顺序、声明式多输入 Readiness、Merge、普通图环、Loop Over Items、Sub-workflow 和错误分支。
3. 以 MySQL 作为 Execution 权威状态，以 Redis Streams 进行至少一次任务投递，并在重复消息、乱序、Worker 崩溃、Lease 过期和服务重启后恢复。
4. 查询 Execution、Node Execution、Attempt、输入输出、错误、Trace 和 Checkpoint，并执行取消、受控重试和超时处理。
5. 从历史 Checkpoint 创建独立 Fork Execution，支持 whole、node、to-node 和 from-node 模式，且不修改原 Execution。
6. 让 Wait 和 Approval 节点持久等待且不占用 Worker，通过幂等 Resume 恢复到正确输出端口。
7. 在 Execution 页面完成 Checkpoint、Fork、等待、审批恢复和副作用确认，并通过临时 Kubernetes E2E 证明完整链路。

M4 不实现 Model、MCP Tool、Skill、RAG、Memory、Agent Tool Loop 或 OpenSandbox 运行，这些属于 M5；不实现完整 Workflow Studio 和动态节点配置界面，这些属于 M6；不在本阶段接通 Application、Evaluation、Trigger Webhook 和 Schedule 到真实 Execution，这些全链路集成仍属于 M7。M4 必须先冻结 Node Lifecycle 和动态 UI Provider 契约，避免后续建立第二套协议。相关入口在接通前继续返回明确的 `RUNTIME_UNAVAILABLE`，不得创建伪运行记录。

## 2. 完成交付基线

- `agentx-runtime` 已提供 Definition 2.0 编译器、表达式适配、SCC/回边、Readiness IR、Canonical Hash 和 Activation/Delivery 状态机，并保持无 SQLx/Redis 依赖。
- 旧 `agentx-node-sdk` 已清理，替换为 `agentx-node-protocol`、Node API OpenAPI/JSON Schema、接入文档、协议一致性 Fixture 和 `echo-node` 参考服务，不提供公共语言 SDK。
- `workflow-coordinator`、`workflow-worker`、MySQL Runtime Repository、Transactional Outbox、Redis 能力队列、Lease、Heartbeat 和 Reaper 已形成可恢复运行循环。
- Migration 0011～0013 已落地 Execution Snapshot、Node Execution/Attempt、Edge Delivery、Item Lineage、Outbox、Lease、Checkpoint、Fork、Wait、Resume、副作用确认和调用期 Broker Handle 数据。
- M3 的 `ExecutionRuntime`、`ApprovalResumePort`、Trace Stream/Writer 和 ClickHouse 查询链路已接入真实运行状态，没有建立旁路或伪运行记录。
- Workflow Definition 已直接升级为 2.0；开发数据和 Fixture 已同步，不维护旧 1.0 Definition 兼容分支。

### 2.1 n8n 一致性边界

- 目标是 Workflow 行为和用户可实现能力一致，包括 Item/Lineage、执行顺序、普通图环、多输入就绪、节点设置、表达式、Wait 和 Sub-workflow 语义。
- n8n Workflow JSON 只作为后续 Import Adapter 输入，不与内部 Definition、IR 或运行表共用 Schema。
- 首期不兼容 n8n npm 社区节点二进制，不复刻全部第三方连接器，也不提供 Rust、Python 或 JavaScript 公共 Node SDK。
- 节点扩展使用版本化 Node Manifest、Node Action/Lifecycle API、OpenAPI/JSON Schema、接入文档和协议一致性 Fixture。
- Agentx 允许提供显式 parallel、不可变 Sub-workflow Version、权限、Checkpoint、激活预算和副作用保护等扩展，但默认 n8n_v1 行为不能被扩展隐式改变。

## 3. 实施原则与冻结点

1. 先纯逻辑后基础设施：Definition、Node Manifest/Protocol、表达式、Compiler 和内存 Activation/Delivery 状态机通过 Fixture 后，才能开始持久化 Scheduler。
2. MySQL 是唯一权威：Redis 消息只携带稳定 ID、Attempt 和 Lease 要求；Coordinator 重启后必须只依赖 MySQL 与 Compiled IR 继续推进。
3. 按至少一次语义设计：队列、Outbox、Worker 提交和 Resume 都必须幂等，不承诺外部副作用 Exactly Once。
4. 恢复不改历史：Retry 追加 Attempt，Fork 创建新 Execution，Trace、Checkpoint 和审计分别保存。
5. 运行服务不反向侵入控制面：`agentx-runtime` 保持无 SQLx/Redis 依赖，基础设施实现 `agentx-application` Port，服务之间不直接依赖。
6. M4 Fixture 至少覆盖 `manual_trigger`、确定性数据变换、IF、Switch、required inputs、Merge、条件回边、Loop Over Items、Wait、Approval、固定版本 Sub-workflow、declarative_http 和 remote_action；AI 资源节点在 M5 前编译或执行时返回明确的不支持原因。
7. `retryOnFail`、`maxTries`、`waitBetweenTries`、`executeOnce`、`alwaysOutputData`、`onError`、超时、取消和副作用策略由 Engine 解释，远程节点不得自行推进 Workflow 状态。
8. 运行契约冻结后同步更新 Workflow 引擎、运行治理、数据模型、OpenAPI 和功能追踪矩阵，不在实现中保留第二套未文档化协议。

冻结点：

- RUN-005 完成后冻结 Definition、Node Manifest/Action/Lifecycle Version、表达式 AST、SCC/branchOrder/Readiness IR、Canonical Hash 和编译错误结构。
- RUN-009 完成后冻结 Execution Snapshot、Activation/Attempt/Delivery/Lineage、Outbox、Lease 和基础 Checkpoint 表结构及 Repository 事务边界。
- RUN-014 完成后冻结 Execution Runtime 命令、事件、查询与 Trace 映射，阶段 08 通过门禁。
- REC-009 完成后冻结 Fork、Partial Run、Wait/Resume、Approval Resume 和 Side Effect Policy API。

## 4. 依赖流程

```text
M4-0 运行契约与 Fixture（RUN-001～002）
  -> M4-1 Node Protocol、表达式与编译器（RUN-003～005）
  -> M4-2 Activation/Delivery 状态机与控制流（RUN-006～008）
  -> M4-3 权威持久化（RUN-009）
  -> M4-4 Coordinator、Outbox 与 Redis Queue（RUN-010～011）
  -> M4-5 Worker、Runner、Lease 与故障恢复（RUN-012～013）
  -> M4-6 Execution API、Trace 与基础 Checkpoint（RUN-014）
  -> M4-7 完整 Checkpoint、Fork 与副作用保护（REC-001～003、REC-007）
  -> M4-8 Wait、Approval、Resume 与清理（REC-004～006、REC-008～009）
  -> M4-9 Execution UI 与 Kubernetes E2E（REC-010）
```

REC-004 可以在 REC-001 和 RUN-010 完成后与 REC-002～003 并行；REC-005～006 沿 Approval 分支推进，REC-007 沿 Fork 分支推进，两条分支必须在 REC-008 前汇合。除这两条明确分支外，不应为追求并行而跨越冻结点。

## 5. 实施批次

| 批次 | 任务 | 主要交付物 | 批次退出门禁 |
|---|---|---|---|
| M4-0 | RUN-001～002 | Definition JSON Schema、Node Manifest、Readiness、Action/Lifecycle、兼容规则和成功/失败 Fixture | n8n 行为边界、Schema 版本、节点设置/端口和协议错误可评审；未知版本稳定失败 |
| M4-1 | RUN-003～005 | Item/Binary/Lineage、Node Protocol、受限表达式、SCC/branchOrder/Readiness Compiler、IR 和 Canonical Hash | 无 SQLx/Redis 的单元与属性测试通过；多来源可回溯；受控图环可编译；相同输入生成相同 IR/Hash/顺序 |
| M4-2 | RUN-006～008 | Execution/Activation/Attempt/Delivery 状态机、IF、Switch、ClosedWithoutData、Readiness、Merge、普通图环、Loop 和 Sub-workflow | 全部控制流 Fixture 确定性通过；普通图环可终止，激活预算阻止无限循环，Join 不永久等待 |
| M4-3 | RUN-009 | 追加式 Runtime Migration、MySQL Repository、Activation/Delivery/Lineage、Execution Snapshot 和事务测试 | 空库及当前 M3 Schema 均能升级；Execution 固化 Version/IR/资源/设置；MySQL 可恢复循环前沿 |
| M4-4 | RUN-010～011 | Coordinator、branchOrder/Readiness、Transactional Outbox、Dispatcher、Redis Streams、能力队列和幂等消费 | 状态变化与消息原子关联；默认分支顺序稳定；重复/乱序投递不创建重复有效 Attempt；Redis 中断后可补投 |
| M4-5 | RUN-012～013 | Worker、builtin/declarative_http/remote_action、Node Action API、Lease、Heartbeat、Reaper、Retry、Timeout、Cancel 和优雅停止 | 只有有效 Lease 能提交；Engine 解释节点设置；远程协议幂等；迟到 Worker 被拒绝；服务重启后状态一致 |
| M4-6 | RUN-014 | 基础 Checkpoint、Execution 命令/API、运行事件、Trace 接入和真实运行状态 | JSON Fixture 可从 UI 命令启动并查询节点结果；ClickHouse 故障不回滚 Execution；阶段 08 门禁通过 |
| M4-7 | REC-001～003、REC-007 | 完整 Checkpoint/State Hash、Fork、部分执行、输入覆盖和 Side Effect Policy | Fork 不改变原记录；依赖缺失可解释；Irreversible 节点必须确认、复用旧输出或 Dry Run |
| M4-8 | REC-004～006、REC-008～009 | 时间/日期/Resume Webhook/Form Wait、Subscription/Token、Approval Resume、超时取消、Artifact、REST/OpenAPI/审计/通知 | 等待不持有 Lease；Webhook/Form 鉴权和响应受控；Approve/Reject/Timeout 进入正确端口；重复和迟到事件最多生效一次 |
| M4-9 | REC-010 | Execution Checkpoint/Fork/确认界面、Kubernetes 故障注入 E2E、验收证据和状态同步 | M4 全部门禁通过，阶段 08/09、路线图和追踪矩阵同步标记 `done` |

## 6. 24 项任务的执行要求

阶段 08 的 RUN-001～014 和阶段 09 的 REC-001～010 是 M4 的唯一原子任务编号，本清单不创建重复编号。实施时按以下完成边界推进：

| 任务范围 | 数量 | 完成边界 |
|---|---:|---|
| RUN-001～005 | 5 | Definition、Node Manifest/Action/Lifecycle、Lineage、表达式和 IR 可独立发布并由 Fixture 验证 |
| RUN-006～008 | 3 | 纯内存状态推进覆盖 branchOrder、Readiness、Activation/Delivery、Merge、普通图环、Loop 和 Sub-workflow |
| RUN-009～014 | 6 | MySQL/Redis 分布式运行、Worker 可靠性、API、Trace 和基础 Checkpoint 可用 |
| REC-001～003 | 3 | 完整 Checkpoint、Fork 和部分执行可用 |
| REC-004～006 | 3 | Wait 与 Approval 持久等待、幂等恢复可用 |
| REC-007～010 | 4 | 副作用保护、清理、公开契约和 Execution UI 可用 |
| **M4 合计** | **24** | 阶段 08 和 09 的退出条件全部满足 |

每个任务只有在领域测试、Migration/Repository 测试、API 契约、权限边界和对应集成测试齐备后才能标记 `done`。批次门禁不是额外任务，也不能替代其中任一原子任务。

## 7. 数据与接口落地顺序

建议使用追加 Migration，避免把运行实现混入已经验收的 M3 表：

1. Runtime Core Migration：Node Manifest/Protocol Version、Execution Snapshot、Node Execution/Activation、Node Attempt、Edge Delivery、Item Lineage、Execution Outbox、Worker Lease、Runtime Idempotency Key 和基础 Checkpoint，并扩展现有 Execution 摘要所需字段。
2. Recovery Migration：Checkpoint Artifact、Resume Token、Wait Subscription、Resume Webhook Binding、Fork/子 Workflow 父子关系、Execution Type、Side Effect Level 和 Resume Policy。

具体表名和字段由 RUN-009、REC-001 固化；实现前先与 `workflow_executions`、`execution_events`、`artifacts`、`outbox_events` 和 `trace_delivery_outbox` 的现有职责去重。

接口接入顺序：

1. Node Manifest、Node Action/Lifecycle、动态 Provider、认证、Artifact/Credential Handle 和协议错误契约。
2. 内部 Request/Cancel/Retry/NodeCompleted/NodeFailed/NodeSuspended/LeaseExpired 命令和 Runtime Event。
3. M3 已定义的 `ExecutionRuntime` 真实实现及 Coordinator 接入方式。
4. Platform API 的手动运行、取消、节点输入输出、Checkpoint 和 Wait 查询。
5. Platform API 的 Fork、whole/node/to-node/from-node 和副作用确认；手动失败重试通过 Fork 创建新 Execution，不修改原记录。
6. M3 已定义的 `ApprovalResumePort` 真实实现。

Wait Webhook/Form 使用 Trigger Gateway 的带认证 opaque Resume URL，Approval 通过内部 gRPC 命令恢复；Platform API 不提供通用 Resume Endpoint。Application、Evaluation 和生产 Trigger Node 不因 `ExecutionRuntime` 已存在而在 M4 自动接通；其业务对象与 Execution 的原子关联仍按 INT-001、INT-003、INT-004 在 M7 完成。

## 8. 测试与验收流程

### 8.1 快速门禁

`scripts/check.ps1` 继续作为每批基础门禁，并扩展覆盖：

- Definition、Node Protocol、Lineage、表达式、SCC/branchOrder/Readiness Compiler、Activation/Delivery 状态机和 Scheduler 单元/属性/Snapshot 测试。
- MySQL Migration、Repository 条件更新、事务回滚、tenant_id 和状态机集成测试。
- Redis 重复、乱序、Consumer 接管、Lease 过期、Outbox 补投和幂等测试。
- builtin/declarative_http/remote_action、Node Action 协议一致性、Engine 节点设置、超时、取消、迟到提交、日志脱敏和 Artifact/Credential Handle 授权测试。
- OpenAPI 与生成 TypeScript Client 漂移检查、Execution 前端组件测试和 Kustomize 渲染。

### 8.2 临时 Kubernetes E2E

`scripts/e2e.ps1` 必须在独立 `agentx-e2e` Namespace 中构建并部署 Coordinator、Worker 及 M4 Fixture，成功或失败后按现有规范保存证据并默认删除 Namespace。集群资源不足时可以先缩容 `agentx` Namespace，但不得复用其数据库、PVC 或登录状态。

M4 至少增加两个 Playwright 场景：

- `m4-runtime.spec.ts`：通过页面启动多来源、多分支顺序、required inputs、Merge、条件回边、Loop Over Items 和 Sub-workflow Fixture，查看节点输入输出/Lineage、Attempt、Trace 与 Checkpoint，执行取消和受控重试。
- `m4-recovery.spec.ts`：按时长/日期等待、运行时 Resume Webhook/Form、Approval Approve/Reject/Timeout、Checkpoint Fork、原记录不变、副作用确认以及重复 Resume 防护。

编排脚本额外执行故障注入和最终 MySQL 证据校验：

- 强制终止 Worker 后由 Lease/Reaper 恢复，节点只有一个有效完成结果。
- 重启 Coordinator 后从 MySQL 推进，不依赖丢失的进程内状态。
- Redis 短暂中断后 Outbox 最终补投，Execution 不产生重复有效 Attempt。
- ClickHouse 暂停时 Execution 仍完成，恢复后 Trace 最终可查询。
- Wait/Approval 期间 Worker Lease 数量为零；重复或迟到 Resume 不改变已恢复终态。
- Fork 前后原 Execution、Checkpoint 和 Trace 的 Hash/记录数保持不变。

## 9. M4 完成门禁

只有以下条件全部成立，阶段 08、阶段 09 和 M4 才能标记为 `done`：

1. RUN-001～014 与 REC-001～010 全部为 `done`，不存在跳过、合并为“后续处理”或仅人工验证的任务。
2. 非 AI JSON Fixture 覆盖成功、失败、多来源、分支顺序、分支关闭、required inputs、Merge、普通图环、Loop、Sub-workflow、重试、取消、超时、Wait、Approval 和 Fork。
3. MySQL 能独立解释每个 Execution 的当前状态、Activation/Delivery 前沿、有效 Attempt、下一步、等待原因和恢复来源。
4. Queue 至少一次、Lease、迟到提交、Outbox 和 Resume 幂等边界经过自动故障测试。
5. 原 Execution 在 Fork 后不变；不可逆副作用未经确认不会再次执行。
6. Execution/Trace 页面使用真实 API，能查看节点输入输出、Attempt、Checkpoint、Fork 和审批恢复。
7. Rust、前端、OpenAPI、Migration、Kustomize、临时 Kubernetes Playwright 和故障注入门禁全部通过。
8. 新增 `m4-acceptance-evidence.md` 记录命令、报告路径、关键数据库断言和故障恢复证据。
9. 阶段 08/09、路线图、总计划和功能追踪矩阵状态同步；M5 入口契约无阻塞项。

## 10. 剩余任务量

按阶段文档中的原子任务编号统计，不把批次门禁、验收文档和发现性缺陷重复计数。n8n 行为兼容分析重定义了 RUN-001～012 的交付边界，但没有创建重复任务编号；任务数量保持不变，不代表原 M4 工作量估算保持不变：

| 范围 | 原子任务数 | 当前状态 |
|---|---:|---|
| M4：RUN-001～014、REC-001～010 | 24 | done |
| M5：AGT-001～013 | 13 | done |
| M6：STU-001～016 | 16 | done |
| M7：INT-001～014 | 14 | planned |
| **当前至首期发布剩余** | **14** | M7 |

功能追踪矩阵中的未完成能力行和 MVP 步骤由剩余 14 个原子任务覆盖，不应再相加。实际实施中新增的缺陷修复或破坏性设计修正会作为所在阶段的补充任务单独登记。

## 11. 向 M5 提供的稳定输出

- 可版本化的 Node Registry/Manifest、Node Action/Lifecycle API、接入文档/一致性 Fixture、表达式和 Compiled IR。
- Execution/Activation/Attempt/Delivery 状态机、MySQL Runtime Repository、Coordinator、Worker、Queue 和 Lease。
- Execution Runtime 命令与事件、Trace/Artifact 接入、完整 Checkpoint 和 Fork。
- Wait/Resume、Approval Runtime 和 Side Effect Policy。
- 可供 Agent、MCP Tool、Skill、RAG、Memory 与 OpenSandbox Runner 复用的能力队列、运行上下文和故障恢复边界。
