# 阶段 09：恢复、等待和审批运行

## 1. 目标与用户价值

让 Execution 可以长期等待、审批后恢复，并从历史 Checkpoint 创建派生执行，同时保护不可逆副作用和原始审计历史。

## 2. 当前状态和进入条件

- 状态：`planned`。
- 进入条件：[阶段 08](08-workflow-runtime-core.md) 的状态机、基础 Checkpoint、Coordinator 和 Worker 完成；[阶段 07](07-approvals-notifications-trace.md) 的 Approval Port 可用。

## 3. 范围和不做内容

实现完整 Checkpoint、Fork、Wait、Approval Resume、部分执行和副作用保护。不实现通用 BPMN、人事审批流程或跨 Workflow 分布式事务。

## 4. 领域对象、状态和不变量

- Checkpoint 是某个 Execution 状态点，不是可修改 Draft。
- Checkpoint 包含 Version、图位置、已完成节点、Edge State、变量、输出引用、Artifact 和 State Hash。
- Fork 创建新 Execution，并记录 parent_execution_id 和 fork_checkpoint_id。
- 原 Execution、Node Execution、Trace 和 Checkpoint 不因 Fork 被修改。
- Wait/Approval 期间 Execution 进入持久化 Waiting 状态并释放 Worker。
- 恢复事件使用稳定 resume_token 和幂等键，同一等待点只恢复一次。
- 部分执行必须从可满足依赖的 Checkpoint、Pin 或显式输入开始。

## 5. 数据和 Migration

扩展：

- checkpoints、checkpoint_artifacts、execution_snapshots
- execution_resume_tokens、wait_subscriptions
- workflow_executions 的 parent_execution_id、fork_checkpoint_id 和 execution_type
- node_definitions 的 side_effect_level 和 resume_policy

小状态可保存在 MySQL；大型 Items、Agent State、文件和 Checkpoint Payload 写入对象存储。State Hash 覆盖所有恢复必需引用。

## 6. API、Port 和事件

- `POST /api/v1/executions/{id}/fork`
- `POST /api/v1/executions/{id}/cancel`
- `POST /api/v1/executions/{id}/resume`
- 手动运行命令支持 whole、node、to-node 和 from-node 模式。

内部 Port：`CheckpointStore`、`ExecutionForker`、`WaitSubscriptionStore`、`ApprovalResumePort` 和 `SideEffectPolicy`。

事件：`ExecutionWaiting`、`ApprovalRequested`、`ExecutionResumed`、`ExecutionForked`、`SideEffectConfirmationRequired`。

## 7. 服务和前端改动

- Coordinator 增加 Fork 初始化、Wait Subscription、Resume 和超时扫描。
- Worker 增加节点前后 Checkpoint、部分执行输入和副作用确认协议。
- Approval Node 创建 Task 后提交 Waiting 状态，不能占用 Worker 等待用户。
- Execution/Trace 页面增加 Checkpoint 时间线、Fork、重试和副作用确认。
- Workflow Studio 的具体画布入口留到阶段 11，但后端命令在本阶段稳定。

## 8. 实施任务

| 编号 | 状态 | 依赖 | 交付物 | 验收条件 |
|---|---|---|---|---|
| REC-001 | planned | RUN-014、FND-005 | 完整 Checkpoint Schema、Payload 和 State Hash | 保存后可校验所有引用存在且 Hash 稳定 |
| REC-002 | planned | REC-001 | Fork Execution 和父子追踪 | Fork 复用上游输出但拥有独立状态、Trace 和成本 |
| REC-003 | planned | REC-001–002 | Execute Node、To Node、From Node 和输入覆盖语义 | 缺失依赖时拒绝执行并列出所需节点 |
| REC-004 | planned | RUN-010、REC-001 | Wait Node、订阅、超时和恢复 Token | 等待期间没有 Worker Lease，重复 Resume 只有一次生效 |
| REC-005 | planned | OBS-001–002、REC-004 | Approval Node 创建 Task 和 Waiting 状态 | Task、Checkpoint 和 Waiting 状态原子可追溯 |
| REC-006 | planned | OBS-007、REC-005 | Approval Resume、结果 Item 和输出端口 | Approve/Reject/Timeout 进入正确端口并重新入队 |
| REC-007 | planned | RUN-002、REC-002–003 | 副作用等级和重执行策略 | Irreversible 节点必须确认、复用旧输出或 Dry Run |
| REC-008 | planned | REC-001–007 | 超时、取消、清理和 Artifact 引用管理 | 等待取消后迟到事件不能恢复 Execution |
| REC-009 | planned | REC-002–008 | REST API、OpenAPI、Trace 和通知事件 | Fork/Resume/确认动作全部具有 Actor 与审计记录 |
| REC-010 | planned | REC-009、OBS-010 | Execution Checkpoint、Fork 和确认界面 | 用户能理解来源、风险、将复用和将重跑的节点 |

## 9. 失败、安全和幂等边界

- Checkpoint 保存失败时 Node 结果处理遵循事务边界，不能产生指向不存在 Payload 的有效 Checkpoint。
- Resume Token 只保存 Hash、限定等待点并具有过期时间。
- 迟到的审批、Webhook 或 Timeout Event 通过状态条件忽略并记录审计。
- 使用兼容新 Draft Fork 时必须重新编译并验证节点映射、资源授权和 Schema。
- 复用旧输出不代表重新执行副作用；UI 必须清楚区分。
- Fork 权限同时检查原 Execution、目标 Workflow/Draft 和所有资源。

## 10. 测试

- Checkpoint State Hash、Payload 引用和恢复一致性测试。
- Fork 原记录不变、父子追踪和成本独立测试。
- Wait/Resume 并发、重复事件、取消和超时集成测试。
- Approval 多 Coordinator 恢复和 Worker 零占用测试。
- 不同副作用等级的重跑确认、Dry Run 和旧输出复用测试。
- Checkpoint/Fork UI 风险提示和权限端到端测试。

## 11. 验收门禁

- 任意可恢复节点可以创建 Fork，原 Execution 不变。
- Wait 和 Approval 长期等待不占用 Worker，服务重启后仍可恢复。
- 重复或迟到 Resume 不会推进两次。
- Irreversible 节点不会被无确认重复执行。
- Checkpoint、Fork 和审批恢复在 Trace 与审计中完整关联。

## 12. 对后续阶段的稳定输出

- 完整 Checkpoint、Fork 和部分执行协议。
- Wait/Resume 与 Approval Runtime 实现。
- Side Effect Policy 和人工确认边界。
- Studio 可直接调用的调试与恢复 API。

