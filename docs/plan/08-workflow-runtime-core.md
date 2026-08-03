# 阶段 08：Workflow 运行内核

## 1. 目标与用户价值

实现不依赖画布运行时状态的确定性 Workflow 编译与分布式执行，使 JSON Fixture 定义的非 AI Workflow 按 n8n 行为语义正确处理 Item、多来源追踪、分支顺序、多输入就绪、普通图环、Loop、重试和故障恢复。

## 2. 当前状态和进入条件

- 状态：`done`。验收证据见 [M4 验收证据](m4-acceptance-evidence.md)。
- 进入条件：阶段 01 的基础设施、阶段 03 的不可变 Workflow Version、阶段 07 的 Execution/Trace 查询结构完成。
- 运行语义以 [Workflow 引擎设计](../03-workflow-engine.md) 为依据，先通过 JSON Fixture 固化，不依赖 React Flow。

## 3. 范围和不做内容

实现 Definition、Node Registry/Manifest、Node Action API、Item、表达式、Compiler、IR、固定版本 Sub-workflow、Scheduler、Coordinator、Worker、Redis Stream、Lease、Retry 和基础 Checkpoint。不实现 Agent Tool Loop、CubeSandbox、完整 Wait/Approval 恢复或 Workflow Studio；Trigger/Poll/Webhook Lifecycle 和动态 UI Provider 在本阶段冻结协议，真实 Trigger Gateway 接通属于阶段 12，完整节点配置 UI 属于阶段 11。

## 4. 领域对象、状态和不变量

- Workflow Definition 由 Nodes、Connections、Variables、Settings、Triggers、执行顺序、激活预算和错误策略组成。
- main 连接允许回边；Compiler 计算 SCC、回边和循环边界，Workflow 级最大激活次数、超时和取消策略防止无限循环。
- Item 包含 json、binary Artifact、零到多个 pairedItems 和运行 Metadata。
- Node Execution 表示一次逻辑激活，以 node_execution_id 为稳定身份，并包含 execution_id、node_id、run_index、activation_sequence 和可选 loop_iteration_index。
- Retry 创建新的 Node Attempt，不增加 run_index。
- Sub-workflow 必须引用不可变 Workflow Version，编译阶段拒绝版本依赖环和超过限制的递归深度。
- Execution、Node Execution、Attempt 和 Edge Delivery 只能按状态机条件迁移。
- Edge Delivery 按 source activation 追加，同一 Edge 在循环中可多次交付；未命中分支为对应 activation 产生 `ClosedWithoutData`，Join 不能永久等待已关闭分支。
- 节点 Ready 判定来自 Node Manifest 的 Readiness Policy，不把多输入规则硬编码到 Merge。
- 默认 `n8n_v1` 按固化 branchOrder 串行推进分支；显式 parallel 模式才允许并行可观察分支。
- Scheduler 决策来自 MySQL 状态和编译 IR，不依赖 Coordinator 内存。

## 5. 数据和 Migration

主要表：

- node_definitions、node_definition_versions
- workflow_executions、execution_snapshots
- node_executions、node_attempts
- execution_edge_deliveries、item_lineage、execution_events、execution_outbox
- checkpoints、artifacts
- worker_leases、runtime_idempotency_keys

大 Item 和 Binary 使用 Artifact Reference。Execution Snapshot 固化 Workflow Version、Compiled IR、资源版本和运行设置，避免执行期间读取变化中的控制面配置。

## 6. Definition、IR、命令和事件

- `WorkflowDefinition` 与 React Flow State、数据库实体和 n8n JSON 分离。
- `NodeManifestVersion` 固化参数 JSON Schema、富 UI Schema、动态 Provider、端口、Readiness Policy、Execution Style、执行模式、Credential、超时、重试、副作用和沙箱要求。
- `CompiledWorkflow` 预计算入边、出边、SCC/回边、branchOrder、Readiness、Join、分支关闭传播、Loop、Error Branch、资源依赖和 Checkpoint 边界。
- `NodeActionExecution` 固化分组输入、按 Item 解析参数、Lineage、Artifact/Credential Handle、幂等键、Deadline、Trace Context 以及 `completed | failed | suspended` 结果。
- `NodeLifecycle` 固化 activate、deactivate、poll、webhook、suspend 和 resume；本阶段只实现 Action 与基础 Suspend Adapter。
- 运行命令包括 Request、Cancel、Retry、NodeCompleted、NodeFailed、NodeSuspended 和 LeaseExpired。
- Queue Message 只携带稳定 ID、Attempt、Lease 要求和能力标签，不携带权威状态。
- Runtime Event 通过 Outbox 发送 Trace、Notification 和外围投影。

## 7. 服务和前端边界

- `agentx-runtime` 实现纯编译、SCC/图结构、Activation/Delivery 状态推进和内存 Scheduler，不依赖 SQLx 或 Redis。
- `agentx-node-protocol` 固化 Node Manifest、Item/Lineage、Action/Lifecycle、Artifact、Credential Handle 和错误 DTO，不提供公共语言 SDK。
- Workflow Coordinator 负责创建 Execution、按 branchOrder/Readiness 推进 Activation、Join/Loop、取消和 Reaper。
- Workflow Worker 负责领取任务、Lease、平台侧表达式求值、builtin/declarative_http/remote_action Adapter、结果、基础 Checkpoint 和 Trace Event。
- Infrastructure 实现 MySQL Runtime Repository、Redis Queue 和 Outbox Dispatcher。
- Execution 页面接入真实执行状态；画布仍不作为本阶段验收入口。

## 8. 实施任务

| 编号 | 状态 | 依赖 | 交付物 | 验收条件 |
|---|---|---|---|---|
| RUN-001 | done | WCP-003–004 | Workflow Definition JSON Schema、n8n 行为兼容边界和 Fixture 集 | Schema 覆盖执行顺序、节点设置、端口、激活预算、错误策略和版本 |
| RUN-002 | done | RUN-001 | Node Registry/Manifest Version、Readiness、Action/Lifecycle API 和兼容规则 | 未知版本稳定失败；Manifest 能表达多输入、动态 UI、Action、Trigger 和 Suspend 能力 |
| RUN-003 | done | RUN-001–002 | `agentx-node-protocol`、Item/Binary、零到多来源 Lineage、Artifact/Credential Handle 和协议一致性 Fixture | Merge/聚合输出可追溯多个上游 run/output/item，旧单来源和 `agentx-node-sdk` 协议被清理 |
| RUN-004 | done | RUN-003 | 受限表达式 Lexer、Parser、AST、Validator、Interpreter 和按 Item 参数解析 | linked item、all(branch, run)、当前 item/run 和公共/逐 Item 参数正确；任意 JavaScript、Secret 外泄和越界访问被拒绝 |
| RUN-005 | done | RUN-001–004 | Compiler、SCC/回边、branchOrder、Readiness IR、Sub-workflow 依赖校验和 Canonical Hash | 普通受控图环可编译；端口、表达式、资源、递归子版本和不可达节点可解释失败；相同 Definition 产生相同顺序和 Hash |
| RUN-006 | done | RUN-005 | 内存版 Execution/Activation/Attempt/Delivery 状态机 | 同一 Edge 可按 activation 多次交付；非法迁移不能提交，终态不可回退 |
| RUN-007 | done | RUN-006 | IF、Switch、branchOrder 和按 Activation 关闭分支传播 | `n8n_v1` 分支顺序稳定，未命中端口形成本轮 ClosedWithoutData 并正确推进下游 |
| RUN-008 | done | RUN-006–007 | Readiness 策略、Merge、普通图环、Loop Over Items 和固定版本 Sub-workflow | required inputs/any/all、Merge、条件回边、批次 Loop、激活上限和子版本边界通过 Fixture |
| RUN-009 | done | RUN-005–008 | MySQL Runtime Repository、Activation/Delivery/Lineage 和 Execution Snapshot | 创建执行固化 Version/IR/资源；循环 Delivery 可恢复且 MySQL 能解释下一激活 |
| RUN-010 | done | RUN-009、FND-006 | Coordinator、branchOrder/Readiness 判定和 Transactional Outbox | 分支顺序、多输入 generation 和待投递任务在事务中一致推进 |
| RUN-011 | done | RUN-010 | Redis Streams、能力队列和 Dispatcher | 重复投递不创建重复有效 Attempt |
| RUN-012 | done | RUN-003–004、RUN-011 | Worker、builtin/declarative_http/remote_action Adapter、Node Action API 和结果保存 | Worker 只在有效 Lease 下执行；远程请求具备幂等、Deadline、Trace、Credential Handle 和 completed/failed/suspended 契约 |
| RUN-013 | done | RUN-010–012 | Lease、Heartbeat、Reaper、Retry、Timeout 和 Cancel | Worker 崩溃、超时和重复消息场景可恢复 |
| RUN-014 | done | RUN-009–013 | 基础 Checkpoint、Execution API 和 Trace 接入 | 每个成功节点结果可查询并形成逻辑恢复点 |

## 9. 失败、幂等和安全边界

- Queue 采用至少一次语义；Node Attempt 提交使用状态条件和 Lease Token 防止迟到 Worker 覆盖新结果。
- 外部副作用没有通用 Exactly Once，Runner 必须接收稳定幂等键和副作用等级。
- Retry 只针对配置允许的错误；不可逆副作用默认不自动重试。
- Redis 不可用时 Outbox 保留；ClickHouse 不可用不回滚 Node 完成。
- Cancel 是持久化状态，Worker 在领取、Heartbeat 和提交前检查。
- 表达式不能读取未声明 Secret，输出进入日志和 Trace 前继续脱敏。

## 10. 测试

- Definition、Node Manifest、协议兼容、Item 多来源和表达式 AST 单元/属性测试。
- 图编译 Snapshot 覆盖 SCC/回边、branchOrder、关闭分支、Readiness、Merge、普通图环、Loop 和 Sub-workflow 依赖。
- 内存 Scheduler 确定性测试验证相同输入产生相同激活、Delivery 和 `n8n_v1` 分支顺序。
- MySQL/Redis 集成测试：重复消息、乱序、Lease 过期、迟到提交和事务回滚。
- Worker 强制终止、Coordinator 重启、Redis 短暂中断和 Trace 延迟故障测试。
- Execution API、取消、重试和节点输入输出端到端测试。

## 11. 验收门禁

- 多分支、required inputs、Merge、普通图环、Loop Over Items 和固定版本 Sub-workflow JSON Fixture 全部正确执行。
- 多来源 Item 能按 node run/output/item 回溯；ClosedWithoutData 只关闭对应 activation 并避免 Join 永久等待。
- 默认分支副作用顺序与固化的 `n8n_v1` branchOrder 一致，未显式 parallel 时不被 Worker 并行改变。
- 重复消息、Worker 崩溃和 Lease 过期不会无条件重复完成节点。
- MySQL 始终能解释 Execution 当前状态和下一步。
- 节点输入、输出、Attempt、错误和基础 Checkpoint 可查询。

## 12. 对后续阶段的稳定输出

- Workflow Definition、Node Manifest、Node Action/Lifecycle API、Compiled IR 和接入文档/一致性 Fixture。
- Execution/Activation/Attempt/Delivery 状态机与 Runtime Repository。
- Coordinator、Worker、Queue、Lease 和基础 Checkpoint。
- `ExecutionRuntime` 的真实实现和标准 Runtime Event。
