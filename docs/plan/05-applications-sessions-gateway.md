# 阶段 05：应用、会话和调用入口

## 1. 目标与用户价值

让已发布 Workflow 可以配置为 Application，具备安全的 API Key、Session、Message、Webhook、SSE 和 Playground 协议，并为运行引擎保留唯一调用入口。

## 2. 当前状态和进入条件

- 状态：`done`。验收证据见 [M3 验收证据](m3-acceptance-evidence.md)。
- 进入条件：[阶段 03](03-workflow-control-plane.md) 的 Version/Deployment 和 [阶段 04](04-resource-center.md) 的发布校验完成。
- 引擎尚未接入时允许管理 Application 和会话元数据，但调用必须返回 `RUNTIME_UNAVAILABLE`。

## 3. 范围和不做内容

实现 Application 控制面、API Key、Session、Message、Gateway 认证、幂等和 SSE 协议。不实现 Workflow 调度、节点执行或独立 Playground Runner。

## 4. 领域对象、状态和不变量

- Application 状态为 `draft`、`active` 或 `disabled`，只能绑定同 Tenant 的 Workflow。
- Application Deployment 固定 Workflow Version、Environment、输入输出 Schema 和版本路由策略。
- API Key 只在创建时返回完整值，之后只显示前缀、名称、创建时间和最后使用时间。
- Session 创建时固定 Application Deployment 和 Workflow Version。
- 一条 User Message 对应一次 Application Invocation，运行接通后对应独立 Execution。
- Assistant Message 只能由已完成或流式中的 Invocation 产生，不接受客户端伪造。
- Idempotency Key 在 Application、调用主体和请求语义范围内唯一。

## 5. 数据和 Migration

主要表：

- applications、application_deployments
- application_api_keys、application_webhooks、application_schedules
- sessions、messages、message_parts
- application_invocations、invocation_idempotency_keys
- stream_sessions

Message Part 支持 text、json、image、audio、file、tool_call 和 tool_result；Binary 统一通过 Artifact Reference 保存。

## 6. REST API、Port 和事件

控制面：

- `/api/v1/applications`
- `/api/v1/applications/{id}/deployments`
- `/api/v1/applications/{id}/api-keys`
- `/api/v1/applications/{id}/webhooks|schedules`
- `/api/v1/applications/{id}/sessions`

调用面由 Trigger Gateway 提供版本化 Application API：创建 Session、发送 Message、查询 Invocation、取消、SSE Stream 和 Webhook Trigger。

定义 `ExecutionRuntime` Port：`request_execution`、`get_execution`、`cancel_execution`。阶段 08 前使用不可用实现并返回稳定错误，不写入伪 Execution。

业务事件：`ApplicationPublished`、`ApiKeyRotated`、`SessionCreated`、`MessageAccepted`、`InvocationRequested`、`InvocationCompleted`。

## 7. 后端和前端改动

- Platform API 负责 Application、Deployment、API Key、Session 和 Message 控制面。
- Trigger Gateway 负责 Application API、API Key 认证、输入 Schema、幂等、Webhook 和 SSE。
- API Key 与平台 JWT 使用不同认证中间件和权限域。
- `/applications` 增加详情、部署、API Key、Webhook 和 Session 页面。
- `/playground` 使用正式 Gateway Client，未接入运行时显示明确不可用状态。
- Message UI 支持结构化 Part 和流状态，不在浏览器持久化伪会话。

## 8. 实施任务

| 编号 | 状态 | 依赖 | 交付物 | 验收条件 |
|---|---|---|---|---|
| APP-001 | done | WCP-005、IAM-005 | Application、Deployment 和版本路由 Schema | 只能绑定同 Tenant 的 Active Workflow Version |
| APP-002 | done | APP-001 | API Key 生成、Hash、前缀、轮换和撤销 | 完整 Key 只返回一次，撤销后立即拒绝调用 |
| APP-003 | done | APP-001、FND-005 | Session、Message、Message Part 和 Artifact 引用 | Message 顺序稳定且 Binary 不进入大字段 |
| APP-004 | done | APP-001–003 | Invocation 和 Idempotency Repository | 并发相同 Key 只创建一条 Invocation |
| APP-005 | done | APP-004 | `ExecutionRuntime` Port 与 `RUNTIME_UNAVAILABLE` 实现 | 未接引擎时无伪 Execution、Message 或成功状态 |
| APP-006 | done | APP-002–005 | Trigger Gateway API Key 认证和输入 Schema 校验 | 无效 Key、禁用应用和非法输入在创建 Invocation 前失败 |
| APP-007 | done | APP-006 | SSE、异步查询、取消和 Webhook 协议 | 断线重连可按事件游标继续，不重复业务事件 |
| APP-008 | done | APP-001、APP-006 | Schedule 配置和触发命令外壳 | 时区和重复触发使用稳定幂等键 |
| APP-009 | done | APP-001–008 | 控制面与调用面 OpenAPI、审计事件 | 两类 API 认证边界在契约测试中分离 |
| APP-010 | done | APP-009、FND-011 | Application 详情、Key、Session 和 Message 页面 | Key 不可二次查看，页面支持撤销和轮换确认 |
| APP-011 | done | APP-006–010 | Playground 正式 Gateway Client | 未接引擎显示不可用；接入后无需更换调用路径 |

## 9. 失败、安全和幂等边界

- API Key 比较使用常量时间验证，Key 不进入日志和 Trace。
- Idempotency Key 复用但请求摘要不同返回冲突，不能静默复用旧结果。
- Session 已固定 Version 时，Deployment 切换不影响其后续消息。
- SSE 是事件读取通道，不是权威状态；断线后通过 Invocation 查询校准。
- Webhook 和 Schedule 重试使用同一触发幂等键。
- Application disabled 后拒绝新调用，历史 Session 和 Message 仍可按权限查询。

## 10. 测试

- API Key 创建、Hash、撤销、轮换和权限单元测试。
- Invocation 并发幂等、请求摘要冲突和 Session Version 固定集成测试。
- Gateway 输入 Schema、同步、异步、SSE 重连和取消契约测试。
- Playground 与正式 Gateway 路径一致性测试。
- 两租户 Application、Session 和 API Key 越权测试。

## 11. 验收门禁

- Application、Deployment、API Key、Session 和 Message 使用真实数据。
- API Key 只显示一次，相同幂等请求不产生重复 Invocation。
- Session 可以定位到固定 Workflow Version。
- Playground 只调用正式 Application API。
- 引擎未接入时明确失败且不产生伪业务记录。

## 12. 对后续阶段的稳定输出

- Application Invocation、Session 和 Message 模型。
- Trigger Gateway 认证、幂等和 SSE 协议。
- `ExecutionRuntime` Port 和运行接入点。
- Playground 的正式调用路径。
