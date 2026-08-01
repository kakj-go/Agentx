# 数据模型与存储分工

## 1. 存储职责

| 存储 | 职责 |
|---|---|
| MySQL | 租户、Workflow、版本、运行状态、审批、会话、测试评测元数据 |
| Redis | Queue、Lease、限流、短期缓存、SSE 临时事件 |
| ClickHouse | Trace Event、模型调用、工具调用、成本和时延明细 |
| MinIO/S3 | Binary、附件、大型节点输入输出、Checkpoint Payload、报告文件 |

Redis 和 ClickHouse 都不能代替 MySQL 中的权威 Execution 状态。

## 2. 租户与权限表

- tenants
- tenant_settings
- departments
- users
- roles
- permissions
- user_roles
- role_permissions
- resource_grants
- workflow_service_identities

核心索引应以 tenant_id 开头，防止跨租户查询和减少扫描。

## 3. Workflow 表

- workflows
- workflow_drafts
- workflow_draft_revisions
- workflow_versions
- workflow_deployments
- workflow_triggers
- node_definitions
- node_definition_versions

workflow_versions 保存：

- definition_json
- compiled_ir
- content_hash
- node_version_snapshot
- asset_reference_snapshot
- created_by
- created_at

## 4. Execution 表

- workflow_executions
- execution_snapshots
- node_executions
- node_attempts
- execution_edge_states
- execution_events
- execution_outbox
- checkpoints
- artifacts

workflow_executions 主要字段：

- tenant_id
- workflow_id
- workflow_version_id
- deployment_id
- application_id
- session_id
- execution_type
- status
- trigger_type
- parent_execution_id
- fork_checkpoint_id
- started_at
- finished_at
- total_tokens
- total_cost
- error_summary

node_executions 主要字段：

- execution_id
- node_id
- run_index
- branch_index
- iteration_index
- status
- input_ref
- output_ref
- started_at
- finished_at
- retry_count
- waiting_reason

node_attempts 主要字段：

- node_execution_id
- attempt_number
- worker_id
- lease_token
- lease_expires_at
- heartbeat_at
- sandbox_id
- error_code
- error_message

## 5. 资源表

- credentials
- model_providers
- model_deployments
- model_price_versions
- tools
- tool_versions
- rag_connections
- rag_resources
- memory_connections
- memory_namespaces
- workflow_resource_grants

Credential 表只保存加密数据或外部 Secret Reference，不向前端返回明文。

## 6. 应用和会话表

- applications
- application_deployments
- application_api_keys
- sessions
- messages
- message_parts
- application_invocations

Message Part 支持：

- text
- json
- image
- audio
- file
- tool_call
- tool_result

二进制内容保存到对象存储。

## 7. 审批和通知表

- approval_tasks
- approval_candidates
- approval_actions
- notifications

Approval Task 记录：

- execution_id
- node_execution_id
- assignee rule
- current status
- form schema
- context snapshot ref
- due_at
- resolved_at

## 8. 测试和评测表

- datasets
- dataset_versions
- test_cases
- evaluators
- evaluation_runs
- evaluation_case_results
- evaluation_metrics

Evaluation Case Result 需要引用对应的 Workflow Execution，便于直接跳转到 Trace。

## 9. ClickHouse Trace 表

首期统一存放 workflow_trace_events。

建议分区：

- 按月 event_time

建议排序键：

- tenant_id
- workflow_id
- event_time
- execution_id

常用查询字段应设置为独立列，变化频繁的扩展字段放 attributes_json。

后续查询量增大时，可以增加物化视图：

- workflow_execution_daily
- model_cost_daily
- tool_error_daily
- node_latency_daily

首期不需要提前建设所有聚合表。

## 10. Artifact

Artifact 用于统一引用：

- Binary Item
- 上传附件
- 大型 JSON
- 模型原始响应
- Tool 原始结果
- Sandbox 文件
- Checkpoint Payload
- Evaluation Report

Artifact 元数据在 MySQL，内容在对象存储。引用包含：

- artifact_id
- tenant_id
- content_type
- size
- hash
- storage_key
- created_by_execution

## 11. 数据保留

围绕 Workflow 提供简单策略：

- Execution 摘要保留时间
- Trace 保留时间
- Prompt 和 Response 是否完整保存
- Artifact 保留时间
- 会话消息保留时间
- 测试报告保留时间

清理时先检查 Checkpoint、报告和会话是否仍引用 Artifact。

