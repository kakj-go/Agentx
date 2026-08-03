# 阶段 09：恢复、等待和审批运行

## 1. 目标与用户价值

让 Execution 可以长期等待、审批后恢复，并从历史 Checkpoint 创建派生执行，同时保护不可逆副作用和原始审计历史。

## 2. 当前状态和进入条件

- 状态：`done`。验收证据见 [M4 验收证据](m4-acceptance-evidence.md)。
- 进入条件：[阶段 08](08-workflow-runtime-core.md) 的状态机、基础 Checkpoint、Coordinator 和 Worker 完成；[阶段 07](07-approvals-notifications-trace.md) 的 Approval Port 可用。

## 3. 范围和不做内容

实现完整 Checkpoint、Fork、Wait、Approval Resume、部分执行和副作用保护。不实现通用 BPMN、人事审批流程或跨 Workflow 分布式事务。

## 4. 领域对象、状态和不变量

- Checkpoint 是某个 Execution 状态点，不是可修改 Draft。
- Checkpoint 包含 Version、图位置、已完成 Node Activation、Edge Delivery Cursor、待满足输入集合、变量、输出引用、Artifact 和 State Hash。
- Fork 创建新 Execution，并记录 parent_execution_id 和 fork_checkpoint_id。
- 原 Execution、Node Execution、Trace 和 Checkpoint 不因 Fork 被修改。
- Wait/Approval 期间 Execution 进入持久化 Waiting 状态并释放 Worker，Node Action 以 `suspended` 结果提交平台拥有的 Resume Contract。
- 恢复事件使用稳定 resume_token 和幂等键，同一等待点只恢复一次。
- 部分执行必须从可满足依赖的 Checkpoint、Pin 或显式输入开始。

## 5. 数据和 Migration

扩展：

- checkpoints、checkpoint_artifacts、execution_snapshots
- execution_resume_tokens、wait_subscriptions、resume_webhook_bindings
- workflow_executions 的 parent_execution_id、fork_checkpoint_id 和 execution_type
- node_definition_versions 的 side_effect_level 和 resume_policy

小状态可保存在 MySQL；大型 Items、Agent State、文件和 Checkpoint Payload 写入对象存储。State Hash 覆盖所有恢复必需引用。

## 6. API、Port 和事件

- `POST /api/v1/executions/{id}/fork`
- `POST /api/v1/executions/{id}/cancel`
- `GET /api/v1/executions/{id}/checkpoints`
- `GET /api/v1/executions/{id}/waits`
- `POST /api/v1/executions/{id}/side-effect-confirmations`
- Fork 支持 whole、node、to-node 和 from-node 模式；Wait Webhook/Form 通过 Trigger Gateway 的调用绑定 opaque URL 恢复，不暴露通用 Platform Resume API。

内部 Port：`CheckpointStore`、`ExecutionForker`、`WaitSubscriptionStore`、`ApprovalResumePort` 和 `SideEffectPolicy`。

事件：`ExecutionWaiting`、`ApprovalRequested`、`ExecutionResumed`、`ExecutionForked`、`SideEffectConfirmationRequired`。

## 7. 服务和前端改动

- Coordinator 增加 Fork 初始化、Wait Subscription、Resume 和超时扫描。
- Worker 增加节点前后 Checkpoint、部分执行输入、副作用确认和 `suspended` 结果协议。
- Approval Node 创建 Task 后提交 Waiting 状态，不能占用 Worker 等待用户。
- Execution/Trace 页面增加 Checkpoint 时间线、Fork、重试和副作用确认。
- Workflow Studio 的具体画布入口留到阶段 11，但后端命令在本阶段稳定。

## 8. 实施任务

| 编号 | 状态 | 依赖 | 交付物 | 验收条件 |
|---|---|---|---|---|
| REC-001 | done | RUN-014、FND-005 | 完整 Checkpoint Schema、Payload 和 State Hash | 保存后可校验所有引用存在且 Hash 稳定 |
| REC-002 | done | REC-001 | Fork Execution 和父子追踪 | Fork 复用上游输出但拥有独立状态、Trace 和成本 |
| REC-003 | done | REC-001–002 | Execute Node、To Node、From Node 和输入覆盖语义 | 缺失依赖时拒绝执行并列出所需节点 |
| REC-004 | done | RUN-010、REC-001 | Wait Node、时间/指定日期、运行时 Resume Webhook/Form Contract、订阅、认证、超时和恢复 Token | 等待期间没有 Worker Lease；时间、Webhook 和表单 Fixture 可恢复；重复 Resume 只有一次生效 |
| REC-005 | done | OBS-001–002、REC-004 | Approval Node 创建 Task 和 Waiting 状态 | Task、Checkpoint 和 Waiting 状态原子可追溯 |
| REC-006 | done | OBS-007、REC-005 | Approval Resume、结果 Item 和输出端口 | Approve/Reject/Timeout 进入正确端口并重新入队 |
| REC-007 | done | RUN-002、REC-002–003 | 副作用等级和重执行策略 | Irreversible 节点必须确认、复用旧输出或 Dry Run |
| REC-008 | done | REC-001–007 | 超时、取消、清理和 Artifact 引用管理 | 等待取消后迟到事件不能恢复 Execution |
| REC-009 | done | REC-002–008 | REST API、OpenAPI、Trace 和通知事件 | Fork/Resume/确认动作全部具有 Actor 与审计记录 |
| REC-010 | done | REC-009、OBS-010 | Execution Checkpoint、Fork 和确认界面 | 用户能理解来源、风险、将复用和将重跑的节点 |

## 9. 失败、安全和幂等边界

- Checkpoint 保存失败时 Node 结果处理遵循事务边界，不能产生指向不存在 Payload 的有效 Checkpoint。
- Resume Token 只保存 Hash、限定等待点并具有过期时间。
- Resume Webhook URL 按 Execution 和等待点生成，限制 HTTP Method、认证、响应模式和最长等待时间；平台校验请求后再转换为幂等 Resume Event。
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
