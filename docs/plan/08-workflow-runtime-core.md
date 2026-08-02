# 阶段 08：Workflow 运行内核

## 1. 目标与用户价值

实现不依赖画布的确定性 Workflow 编译与分布式执行，使 JSON Fixture 定义的非 AI Workflow 能正确处理 Item、分支、Merge、Loop、重试和故障恢复。

## 2. 当前状态和进入条件

- 状态：`planned`。
- 进入条件：阶段 01 的基础设施、阶段 03 的不可变 Workflow Version、阶段 07 的 Execution/Trace 查询结构完成。
- 运行语义以 [Workflow 引擎设计](../03-workflow-engine.md) 为依据，先通过 JSON Fixture 固化，不依赖 React Flow。

## 3. 范围和不做内容

实现 Definition、Node Registry、Item、表达式、Compiler、IR、Scheduler、Coordinator、Worker、Redis Stream、Lease、Retry 和基础 Checkpoint。不实现 Agent Tool Loop、CubeSandbox、完整 Wait/Approval 恢复或 Workflow Studio。

## 4. 领域对象、状态和不变量

- Workflow Definition 由 Nodes、Connections、Variables、Settings、Triggers 和错误策略组成。
- 一般图环非法，只允许显式 Loop 节点表达循环。
- Item 包含 json、binary Artifact、pairedItem 和运行 Metadata。
- Node Execution 唯一维度包含 execution_id、node_id、run_index、branch_index 和 iteration_index。
- Retry 创建新的 Node Attempt，不增加 run_index。
- Execution、Node Execution 和 Edge State 只能按状态机条件迁移。
- 未命中分支必须产生 `ClosedWithoutData`，Join 不能永久等待已关闭分支。
- Scheduler 决策来自 MySQL 状态和编译 IR，不依赖 Coordinator 内存。

## 5. 数据和 Migration

主要表：

- node_definitions、node_definition_versions
- workflow_executions、execution_snapshots
- node_executions、node_attempts
- execution_edge_states、execution_events、execution_outbox
- checkpoints、artifacts
- worker_leases、runtime_idempotency_keys

大 Item 和 Binary 使用 Artifact Reference。Execution Snapshot 固化 Workflow Version、Compiled IR、资源版本和运行设置，避免执行期间读取变化中的控制面配置。

## 6. Definition、IR、命令和事件

- `WorkflowDefinition` 与 React Flow State、数据库实体和 n8n JSON 分离。
- `NodeDefinitionVersion` 固化参数 JSON Schema、UI Schema、端口、Runner、执行模式、超时、重试、副作用和沙箱要求。
- `CompiledWorkflow` 预计算入边、出边、依赖、Join、分支关闭传播、Loop、Error Branch、资源依赖和 Checkpoint 边界。
- 运行命令包括 Request、Cancel、Retry、NodeCompleted、NodeFailed 和 LeaseExpired。
- Queue Message 只携带稳定 ID、Attempt、Lease 要求和能力标签，不携带权威状态。
- Runtime Event 通过 Outbox 发送 Trace、Notification 和外围投影。

## 7. 服务和前端边界

- `agentx-runtime` 实现纯编译、图结构、状态推进和内存 Scheduler，不依赖 SQLx 或 Redis。
- `agentx-node-sdk` 固化 Item、NodeContext、NodeRunner、NodeError 和结果协议。
- Workflow Coordinator 负责创建 Execution、推进状态、Join/Loop、取消和 Reaper。
- Workflow Worker 负责领取任务、Lease、表达式、Runner、结果、基础 Checkpoint 和 Trace Event。
- Infrastructure 实现 MySQL Runtime Repository、Redis Queue 和 Outbox Dispatcher。
- Execution 页面接入真实执行状态；画布仍不作为本阶段验收入口。

## 8. 实施任务

| 编号 | 状态 | 依赖 | 交付物 | 验收条件 |
|---|---|---|---|---|
| RUN-001 | planned | WCP-003–004 | Workflow Definition JSON Schema 和 Fixture 集 | Schema 覆盖基础节点、端口、设置、错误策略和版本 |
| RUN-002 | planned | RUN-001 | Node Registry、Definition Version 和兼容规则 | 缺失或不兼容节点版本编译失败且原因明确 |
| RUN-003 | planned | RUN-001–002 | Item、Binary、Paired Item 和 Node SDK | 多输入 Item 来源可追溯到准确上游索引 |
| RUN-004 | planned | RUN-003 | 受限表达式 Lexer、Parser、AST、Validator 和 Interpreter | 任意 JavaScript、Secret 外泄和越界对象访问被拒绝 |
| RUN-005 | planned | RUN-001–004 | Compiler、IR、图校验和 Canonical Hash | 非法环、端口、表达式、资源和不可达节点可解释失败 |
| RUN-006 | planned | RUN-005 | 内存版 Execution/Node/Edge 状态机 | 非法状态迁移不能提交，终态不可回退 |
| RUN-007 | planned | RUN-006 | IF、Switch 和关闭分支传播 | 未命中端口形成 ClosedWithoutData 并正确推进 Join |
| RUN-008 | planned | RUN-006–007 | Merge 策略和显式 Loop | Wait All/Any、Append、By Position/Key 与循环边界通过 Fixture |
| RUN-009 | planned | RUN-005–008 | MySQL Runtime Repository 和 Execution Snapshot | 创建执行在事务内固化 Version、IR 和资源快照 |
| RUN-010 | planned | RUN-009、FND-006 | Coordinator、Ready 判定和 Transactional Outbox | 状态变化与待投递任务原子提交 |
| RUN-011 | planned | RUN-010 | Redis Streams、能力队列和 Dispatcher | 重复投递不创建重复有效 Attempt |
| RUN-012 | planned | RUN-003–004、RUN-011 | Worker、Native Runner、HTTP Runner 和结果保存 | Worker 只在取得有效 Lease 后执行和提交结果 |
| RUN-013 | planned | RUN-010–012 | Lease、Heartbeat、Reaper、Retry、Timeout 和 Cancel | Worker 崩溃、超时和重复消息场景可恢复 |
| RUN-014 | planned | RUN-009–013 | 基础 Checkpoint、Execution API 和 Trace 接入 | 每个成功节点结果可查询并形成逻辑恢复点 |

## 9. 失败、幂等和安全边界

- Queue 采用至少一次语义；Node Attempt 提交使用状态条件和 Lease Token 防止迟到 Worker 覆盖新结果。
- 外部副作用没有通用 Exactly Once，Runner 必须接收稳定幂等键和副作用等级。
- Retry 只针对配置允许的错误；不可逆副作用默认不自动重试。
- Redis 不可用时 Outbox 保留；ClickHouse 不可用不回滚 Node 完成。
- Cancel 是持久化状态，Worker 在领取、Heartbeat 和提交前检查。
- 表达式不能读取未声明 Secret，输出进入日志和 Trace 前继续脱敏。

## 10. 测试

- Definition、Node Version、Item 来源和表达式 AST 单元/属性测试。
- 图编译 Snapshot，覆盖非法环、关闭分支、Merge 和 Loop。
- 内存 Scheduler 确定性测试，相同输入产生相同调度决策。
- MySQL/Redis 集成测试：重复消息、乱序、Lease 过期、迟到提交和事务回滚。
- Worker 强制终止、Coordinator 重启、Redis 短暂中断和 Trace 延迟故障测试。
- Execution API、取消、重试和节点输入输出端到端测试。

## 11. 验收门禁

- 多分支、Merge 和 Loop JSON Fixture 全部正确执行。
- ClosedWithoutData 能终止未命中路径并避免 Join 永久等待。
- 重复消息、Worker 崩溃和 Lease 过期不会无条件重复完成节点。
- MySQL 始终能解释 Execution 当前状态和下一步。
- 节点输入、输出、Attempt、错误和基础 Checkpoint 可查询。

## 12. 对后续阶段的稳定输出

- Workflow Definition、Compiled IR 和 Node SDK。
- Execution/Node/Edge 状态机与 Runtime Repository。
- Coordinator、Worker、Queue、Lease 和基础 Checkpoint。
- `ExecutionRuntime` 的真实实现和标准 Runtime Event。

