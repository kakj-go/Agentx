# 阶段 07：审批、消息与运行查询

## 1. 目标与用户价值

提前完成审批任务、站内消息、Execution 摘要、Trace 写入与查询结构，使运行引擎后续只需产生标准事件即可接通外围页面。

## 2. 当前状态和进入条件

- 状态：`done`。验收证据见 [M3 验收证据](m3-acceptance-evidence.md)。
- 进入条件：[阶段 02](02-bootstrap-auth-iam.md) 提供候选人与权限，[阶段 01](01-contracts-and-foundation.md) 提供 Outbox、Artifact 和 ClickHouse Client。
- Production API 不提供人工创建伪审批或伪 Trace 的入口；测试通过内部 Port 和 Fixture 验证。

## 3. 范围和不做内容

实现 Approval Task、Notification、Trace Writer、Execution/Trace 查询和简要运行状态。不实现通用 OA、通用告警平台、Prometheus/Grafana 或审批节点恢复调度。

## 4. 领域对象、状态和不变量

- Approval Task 状态为 `pending`、`claimed`、`approved`、`rejected`、`cancelled` 或 `timed_out`。
- Approval Action 追加写入，已完成任务不能再次产生终态动作。
- Candidate 可以由用户、角色或部门规则产生，执行动作时重新校验资格。
- Notification 是业务事件投影，已读状态不改变源业务对象。
- Execution Summary 在 MySQL，Trace Event 在 ClickHouse，Artifact 内容在对象存储。
- Trace、Checkpoint 和 Audit 是独立对象，不能用 Trace 恢复状态或替代审计。

## 5. 数据和 Migration

MySQL：

- approval_tasks、approval_candidates、approval_actions
- notifications、notification_receipts
- workflow_executions 摘要字段和 execution_events 查询投影
- trace_delivery_outbox、trace_delivery_offsets

ClickHouse：

- `workflow_trace_events`，按月分区并以 tenant_id、workflow_id、event_time、execution_id 排序。

大段 Prompt、Response、Tool Result 和文件使用 content_ref 指向 Artifact。

## 6. REST API、Port 和事件

- `/api/v1/approvals`、`/{id}/claim|approve|reject|cancel`
- `/api/v1/notifications`、`/{id}/read`、`/read-all`
- `/api/v1/executions`、`/{id}`、`/{id}/trace`
- `/api/v1/traces/{traceId}` 和 Artifact 受权访问接口
- `/api/v1/runtime/status`

定义 `ApprovalTaskPort`、`ApprovalResumePort`、`NotificationPublisher`、`TraceSink` 和 `ExecutionQuery`。阶段 09 前 `ApprovalResumePort` 对真实恢复返回不可用，但审批动作和审计可正常完成。

## 7. 后端和前端改动

- Platform API 增加 approvals、notifications、execution-query、trace-query 和 runtime-status 模块。
- Trace Writer 从专用 Stream 批量读取并追加到 ClickHouse，维护消费 Offset 和重试。
- `/approvals`、`/executions` 使用真实 API，并新增 Approval/Execution/Trace 详情。
- Header 通知下拉改用真实 Notification Query，并提供消息中心。
- Trace 页面展示 Span 树、runIndex、iterationIndex、输入输出引用、模型、Tool、成本、错误和重试。
- 运行状态只展示 Worker、Queue、Running/Waiting/Failed、Sandbox 和 Trace Queue，不扩展成通用监控。

## 8. 实施任务

| 编号 | 状态 | 依赖 | 交付物 | 验收条件 |
|---|---|---|---|---|
| OBS-001 | done | IAM-004–005、FND-003 | Approval Task、Candidate、Action Schema 和状态机 | 并发终态动作只有一个成功，动作追加不可改 |
| OBS-002 | done | OBS-001 | Claim、Approve、Reject、Cancel、Timeout 用例和审计 | 动作时重新校验候选资格和 Tenant |
| OBS-003 | done | FND-006、IAM-005 | Notification 投影、已读状态和业务链接 | 重复业务事件不产生重复通知 |
| OBS-004 | done | FND-003 | Execution Summary 和查询 Repository | 列表不扫描 ClickHouse 且支持租户/Workflow/状态过滤 |
| OBS-005 | done | FND-004–006 | Trace Event Schema、Queue 和 Trace Writer | 批量重复写入按事件 ID 去重或保持查询幂等 |
| OBS-006 | done | OBS-005、FND-005 | Trace Query、Span 树和 Artifact 授权 | 父子 Span、Node Attempt 和内容引用可还原 |
| OBS-007 | done | OBS-001–002 | `ApprovalTaskPort` 和 `ApprovalResumePort` | 引擎未接入时审批可记录但恢复明确不可用 |
| OBS-008 | done | OBS-004–006 | Runtime Status Query | Redis/服务健康数据缺失时显示 unknown 而非伪零值 |
| OBS-009 | done | OBS-001–008 | REST API、OpenAPI 和权限检查 | 契约测试覆盖并发审批、Trace 游标和 Artifact 越权 |
| OBS-010 | done | OBS-009、FND-011 | Approval、Notification、Execution 和 Trace 页面 | Mock 移除，业务跳转与权限不足状态正确 |
| OBS-011 | done | OBS-008–010 | 简要运行状态页面 | 不依赖 Prometheus/Grafana，查询失败不影响 Workflow API |

## 9. 失败、安全和幂等边界

- 审批终态通过条件更新和唯一约束保证一次完成。
- Notification 投影允许重建，源业务事件保持权威。
- ClickHouse 不可用时 Trace 留在队列并重试，不能回滚已完成节点事务。
- Trace 写入前脱敏 Secret、Cookie、Authorization 和 Credential。
- Artifact 下载必须校验 Tenant、Workflow 和 Execution 数据范围，不能仅凭对象 Key。
- Runtime Status 只是观察信息，不参与 Scheduler 状态判断。

## 10. 测试

- Approval 状态机、候选规则和并发终态单元/集成测试。
- Notification 重放、已读和业务链接测试。
- Trace Writer 批次、重复、ClickHouse 中断和恢复测试。
- Trace Span 树、游标分页、脱敏和 Artifact 权限测试。
- Approval、Notification、Execution、Trace 页面与跨租户端到端测试。

## 11. 验收门禁

- 审批动作可审计且并发安全。
- Notification 能跳转源业务对象，重复事件不会重复通知。
- Execution 列表使用 MySQL 摘要，Trace 明细通过 ClickHouse 查询。
- ClickHouse 故障不改变权威业务状态。
- 审批、通知、执行和 Trace 页面不再使用 Mock。

## 12. 对后续阶段的稳定输出

- Approval Task、Action 和 Resume 接入点。
- Notification 投影协议。
- Execution Summary、Trace Event、Trace Writer 和查询 API。
- 运行状态的有限业务边界。
