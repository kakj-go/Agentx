# 历史 V1 MySQL Schema Catalog

该目录是 V2-00 逐表处置使用的历史 V1 输入，由当时的 `migrations/mysql/0001～0025` 在 MySQL 8.4 空库顺序执行后从 `information_schema` 导出。V2-08A 已删除该共享 Migration；当前 V2 Schema 以 `migrations/control`、`migrations/runtime` 和 `migrations/observability` 为准。

- 业务表：132
- SQLx 元数据表：1
- 总计：133

### _sqlx_migrations

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| version | bigint | NO | ∅ |  |
| description | text | NO | ∅ |  |
| installed_on | timestamp | NO | CURRENT_TIMESTAMP | DEFAULT_GENERATED |
| success | tinyint(1) | NO | ∅ |  |
| checksum | blob | NO | ∅ |  |
| execution_time | bigint | NO | ∅ |  |

| Index | Unique | Columns |
|---|---:|---|
|  | no |  |
| R | no | M |
|  | no |  |
| e | no | s |

| Foreign key | Columns | References |
|---|---|---|
| — | — | — |

### agent_iterations

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| agent_run_id | binary(16) | NO | ∅ |  |
| iteration_index | int unsigned | NO | ∅ |  |
| status | enum('running','completed','failed','cancelled') | NO | running |  |
| state_before_hash | char(64) | NO | ∅ |  |
| state_after_hash | char(64) | YES | ∅ |  |
| state_artifact_id | binary(16) | YES | ∅ |  |
| stop_reason | varchar(64) | YES | ∅ |  |
| started_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |
| ended_at | timestamp(6) | YES | ∅ |  |

| Index | Unique | Columns |
|---|---:|---|
| fk_agent_iteration_state | no | state_artifact_id |
| fk_agent_iteration_tenant | no | tenant_id |
| PRIMARY | yes | id |
| uq_agent_iteration | yes | agent_run_id, iteration_index |

| Foreign key | Columns | References |
|---|---|---|
| fk_agent_iteration_run | agent_run_id | agent_runs.id |
| fk_agent_iteration_state | state_artifact_id | artifacts.id |
| fk_agent_iteration_tenant | tenant_id | tenants.id |

### agent_runs

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| execution_id | binary(16) | NO | ∅ |  |
| node_execution_id | binary(16) | NO | ∅ |  |
| status | enum('running','succeeded','failed','cancelled') | NO | running |  |
| budget_json | json | NO | ∅ |  |
| iteration_count | int unsigned | NO | 0 |  |
| model_call_count | int unsigned | NO | 0 |  |
| tool_call_count | int unsigned | NO | 0 |  |
| reserved_tokens | bigint unsigned | NO | 0 |  |
| input_tokens | bigint unsigned | NO | 0 |  |
| output_tokens | bigint unsigned | NO | 0 |  |
| reserved_cost_micros | bigint unsigned | NO | 0 |  |
| cost_micros | bigint unsigned | NO | 0 |  |
| state_artifact_id | binary(16) | YES | ∅ |  |
| state_hash | char(64) | YES | ∅ |  |
| stop_reason | varchar(64) | YES | ∅ |  |
| version | bigint unsigned | NO | 1 |  |
| started_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |
| ended_at | timestamp(6) | YES | ∅ |  |

| Index | Unique | Columns |
|---|---:|---|
| fk_agent_run_execution | no | execution_id |
| fk_agent_run_node | no | node_execution_id |
| fk_agent_run_state | no | state_artifact_id |
| idx_agent_run_execution | no | tenant_id, execution_id, started_at |
| PRIMARY | yes | id |
| uq_agent_run_node | yes | tenant_id, node_execution_id |

| Foreign key | Columns | References |
|---|---|---|
| fk_agent_run_execution | execution_id | workflow_executions.id |
| fk_agent_run_node | node_execution_id | node_executions.id |
| fk_agent_run_state | state_artifact_id | artifacts.id |
| fk_agent_run_tenant | tenant_id | tenants.id |

### application_api_keys

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| application_id | binary(16) | NO | ∅ |  |
| family_id | binary(16) | NO | ∅ |  |
| name | varchar(160) | NO | ∅ |  |
| key_prefix | varchar(48) | NO | ∅ |  |
| secret_hash | binary(32) | NO | ∅ |  |
| status | enum('active','revoked') | NO | active |  |
| created_by | binary(16) | NO | ∅ |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |
| last_used_at | timestamp(6) | YES | ∅ |  |
| revoked_at | timestamp(6) | YES | ∅ |  |

| Index | Unique | Columns |
|---|---:|---|
| fk_application_api_key_application | no | application_id |
| fk_application_api_key_creator | no | created_by |
| idx_application_api_keys | no | tenant_id, application_id, status |
| PRIMARY | yes | id |
| uq_application_api_key_prefix | yes | key_prefix |

| Foreign key | Columns | References |
|---|---|---|
| fk_application_api_key_application | application_id | applications.id |
| fk_application_api_key_creator | created_by | users.id |
| fk_application_api_key_tenant | tenant_id | tenants.id |

### application_deployment_heads

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| tenant_id | binary(16) | NO | ∅ |  |
| application_id | binary(16) | NO | ∅ |  |
| deployment_id | binary(16) | NO | ∅ |  |
| version | bigint unsigned | NO | 1 |  |
| updated_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED on update CURRENT_TIMESTAMP(6) |

| Index | Unique | Columns |
|---|---:|---|
| fk_application_head_application | no | application_id |
| PRIMARY | yes | tenant_id, application_id |
| uq_application_deployment_head | yes | deployment_id |

| Foreign key | Columns | References |
|---|---|---|
| fk_application_head_application | application_id | applications.id |
| fk_application_head_deployment | deployment_id | application_deployments.id |
| fk_application_head_tenant | tenant_id | tenants.id |

### application_deployments

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| application_id | binary(16) | NO | ∅ |  |
| workflow_version_id | binary(16) | NO | ∅ |  |
| environment_id | binary(16) | NO | ∅ |  |
| sequence_number | bigint unsigned | NO | ∅ |  |
| session_version_policy | enum('pinned','follow_deployment','manual_upgrade') | NO | pinned |  |
| status | enum('active','superseded') | NO | active |  |
| created_by | binary(16) | NO | ∅ |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |

| Index | Unique | Columns |
|---|---:|---|
| fk_application_deployment_application | no | application_id |
| fk_application_deployment_creator | no | created_by |
| fk_application_deployment_environment | no | environment_id |
| fk_application_deployment_version | no | workflow_version_id |
| idx_application_deployment_version | no | tenant_id, workflow_version_id |
| PRIMARY | yes | id |
| uq_application_deployment_sequence | yes | tenant_id, application_id, sequence_number |

| Foreign key | Columns | References |
|---|---|---|
| fk_application_deployment_application | application_id | applications.id |
| fk_application_deployment_creator | created_by | users.id |
| fk_application_deployment_environment | environment_id | workflow_environments.id |
| fk_application_deployment_tenant | tenant_id | tenants.id |
| fk_application_deployment_version | workflow_version_id | workflow_versions.id |

### application_invocations

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| application_id | binary(16) | NO | ∅ |  |
| application_deployment_id | binary(16) | YES | ∅ |  |
| session_id | binary(16) | YES | ∅ |  |
| workflow_version_id | binary(16) | NO | ∅ |  |
| execution_id | binary(16) | YES | ∅ |  |
| runtime_command_id | binary(16) | YES | ∅ |  |
| caller_type | enum('user','api_key','webhook','schedule','poll') | NO | ∅ |  |
| caller_id | binary(16) | YES | ∅ |  |
| request_hash | char(64) | NO | ∅ |  |
| idempotency_key | varchar(128) | NO | ∅ |  |
| status | enum('queued','running','completed','failed','cancelled') | NO | ∅ |  |
| last_event_sequence | bigint unsigned | NO | 0 |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |
| completed_at | timestamp(6) | YES | ∅ |  |

| Index | Unique | Columns |
|---|---:|---|
| fk_invocation_application | no | application_id |
| fk_invocation_application_deployment | no | application_deployment_id |
| fk_invocation_session | no | session_id |
| fk_invocation_workflow_version | no | workflow_version_id |
| idx_invocation_deployment | no | tenant_id, application_deployment_id |
| idx_invocations_session | no | tenant_id, session_id, created_at |
| PRIMARY | yes | id |
| uq_invocation_idempotency | yes | tenant_id, application_id, caller_type, caller_id, idempotency_key |
| uq_invocation_runtime_command | yes | runtime_command_id |

| Foreign key | Columns | References |
|---|---|---|
| fk_invocation_application | application_id | applications.id |
| fk_invocation_application_deployment | application_deployment_id | application_deployments.id |
| fk_invocation_runtime_command | runtime_command_id | runtime_commands.id |
| fk_invocation_session | session_id | application_sessions.id |
| fk_invocation_tenant | tenant_id | tenants.id |
| fk_invocation_workflow_version | workflow_version_id | workflow_versions.id |

### application_message_parts

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| message_id | binary(16) | NO | ∅ |  |
| part_index | int unsigned | NO | ∅ |  |
| part_type | enum('text','json','image','audio','file','tool_call','tool_result') | NO | ∅ |  |
| content_json | json | YES | ∅ |  |
| artifact_id | binary(16) | YES | ∅ |  |

| Index | Unique | Columns |
|---|---:|---|
| fk_message_part_artifact | no | artifact_id |
| fk_message_part_tenant | no | tenant_id |
| PRIMARY | yes | id |
| uq_message_part_index | yes | message_id, part_index |

| Foreign key | Columns | References |
|---|---|---|
| fk_message_part_artifact | artifact_id | artifacts.id |
| fk_message_part_message | message_id | application_messages.id |
| fk_message_part_tenant | tenant_id | tenants.id |

### application_messages

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| session_id | binary(16) | NO | ∅ |  |
| invocation_id | binary(16) | YES | ∅ |  |
| sequence_number | bigint unsigned | NO | ∅ |  |
| role | enum('user','assistant','system','tool') | NO | ∅ |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |

| Index | Unique | Columns |
|---|---:|---|
| fk_message_invocation | no | invocation_id |
| fk_message_session | no | session_id |
| PRIMARY | yes | id |
| uq_message_sequence | yes | tenant_id, session_id, sequence_number |

| Foreign key | Columns | References |
|---|---|---|
| fk_message_invocation | invocation_id | application_invocations.id |
| fk_message_session | session_id | application_sessions.id |
| fk_message_tenant | tenant_id | tenants.id |

### application_schedules

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| application_id | binary(16) | NO | ∅ |  |
| name | varchar(160) | NO | ∅ |  |
| cron_expression | varchar(128) | NO | ∅ |  |
| timezone | varchar(64) | NO | ∅ |  |
| input_json | json | NO | ∅ |  |
| misfire_policy | enum('skip','fire_once') | NO | fire_once |  |
| next_fire_at | timestamp(6) | YES | ∅ |  |
| last_fire_at | timestamp(6) | YES | ∅ |  |
| locked_by | binary(16) | YES | ∅ |  |
| locked_until | timestamp(6) | YES | ∅ |  |
| status | enum('active','disabled') | NO | disabled |  |
| version | bigint unsigned | NO | 1 |  |
| created_by | binary(16) | NO | ∅ |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |
| updated_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED on update CURRENT_TIMESTAMP(6) |

| Index | Unique | Columns |
|---|---:|---|
| fk_application_schedule_application | no | application_id |
| fk_application_schedule_creator | no | created_by |
| idx_application_schedule_scan | no | status, next_fire_at, locked_until |
| idx_application_schedules | no | tenant_id, application_id, status |
| PRIMARY | yes | id |

| Foreign key | Columns | References |
|---|---|---|
| fk_application_schedule_application | application_id | applications.id |
| fk_application_schedule_creator | created_by | users.id |
| fk_application_schedule_tenant | tenant_id | tenants.id |

### application_session_contexts

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| tenant_id | binary(16) | NO | ∅ |  |
| application_deployment_id | binary(16) | NO | ∅ |  |
| session_id | binary(16) | NO | ∅ |  |
| context_json | json | NO | ∅ |  |
| context_version | bigint unsigned | NO | 0 |  |
| updated_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED on update CURRENT_TIMESTAMP(6) |

| Index | Unique | Columns |
|---|---:|---|
| fk_session_context_deployment | no | application_deployment_id |
| fk_session_context_session | no | session_id |
| PRIMARY | yes | tenant_id, application_deployment_id, session_id |

| Foreign key | Columns | References |
|---|---|---|
| fk_session_context_deployment | application_deployment_id | application_deployments.id |
| fk_session_context_session | session_id | application_sessions.id |
| fk_session_context_tenant | tenant_id | tenants.id |

### application_session_version_history

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| session_id | binary(16) | NO | ∅ |  |
| from_workflow_version_id | binary(16) | YES | ∅ |  |
| to_workflow_version_id | binary(16) | NO | ∅ |  |
| changed_by | binary(16) | NO | ∅ |  |
| changed_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |

| Index | Unique | Columns |
|---|---:|---|
| fk_session_history_from_version | no | from_workflow_version_id |
| fk_session_history_session | no | session_id |
| fk_session_history_to_version | no | to_workflow_version_id |
| fk_session_history_user | no | changed_by |
| idx_session_version_history | no | tenant_id, session_id, changed_at |
| PRIMARY | yes | id |

| Foreign key | Columns | References |
|---|---|---|
| fk_session_history_from_version | from_workflow_version_id | workflow_versions.id |
| fk_session_history_session | session_id | application_sessions.id |
| fk_session_history_tenant | tenant_id | tenants.id |
| fk_session_history_to_version | to_workflow_version_id | workflow_versions.id |
| fk_session_history_user | changed_by | users.id |

### application_sessions

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| application_id | binary(16) | NO | ∅ |  |
| application_deployment_id | binary(16) | NO | ∅ |  |
| workflow_version_id | binary(16) | YES | ∅ |  |
| version_policy | enum('pinned','follow_deployment','manual_upgrade') | NO | ∅ |  |
| external_user_id | varchar(255) | YES | ∅ |  |
| title | varchar(255) | YES | ∅ |  |
| status | enum('active','closed') | NO | active |  |
| version | bigint unsigned | NO | 1 |  |
| created_by_user_id | binary(16) | YES | ∅ |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |
| updated_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED on update CURRENT_TIMESTAMP(6) |

| Index | Unique | Columns |
|---|---:|---|
| fk_session_application | no | application_id |
| fk_session_creator | no | created_by_user_id |
| fk_session_deployment | no | application_deployment_id |
| fk_session_workflow_version | no | workflow_version_id |
| idx_application_sessions | no | tenant_id, application_id, updated_at |
| PRIMARY | yes | id |

| Foreign key | Columns | References |
|---|---|---|
| fk_session_application | application_id | applications.id |
| fk_session_creator | created_by_user_id | users.id |
| fk_session_deployment | application_deployment_id | application_deployments.id |
| fk_session_tenant | tenant_id | tenants.id |
| fk_session_workflow_version | workflow_version_id | workflow_versions.id |

### application_webhooks

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| application_id | binary(16) | NO | ∅ |  |
| name | varchar(160) | NO | ∅ |  |
| public_id | varchar(64) | NO | ∅ |  |
| secret_provider | varchar(32) | NO | local_encrypted |  |
| secret_ref | varchar(512) | YES | ∅ |  |
| secret_provider_version | varchar(128) | YES | ∅ |  |
| secret_algorithm | varchar(32) | YES | ∅ |  |
| secret_key_id | varchar(64) | YES | ∅ |  |
| secret_nonce | varbinary(32) | YES | ∅ |  |
| secret_ciphertext | blob | YES | ∅ |  |
| status | enum('active','disabled') | NO | active |  |
| version | bigint unsigned | NO | 1 |  |
| created_by | binary(16) | NO | ∅ |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |
| updated_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED on update CURRENT_TIMESTAMP(6) |

| Index | Unique | Columns |
|---|---:|---|
| fk_application_webhook_application | no | application_id |
| fk_application_webhook_creator | no | created_by |
| idx_application_webhooks | no | tenant_id, application_id, status |
| PRIMARY | yes | id |
| uq_application_webhook_public | yes | public_id |

| Foreign key | Columns | References |
|---|---|---|
| fk_application_webhook_application | application_id | applications.id |
| fk_application_webhook_creator | created_by | users.id |
| fk_application_webhook_tenant | tenant_id | tenants.id |

### applications

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| workflow_id | binary(16) | NO | ∅ |  |
| name | varchar(160) | NO | ∅ |  |
| slug | varchar(96) | NO | ∅ |  |
| description | varchar(1000) | YES | ∅ |  |
| visibility | enum('private','department','company') | NO | private |  |
| owner_user_id | binary(16) | NO | ∅ |  |
| owner_department_id | binary(16) | NO | ∅ |  |
| status | enum('draft','active','disabled') | NO | draft |  |
| version | bigint unsigned | NO | 1 |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |
| updated_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED on update CURRENT_TIMESTAMP(6) |

| Index | Unique | Columns |
|---|---:|---|
| fk_application_department | no | owner_department_id |
| fk_application_owner | no | owner_user_id |
| fk_application_workflow | no | workflow_id |
| idx_applications_department | no | tenant_id, owner_department_id, status |
| idx_applications_tenant | no | tenant_id, status, updated_at |
| PRIMARY | yes | id |
| uq_application_slug | yes | tenant_id, slug |

| Foreign key | Columns | References |
|---|---|---|
| fk_application_department | owner_department_id | departments.id |
| fk_application_owner | owner_user_id | users.id |
| fk_application_tenant | tenant_id | tenants.id |
| fk_application_workflow | workflow_id | workflows.id |

### approval_actions

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| approval_task_id | binary(16) | NO | ∅ |  |
| actor_user_id | binary(16) | NO | ∅ |  |
| action_type | enum('claim','release','reassign','approve','reject','cancel','timeout') | NO | ∅ |  |
| input_json | json | YES | ∅ |  |
| from_status | varchar(32) | NO | ∅ |  |
| to_status | varchar(32) | NO | ∅ |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |

| Index | Unique | Columns |
|---|---:|---|
| fk_approval_action_actor | no | actor_user_id |
| fk_approval_action_task | no | approval_task_id |
| idx_approval_actions_task | no | tenant_id, approval_task_id, created_at |
| PRIMARY | yes | id |

| Foreign key | Columns | References |
|---|---|---|
| fk_approval_action_actor | actor_user_id | users.id |
| fk_approval_action_task | approval_task_id | approval_tasks.id |
| fk_approval_action_tenant | tenant_id | tenants.id |

### approval_candidates

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| tenant_id | binary(16) | NO | ∅ |  |
| approval_task_id | binary(16) | NO | ∅ |  |
| candidate_type | enum('user','role','department') | NO | ∅ |  |
| candidate_id | binary(16) | NO | ∅ |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |

| Index | Unique | Columns |
|---|---:|---|
| fk_approval_candidate_task | no | approval_task_id |
| PRIMARY | yes | tenant_id, approval_task_id, candidate_type, candidate_id |

| Foreign key | Columns | References |
|---|---|---|
| fk_approval_candidate_task | approval_task_id | approval_tasks.id |
| fk_approval_candidate_tenant | tenant_id | tenants.id |

### approval_tasks

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| execution_id | binary(16) | NO | ∅ |  |
| workflow_id | binary(16) | NO | ∅ |  |
| node_id | varchar(128) | NO | ∅ |  |
| node_execution_id | binary(16) | YES | ∅ |  |
| resume_token_id | binary(16) | YES | ∅ |  |
| title | varchar(255) | NO | ∅ |  |
| description | varchar(2000) | YES | ∅ |  |
| request_payload_json | json | YES | ∅ |  |
| status | enum('pending','claimed','approved','rejected','cancelled','timed_out') | NO | pending |  |
| claimed_by | binary(16) | YES | ∅ |  |
| claimed_at | timestamp(6) | YES | ∅ |  |
| resume_status | enum('not_requested','pending','succeeded','blocked_runtime','failed') | NO | not_requested |  |
| decision_command_id | binary(16) | YES | ∅ |  |
| deadline_at | timestamp(6) | YES | ∅ |  |
| version | bigint unsigned | NO | 1 |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |
| updated_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED on update CURRENT_TIMESTAMP(6) |

| Index | Unique | Columns |
|---|---:|---|
| fk_approval_resume_token | no | resume_token_id |
| fk_approval_task_claimed_by | no | claimed_by |
| fk_approval_task_execution | no | execution_id |
| fk_approval_task_workflow | no | workflow_id |
| idx_approval_tasks_inbox | no | tenant_id, status, deadline_at, created_at |
| PRIMARY | yes | id |
| uq_approval_decision_command | yes | decision_command_id |
| uq_approval_node_execution | yes | node_execution_id |

| Foreign key | Columns | References |
|---|---|---|
| fk_approval_decision_command | decision_command_id | runtime_commands.id |
| fk_approval_node_execution | node_execution_id | node_executions.id |
| fk_approval_resume_token | resume_token_id | execution_resume_tokens.id |
| fk_approval_task_claimed_by | claimed_by | users.id |
| fk_approval_task_execution | execution_id | workflow_executions.id |
| fk_approval_task_tenant | tenant_id | tenants.id |
| fk_approval_task_workflow | workflow_id | workflows.id |

### artifact_references

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| tenant_id | binary(16) | NO | ∅ |  |
| artifact_id | binary(16) | NO | ∅ |  |
| owner_type | varchar(64) | NO | ∅ |  |
| owner_id | varchar(128) | NO | ∅ |  |
| reference_role | varchar(64) | NO | ∅ |  |
| retention_until | timestamp(6) | YES | ∅ |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |

| Index | Unique | Columns |
|---|---:|---|
| fk_artifact_reference_artifact | no | artifact_id |
| idx_artifact_reference_owner | no | tenant_id, owner_type, owner_id |
| idx_artifact_reference_retention | no | tenant_id, artifact_id, retention_until |
| PRIMARY | yes | tenant_id, artifact_id, owner_type, owner_id, reference_role |

| Foreign key | Columns | References |
|---|---|---|
| fk_artifact_reference_artifact | artifact_id | artifacts.id |
| fk_artifact_reference_tenant | tenant_id | tenants.id |

### artifacts

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| content_type | varchar(255) | NO | ∅ |  |
| size_bytes | bigint unsigned | NO | ∅ |  |
| sha256 | char(64) | NO | ∅ |  |
| storage_key | varchar(512) | NO | ∅ |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |
| deleted_at | timestamp(6) | YES | ∅ |  |

| Index | Unique | Columns |
|---|---:|---|
| idx_artifacts_tenant_created | no | tenant_id, created_at |
| idx_artifacts_tenant_hash | no | tenant_id, sha256 |
| PRIMARY | yes | id |
| uq_artifacts_storage_key | yes | storage_key |

| Foreign key | Columns | References |
|---|---|---|
| r | t | i.f |
| k | _ | a.r |
| e | n | a.n |
| e | n | a.n |
| d |  | . |

### audit_events

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| actor_user_id | binary(16) | YES | ∅ |  |
| action | varchar(128) | NO | ∅ |  |
| target_type | varchar(128) | NO | ∅ |  |
| target_id | varchar(128) | NO | ∅ |  |
| request_id | binary(16) | NO | ∅ |  |
| detail_json | json | NO | ∅ |  |
| occurred_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |

| Index | Unique | Columns |
|---|---:|---|
| fk_audit_actor | no | actor_user_id |
| idx_audit_target | no | tenant_id, target_type, target_id |
| idx_audit_tenant_time | no | tenant_id, occurred_at |
| PRIMARY | yes | id |

| Foreign key | Columns | References |
|---|---|---|
| fk_audit_actor | actor_user_id | users.id |
| fk_audit_tenant | tenant_id | tenants.id |

### auth_login_attempts

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| login_key | char(64) | NO | ∅ |  |
| failure_count | int unsigned | NO | 0 |  |
| window_started_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |
| locked_until | timestamp(6) | YES | ∅ |  |
| updated_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED on update CURRENT_TIMESTAMP(6) |

| Index | Unique | Columns |
|---|---:|---|
| idx_auth_login_attempts_locked | no | locked_until |
| PRIMARY | yes | login_key |

| Foreign key | Columns | References |
|---|---|---|
| — | — | — |

### bootstrap_state

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| singleton_id | tinyint unsigned | NO | ∅ |  |
| state | enum('required','completed') | NO | ∅ |  |
| tenant_id | binary(16) | YES | ∅ |  |
| completed_at | timestamp(6) | YES | ∅ |  |

| Index | Unique | Columns |
|---|---:|---|
| fk_bootstrap_tenant | no | tenant_id |
| PRIMARY | yes | singleton_id |

| Foreign key | Columns | References |
|---|---|---|
| o | o | t.s |
| k | _ | b.o |
| e | n | a.n |
| e | n | a.n |
| d |  | . |

### canvas_plugin_object_gc

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| object_key | varchar(512) | NO | ∅ |  |
| reason | varchar(64) | NO | ∅ |  |
| attempt_count | int unsigned | NO | 0 |  |
| available_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |
| last_error | varchar(1000) | YES | ∅ |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |

| Index | Unique | Columns |
|---|---:|---|
| idx_canvas_plugin_object_gc_claim | no | available_at, created_at |
| PRIMARY | yes | id |
| uq_canvas_plugin_object_gc_key | yes | tenant_id, object_key |

### canvas_plugin_imports

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| bundle_digest | char(71) | NO | ∅ |  |
| artifact_key | varchar(512) | NO | ∅ |  |
| status | enum('ready','installed','failed','cancelled') | NO | ∅ |  |
| manifest_json | json | NO | ∅ |  |
| issues_json | json | NO | ∅ |  |
| installed_version_id | binary(16) | YES | ∅ |  |
| created_by | binary(16) | NO | ∅ |  |
| expires_at | timestamp(6) | NO | ∅ |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |
| updated_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED on update CURRENT_TIMESTAMP(6) |

| Index | Unique | Columns |
|---|---:|---|
| idx_canvas_plugin_import_expiry | no | status, expires_at |
| PRIMARY | yes | id |
| uq_canvas_plugin_import_digest | yes | tenant_id, bundle_digest |

### canvas_plugin_versions

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| plugin_id | binary(16) | NO | ∅ |  |
| package_version | varchar(64) | NO | ∅ |  |
| bundle_digest | char(71) | NO | ∅ |  |
| status | enum('enabled','disabled') | NO | enabled |  |
| sdk_api_version | int unsigned | NO | ∅ |  |
| manifest_json | json | NO | ∅ |  |
| artifact_key | varchar(512) | NO | ∅ |  |
| created_by | binary(16) | NO | ∅ |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |

| Index | Unique | Columns |
|---|---:|---|
| idx_canvas_plugin_versions_plugin | no | tenant_id, plugin_id, status, created_at |
| PRIMARY | yes | id |
| uq_canvas_plugin_digest | yes | tenant_id, bundle_digest |
| uq_canvas_plugin_version | yes | tenant_id, plugin_id, package_version |

### canvas_plugins

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| package_id | varchar(160) | NO | ∅ |  |
| display_name | varchar(160) | NO | ∅ |  |
| description | varchar(1000) | NO | ∅ |  |
| source_type | enum('builtin','imported') | NO | imported |  |
| default_version_id | binary(16) | YES | ∅ |  |
| version | bigint unsigned | NO | 1 |  |
| created_by | binary(16) | NO | ∅ |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |
| updated_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED on update CURRENT_TIMESTAMP(6) |

| Index | Unique | Columns |
|---|---:|---|
| idx_canvas_plugins_tenant_updated | no | tenant_id, updated_at |
| PRIMARY | yes | id |
| uq_canvas_plugin_package | yes | tenant_id, package_id |

### checkpoint_artifacts

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| tenant_id | binary(16) | NO | ∅ |  |
| checkpoint_id | binary(16) | NO | ∅ |  |
| artifact_id | binary(16) | NO | ∅ |  |
| role | enum('state','input','output','binary','log') | NO | ∅ |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |

| Index | Unique | Columns |
|---|---:|---|
| fk_checkpoint_artifact_tenant | no | tenant_id |
| fk_checkpoint_artifact_value | no | artifact_id |
| PRIMARY | yes | checkpoint_id, artifact_id, role |

| Foreign key | Columns | References |
|---|---|---|
| fk_checkpoint_artifact_checkpoint | checkpoint_id | checkpoints.id |
| fk_checkpoint_artifact_tenant | tenant_id | tenants.id |
| fk_checkpoint_artifact_value | artifact_id | artifacts.id |

### checkpoints

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| execution_id | binary(16) | NO | ∅ |  |
| node_execution_id | binary(16) | YES | ∅ |  |
| sequence_number | bigint unsigned | NO | ∅ |  |
| checkpoint_type | enum('execution_start','node_completed','node_suspended','manual') | NO | ∅ |  |
| state_hash | varchar(96) | NO | ∅ |  |
| payload_json | json | YES | ∅ |  |
| payload_artifact_id | binary(16) | YES | ∅ |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |

| Index | Unique | Columns |
|---|---:|---|
| fk_checkpoint_artifact | no | payload_artifact_id |
| fk_checkpoint_node | no | node_execution_id |
| idx_checkpoint_timeline | no | tenant_id, execution_id, created_at |
| PRIMARY | yes | id |
| uq_checkpoint_sequence | yes | execution_id, sequence_number |

| Foreign key | Columns | References |
|---|---|---|
| fk_checkpoint_artifact | payload_artifact_id | artifacts.id |
| fk_checkpoint_execution | execution_id | workflow_executions.id |
| fk_checkpoint_node | node_execution_id | node_executions.id |
| fk_checkpoint_tenant | tenant_id | tenants.id |

### credential_secret_versions

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| credential_id | binary(16) | NO | ∅ |  |
| version_number | bigint unsigned | NO | ∅ |  |
| provider | varchar(32) | NO | local_encrypted |  |
| secret_ref | varchar(512) | YES | ∅ |  |
| provider_version | varchar(128) | YES | ∅ |  |
| algorithm | varchar(32) | YES | ∅ |  |
| key_id | varchar(64) | YES | ∅ |  |
| nonce | varbinary(32) | YES | ∅ |  |
| ciphertext | mediumblob | YES | ∅ |  |
| created_by | binary(16) | NO | ∅ |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |

| Index | Unique | Columns |
|---|---:|---|
| fk_credential_secret_credential | no | credential_id |
| fk_credential_secret_user | no | created_by |
| PRIMARY | yes | id |
| uq_credential_secret_version | yes | tenant_id, credential_id, version_number |

| Foreign key | Columns | References |
|---|---|---|
| fk_credential_secret_credential | credential_id | credentials.id |
| fk_credential_secret_tenant | tenant_id | tenants.id |
| fk_credential_secret_user | created_by | users.id |

### credentials

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| name | varchar(160) | NO | ∅ |  |
| credential_type | enum('api_key','bearer','basic','custom_json') | NO | ∅ |  |
| storage_mode | enum('local_encrypted','external_reference') | NO | local_encrypted |  |
| masked_hint | varchar(32) | NO | ∅ |  |
| status | enum('active','disabled') | NO | active |  |
| current_secret_version | bigint unsigned | NO | 1 |  |
| owner_department_id | binary(16) | NO | ∅ |  |
| created_by | binary(16) | NO | ∅ |  |
| version | bigint unsigned | NO | 1 |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |
| updated_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED on update CURRENT_TIMESTAMP(6) |

| Index | Unique | Columns |
|---|---:|---|
| fk_credentials_department | no | owner_department_id |
| fk_credentials_user | no | created_by |
| idx_credentials_tenant_status | no | tenant_id, status, updated_at |
| PRIMARY | yes | id |

| Foreign key | Columns | References |
|---|---|---|
| fk_credentials_department | owner_department_id | departments.id |
| fk_credentials_tenant | tenant_id | tenants.id |
| fk_credentials_user | created_by | users.id |

### dataset_cases

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| dataset_id | binary(16) | NO | ∅ |  |
| case_key | varchar(128) | NO | ∅ |  |
| name | varchar(255) | NO | ∅ |  |
| input_json | json | NO | ∅ |  |
| expected_output_json | json | YES | ∅ |  |
| context_json | json | YES | ∅ |  |
| tags_json | json | NO | ∅ |  |
| evaluator_override_json | json | YES | ∅ |  |
| sort_order | bigint unsigned | NO | ∅ |  |
| version | bigint unsigned | NO | 1 |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |
| updated_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED on update CURRENT_TIMESTAMP(6) |

| Index | Unique | Columns |
|---|---:|---|
| fk_dataset_case_dataset | no | dataset_id |
| idx_dataset_cases_order | no | tenant_id, dataset_id, sort_order |
| PRIMARY | yes | id |
| uq_dataset_case_key | yes | tenant_id, dataset_id, case_key |

| Foreign key | Columns | References |
|---|---|---|
| fk_dataset_case_dataset | dataset_id | datasets.id |
| fk_dataset_case_tenant | tenant_id | tenants.id |

### dataset_version_cases

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| tenant_id | binary(16) | NO | ∅ |  |
| dataset_version_id | binary(16) | NO | ∅ |  |
| source_case_id | binary(16) | NO | ∅ |  |
| case_key | varchar(128) | NO | ∅ |  |
| name | varchar(255) | NO | ∅ |  |
| input_json | json | NO | ∅ |  |
| expected_output_json | json | YES | ∅ |  |
| context_json | json | YES | ∅ |  |
| tags_json | json | NO | ∅ |  |
| evaluator_override_json | json | YES | ∅ |  |
| sort_order | bigint unsigned | NO | ∅ |  |
| content_hash | char(64) | NO | ∅ |  |

| Index | Unique | Columns |
|---|---:|---|
| fk_dataset_version_case_version | no | dataset_version_id |
| idx_dataset_version_case_order | no | tenant_id, dataset_version_id, sort_order |
| PRIMARY | yes | tenant_id, dataset_version_id, source_case_id |

| Foreign key | Columns | References |
|---|---|---|
| fk_dataset_version_case_tenant | tenant_id | tenants.id |
| fk_dataset_version_case_version | dataset_version_id | dataset_versions.id |

### dataset_versions

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| dataset_id | binary(16) | NO | ∅ |  |
| version_number | bigint unsigned | NO | ∅ |  |
| source_revision | bigint unsigned | NO | ∅ |  |
| content_hash | char(64) | NO | ∅ |  |
| case_count | bigint unsigned | NO | ∅ |  |
| created_by | binary(16) | NO | ∅ |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |

| Index | Unique | Columns |
|---|---:|---|
| fk_dataset_version_creator | no | created_by |
| fk_dataset_version_dataset | no | dataset_id |
| PRIMARY | yes | id |
| uq_dataset_version_hash | yes | tenant_id, dataset_id, source_revision, content_hash |
| uq_dataset_version_number | yes | tenant_id, dataset_id, version_number |

| Foreign key | Columns | References |
|---|---|---|
| fk_dataset_version_creator | created_by | users.id |
| fk_dataset_version_dataset | dataset_id | datasets.id |
| fk_dataset_version_tenant | tenant_id | tenants.id |

### datasets

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| name | varchar(160) | NO | ∅ |  |
| description | varchar(1000) | YES | ∅ |  |
| visibility | enum('private','department','company') | NO | private |  |
| owner_user_id | binary(16) | NO | ∅ |  |
| owner_department_id | binary(16) | NO | ∅ |  |
| status | enum('active','disabled') | NO | active |  |
| revision | bigint unsigned | NO | 0 |  |
| version | bigint unsigned | NO | 1 |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |
| updated_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED on update CURRENT_TIMESTAMP(6) |

| Index | Unique | Columns |
|---|---:|---|
| fk_dataset_department | no | owner_department_id |
| fk_dataset_owner | no | owner_user_id |
| idx_datasets_department | no | tenant_id, owner_department_id |
| idx_datasets_tenant | no | tenant_id, status, updated_at |
| PRIMARY | yes | id |

| Foreign key | Columns | References |
|---|---|---|
| fk_dataset_department | owner_department_id | departments.id |
| fk_dataset_owner | owner_user_id | users.id |
| fk_dataset_tenant | tenant_id | tenants.id |

### department_closure

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| tenant_id | binary(16) | NO | ∅ |  |
| ancestor_id | binary(16) | NO | ∅ |  |
| descendant_id | binary(16) | NO | ∅ |  |
| depth | int unsigned | NO | ∅ |  |

| Index | Unique | Columns |
|---|---:|---|
| fk_department_closure_ancestor | no | ancestor_id |
| fk_department_closure_descendant | no | descendant_id |
| idx_department_closure_descendant | no | tenant_id, descendant_id, ancestor_id |
| PRIMARY | yes | tenant_id, ancestor_id, descendant_id |

| Foreign key | Columns | References |
|---|---|---|
| fk_department_closure_ancestor | ancestor_id | departments.id |
| fk_department_closure_descendant | descendant_id | departments.id |
| fk_department_closure_tenant | tenant_id | tenants.id |

### departments

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| parent_id | binary(16) | YES | ∅ |  |
| name | varchar(100) | NO | ∅ |  |
| normalized_name | varchar(100) | NO | ∅ |  |
| is_root | tinyint(1) | NO | 0 |  |
| status | enum('active','disabled') | NO | active |  |
| version | bigint unsigned | NO | 1 |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |
| updated_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED on update CURRENT_TIMESTAMP(6) |

| Index | Unique | Columns |
|---|---:|---|
| fk_departments_parent | no | parent_id |
| idx_departments_tenant_parent | no | tenant_id, parent_id |
| PRIMARY | yes | id |
| uq_departments_sibling_name | yes | tenant_id, parent_id, normalized_name |

| Foreign key | Columns | References |
|---|---|---|
| fk_departments_parent | parent_id | departments.id |
| fk_departments_tenant | tenant_id | tenants.id |

### deployment_history

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| workflow_id | binary(16) | NO | ∅ |  |
| environment_id | binary(16) | NO | ∅ |  |
| deployment_id | binary(16) | NO | ∅ |  |
| action | enum('published','superseded','rolled_back') | NO | ∅ |  |
| actor_user_id | binary(16) | NO | ∅ |  |
| occurred_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |

| Index | Unique | Columns |
|---|---:|---|
| fk_deployment_history_actor | no | actor_user_id |
| fk_deployment_history_deployment | no | deployment_id |
| fk_deployment_history_environment | no | environment_id |
| fk_deployment_history_workflow | no | workflow_id |
| idx_deployment_history | no | tenant_id, workflow_id, environment_id, occurred_at |
| PRIMARY | yes | id |

| Foreign key | Columns | References |
|---|---|---|
| fk_deployment_history_actor | actor_user_id | users.id |
| fk_deployment_history_deployment | deployment_id | workflow_deployments.id |
| fk_deployment_history_environment | environment_id | workflow_environments.id |
| fk_deployment_history_tenant | tenant_id | tenants.id |
| fk_deployment_history_workflow | workflow_id | workflows.id |

### evaluation_case_results

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| evaluation_run_id | binary(16) | NO | ∅ |  |
| source_case_id | binary(16) | NO | ∅ |  |
| execution_id | binary(16) | NO | ∅ |  |
| status | enum('passed','failed','error','cancelled') | NO | ∅ |  |
| score | decimal(12,6) | YES | ∅ |  |
| detail_json | json | NO | ∅ |  |
| duration_ms | bigint unsigned | YES | ∅ |  |
| cost_micros | bigint unsigned | NO | 0 |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |

| Index | Unique | Columns |
|---|---:|---|
| fk_evaluation_case_execution | no | execution_id |
| fk_evaluation_case_result_run | no | evaluation_run_id |
| PRIMARY | yes | id |
| uq_evaluation_case_result | yes | tenant_id, evaluation_run_id, source_case_id |

| Foreign key | Columns | References |
|---|---|---|
| fk_evaluation_case_execution | execution_id | workflow_executions.id |
| fk_evaluation_case_result_run | evaluation_run_id | evaluation_runs.id |
| fk_evaluation_case_result_tenant | tenant_id | tenants.id |

### evaluation_comparisons

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| name | varchar(160) | NO | ∅ |  |
| baseline_run_id | binary(16) | NO | ∅ |  |
| candidate_run_id | binary(16) | NO | ∅ |  |
| created_by | binary(16) | NO | ∅ |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |

| Index | Unique | Columns |
|---|---:|---|
| fk_evaluation_comparison_baseline | no | baseline_run_id |
| fk_evaluation_comparison_candidate | no | candidate_run_id |
| fk_evaluation_comparison_creator | no | created_by |
| fk_evaluation_comparison_tenant | no | tenant_id |
| PRIMARY | yes | id |

| Foreign key | Columns | References |
|---|---|---|
| fk_evaluation_comparison_baseline | baseline_run_id | evaluation_runs.id |
| fk_evaluation_comparison_candidate | candidate_run_id | evaluation_runs.id |
| fk_evaluation_comparison_creator | created_by | users.id |
| fk_evaluation_comparison_tenant | tenant_id | tenants.id |

### evaluation_metrics

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| tenant_id | binary(16) | NO | ∅ |  |
| evaluation_run_id | binary(16) | NO | ∅ |  |
| metric_key | varchar(128) | NO | ∅ |  |
| metric_value | decimal(24,8) | NO | ∅ |  |
| detail_json | json | YES | ∅ |  |

| Index | Unique | Columns |
|---|---:|---|
| fk_evaluation_metric_run | no | evaluation_run_id |
| PRIMARY | yes | tenant_id, evaluation_run_id, metric_key |

| Foreign key | Columns | References |
|---|---|---|
| fk_evaluation_metric_run | evaluation_run_id | evaluation_runs.id |
| fk_evaluation_metric_tenant | tenant_id | tenants.id |

### evaluation_profile_rules

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| profile_version_id | binary(16) | NO | ∅ |  |
| rule_key | varchar(128) | NO | ∅ |  |
| name | varchar(160) | NO | ∅ |  |
| evaluator_type | enum('exact','contains','regex','json_schema','llm_judge','custom_code') | NO | ∅ |  |
| configuration_json | json | NO | ∅ |  |
| weight | decimal(10,4) | NO | 1.0000 |  |
| required | tinyint(1) | NO | 1 |  |
| sort_order | int unsigned | NO | ∅ |  |

| Index | Unique | Columns |
|---|---:|---|
| fk_evaluation_profile_rule_version | no | profile_version_id |
| idx_evaluation_profile_rule_order | no | tenant_id, profile_version_id, sort_order |
| PRIMARY | yes | id |
| uq_evaluation_profile_rule | yes | tenant_id, profile_version_id, rule_key |

| Foreign key | Columns | References |
|---|---|---|
| fk_evaluation_profile_rule_tenant | tenant_id | tenants.id |
| fk_evaluation_profile_rule_version | profile_version_id | evaluation_profile_versions.id |

### evaluation_profile_versions

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| profile_id | binary(16) | NO | ∅ |  |
| version_number | bigint unsigned | NO | ∅ |  |
| aggregation | enum('all','any','weighted') | NO | all |  |
| pass_threshold | decimal(8,6) | NO | 1.000000 |  |
| content_hash | char(64) | NO | ∅ |  |
| created_by | binary(16) | NO | ∅ |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |

| Index | Unique | Columns |
|---|---:|---|
| fk_evaluation_profile_version_creator | no | created_by |
| fk_evaluation_profile_version_profile | no | profile_id |
| PRIMARY | yes | id |
| uq_evaluation_profile_version | yes | tenant_id, profile_id, version_number |
| uq_evaluation_profile_version_hash | yes | tenant_id, profile_id, content_hash |

| Foreign key | Columns | References |
|---|---|---|
| fk_evaluation_profile_version_creator | created_by | users.id |
| fk_evaluation_profile_version_profile | profile_id | evaluation_profiles.id |
| fk_evaluation_profile_version_tenant | tenant_id | tenants.id |

### evaluation_profiles

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| name | varchar(160) | NO | ∅ |  |
| description | varchar(1000) | YES | ∅ |  |
| owner_user_id | binary(16) | NO | ∅ |  |
| owner_department_id | binary(16) | NO | ∅ |  |
| visibility | enum('private','department','company') | NO | private |  |
| status | enum('active','disabled') | NO | active |  |
| version | bigint unsigned | NO | 1 |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |
| updated_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED on update CURRENT_TIMESTAMP(6) |

| Index | Unique | Columns |
|---|---:|---|
| fk_evaluation_profile_department | no | owner_department_id |
| fk_evaluation_profile_owner | no | owner_user_id |
| idx_evaluation_profiles | no | tenant_id, updated_at |
| PRIMARY | yes | id |

| Foreign key | Columns | References |
|---|---|---|
| fk_evaluation_profile_department | owner_department_id | departments.id |
| fk_evaluation_profile_owner | owner_user_id | users.id |
| fk_evaluation_profile_tenant | tenant_id | tenants.id |

### evaluation_rule_results

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| evaluation_run_case_id | binary(16) | NO | ∅ |  |
| profile_rule_id | binary(16) | NO | ∅ |  |
| evaluator_command_id | binary(16) | YES | ∅ |  |
| evaluator_execution_id | binary(16) | YES | ∅ |  |
| status | enum('queued','running','passed','failed','error','cancelled') | NO | ∅ |  |
| passed | tinyint(1) | YES | ∅ |  |
| score | decimal(12,6) | YES | ∅ |  |
| detail_json | json | NO | ∅ |  |
| duration_ms | bigint unsigned | YES | ∅ |  |
| cost_micros | bigint unsigned | NO | 0 |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |
| completed_at | timestamp(6) | YES | ∅ |  |

| Index | Unique | Columns |
|---|---:|---|
| fk_evaluation_rule_case | no | evaluation_run_case_id |
| fk_evaluation_rule_command | no | evaluator_command_id |
| fk_evaluation_rule_execution | no | evaluator_execution_id |
| fk_evaluation_rule_profile | no | profile_rule_id |
| idx_evaluation_rule_status | no | tenant_id, status, created_at |
| PRIMARY | yes | id |
| uq_evaluation_case_rule | yes | tenant_id, evaluation_run_case_id, profile_rule_id |

| Foreign key | Columns | References |
|---|---|---|
| fk_evaluation_rule_case | evaluation_run_case_id | evaluation_run_cases.id |
| fk_evaluation_rule_command | evaluator_command_id | runtime_commands.id |
| fk_evaluation_rule_execution | evaluator_execution_id | workflow_executions.id |
| fk_evaluation_rule_profile | profile_rule_id | evaluation_profile_rules.id |
| fk_evaluation_rule_tenant | tenant_id | tenants.id |

### evaluation_run_cases

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| evaluation_run_id | binary(16) | NO | ∅ |  |
| source_case_id | binary(16) | NO | ∅ |  |
| target_command_id | binary(16) | NO | ∅ |  |
| target_execution_id | binary(16) | YES | ∅ |  |
| status | enum('queued','running','scoring','completed','failed','cancelled') | NO | queued |  |
| actual_output_json | json | YES | ∅ |  |
| duration_ms | bigint unsigned | YES | ∅ |  |
| cost_micros | bigint unsigned | NO | 0 |  |
| error_code | varchar(128) | YES | ∅ |  |
| error_message | varchar(1000) | YES | ∅ |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |
| completed_at | timestamp(6) | YES | ∅ |  |

| Index | Unique | Columns |
|---|---:|---|
| fk_evaluation_case_run_execution | no | target_execution_id |
| fk_evaluation_case_run_run | no | evaluation_run_id |
| idx_evaluation_case_status | no | tenant_id, evaluation_run_id, status |
| PRIMARY | yes | id |
| uq_evaluation_case_command | yes | target_command_id |
| uq_evaluation_run_case | yes | tenant_id, evaluation_run_id, source_case_id |

| Foreign key | Columns | References |
|---|---|---|
| fk_evaluation_case_run_command | target_command_id | runtime_commands.id |
| fk_evaluation_case_run_execution | target_execution_id | workflow_executions.id |
| fk_evaluation_case_run_run | evaluation_run_id | evaluation_runs.id |
| fk_evaluation_case_run_tenant | tenant_id | tenants.id |

### evaluation_runs

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| name | varchar(160) | NO | ∅ |  |
| workflow_version_id | binary(16) | NO | ∅ |  |
| dataset_version_id | binary(16) | NO | ∅ |  |
| evaluation_profile_version_id | binary(16) | NO | ∅ |  |
| parameters_json | json | NO | ∅ |  |
| status | enum('created','queued','running','completed','failed','cancelled') | NO | created |  |
| created_by | binary(16) | NO | ∅ |  |
| owner_department_id | binary(16) | NO | ∅ |  |
| visibility | enum('private','department','company') | NO | private |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |
| started_at | timestamp(6) | YES | ∅ |  |
| completed_at | timestamp(6) | YES | ∅ |  |

| Index | Unique | Columns |
|---|---:|---|
| fk_evaluation_run_creator | no | created_by |
| fk_evaluation_run_dataset_version | no | dataset_version_id |
| fk_evaluation_run_department | no | owner_department_id |
| fk_evaluation_run_profile_version | no | evaluation_profile_version_id |
| fk_evaluation_run_workflow_version | no | workflow_version_id |
| idx_evaluation_runs | no | tenant_id, status, created_at |
| PRIMARY | yes | id |

| Foreign key | Columns | References |
|---|---|---|
| fk_evaluation_run_creator | created_by | users.id |
| fk_evaluation_run_dataset_version | dataset_version_id | dataset_versions.id |
| fk_evaluation_run_department | owner_department_id | departments.id |
| fk_evaluation_run_profile_version | evaluation_profile_version_id | evaluation_profile_versions.id |
| fk_evaluation_run_tenant | tenant_id | tenants.id |
| fk_evaluation_run_workflow_version | workflow_version_id | workflow_versions.id |

### execution_edge_deliveries

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| execution_id | binary(16) | NO | ∅ |  |
| sequence_number | bigint unsigned | NO | ∅ |  |
| connection_id | varchar(128) | NO | ∅ |  |
| source_node_execution_id | binary(16) | NO | ∅ |  |
| source_port | varchar(128) | NO | ∅ |  |
| target_node_id | varchar(128) | NO | ∅ |  |
| target_port | varchar(128) | NO | ∅ |  |
| target_generation | int unsigned | NO | ∅ |  |
| delivery_kind | enum('data','closed_without_data') | NO | ∅ |  |
| item_count | int unsigned | NO | 0 |  |
| payload_json | json | YES | ∅ |  |
| payload_artifact_id | binary(16) | YES | ∅ |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |

| Index | Unique | Columns |
|---|---:|---|
| fk_edge_delivery_artifact | no | payload_artifact_id |
| fk_edge_delivery_source | no | source_node_execution_id |
| idx_edge_delivery_frontier | no | tenant_id, execution_id, target_node_id, target_generation |
| PRIMARY | yes | id |
| uq_edge_delivery_sequence | yes | execution_id, sequence_number |

| Foreign key | Columns | References |
|---|---|---|
| fk_edge_delivery_artifact | payload_artifact_id | artifacts.id |
| fk_edge_delivery_execution | execution_id | workflow_executions.id |
| fk_edge_delivery_source | source_node_execution_id | node_executions.id |
| fk_edge_delivery_tenant | tenant_id | tenants.id |

### execution_end_deliveries

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| tenant_id | binary(16) | NO | ∅ |  |
| execution_id | binary(16) | NO | ∅ |  |
| sequence_number | bigint unsigned | NO | ∅ |  |
| source_node_execution_id | binary(16) | NO | ∅ |  |
| source_node_id | varchar(128) | NO | ∅ |  |
| source_port | varchar(128) | NO | ∅ |  |
| target_port | enum('main','error') | NO | ∅ |  |
| payload_json | json | NO | ∅ |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |

| Index | Unique | Columns |
|---|---:|---|
| fk_end_delivery_source | no | source_node_execution_id |
| idx_end_delivery_terminal | no | tenant_id, execution_id, target_port, sequence_number |
| PRIMARY | yes | execution_id, sequence_number |

| Foreign key | Columns | References |
|---|---|---|
| fk_end_delivery_execution | execution_id | workflow_executions.id |
| fk_end_delivery_source | source_node_execution_id | node_executions.id |
| fk_end_delivery_tenant | tenant_id | tenants.id |

### execution_events

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| tenant_id | binary(16) | NO | ∅ |  |
| execution_id | binary(16) | NO | ∅ |  |
| event_id | binary(16) | NO | ∅ |  |
| sequence_number | bigint unsigned | NO | ∅ |  |
| event_type | varchar(64) | NO | ∅ |  |
| schema_version | varchar(16) | NO | 1.0 |  |
| status | varchar(32) | NO | ∅ |  |
| summary_json | json | NO | ∅ |  |
| occurred_at | timestamp(6) | NO | ∅ |  |

| Index | Unique | Columns |
|---|---:|---|
| fk_execution_event_execution | no | execution_id |
| PRIMARY | yes | tenant_id, execution_id, sequence_number |
| uq_execution_event_id | yes | event_id |

| Foreign key | Columns | References |
|---|---|---|
| fk_execution_event_execution | execution_id | workflow_executions.id |
| fk_execution_event_tenant | tenant_id | tenants.id |

### execution_outbox

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| execution_id | binary(16) | NO | ∅ |  |
| node_execution_id | binary(16) | YES | ∅ |  |
| attempt_id | binary(16) | YES | ∅ |  |
| message_type | enum('dispatch_node','runtime_event','resume','cancel') | NO | ∅ |  |
| capability | varchar(64) | YES | ∅ |  |
| payload_json | json | NO | ∅ |  |
| status | enum('pending','published','failed') | NO | pending |  |
| available_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |
| locked_by | binary(16) | YES | ∅ |  |
| locked_until | timestamp(6) | YES | ∅ |  |
| attempt_count | int unsigned | NO | 0 |  |
| published_at | timestamp(6) | YES | ∅ |  |
| last_error | varchar(1000) | YES | ∅ |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |

| Index | Unique | Columns |
|---|---:|---|
| fk_execution_outbox_attempt | no | attempt_id |
| fk_execution_outbox_execution | no | execution_id |
| fk_execution_outbox_node | no | node_execution_id |
| fk_execution_outbox_tenant | no | tenant_id |
| idx_execution_outbox_pending | no | status, available_at, locked_until, created_at |
| PRIMARY | yes | id |

| Foreign key | Columns | References |
|---|---|---|
| fk_execution_outbox_attempt | attempt_id | node_attempts.id |
| fk_execution_outbox_execution | execution_id | workflow_executions.id |
| fk_execution_outbox_node | node_execution_id | node_executions.id |
| fk_execution_outbox_tenant | tenant_id | tenants.id |

### execution_resume_tokens

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| execution_id | binary(16) | NO | ∅ |  |
| node_execution_id | binary(16) | NO | ∅ |  |
| token_hash | char(64) | NO | ∅ |  |
| resume_kind | enum('time','webhook','form','approval') | NO | ∅ |  |
| status | enum('active','used','expired','cancelled') | NO | active |  |
| idempotency_key | varchar(192) | YES | ∅ |  |
| expires_at | timestamp(6) | YES | ∅ |  |
| used_at | timestamp(6) | YES | ∅ |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |

| Index | Unique | Columns |
|---|---:|---|
| fk_resume_token_execution | no | execution_id |
| fk_resume_token_tenant | no | tenant_id |
| idx_resume_token_expiry | no | status, expires_at |
| PRIMARY | yes | id |
| uq_resume_node_kind | yes | node_execution_id, resume_kind |
| uq_resume_token_hash | yes | token_hash |

| Foreign key | Columns | References |
|---|---|---|
| fk_resume_token_execution | execution_id | workflow_executions.id |
| fk_resume_token_node | node_execution_id | node_executions.id |
| fk_resume_token_tenant | tenant_id | tenants.id |

### execution_snapshots

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| execution_id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| workflow_version_id | binary(16) | YES | ∅ |  |
| definition_json | json | NO | ∅ |  |
| compiled_ir_json | json | NO | ∅ |  |
| compiled_ir_hash | varchar(255) | NO | ∅ |  |
| compiler_version | varchar(64) | NO | ∅ |  |
| manifest_snapshot_json | json | YES | ∅ |  |
| debug_plan_json | json | YES | ∅ |  |
| debug_overlay_snapshot_json | json | YES | ∅ |  |
| resource_snapshot_json | json | NO | ∅ |  |
| runtime_settings_json | json | NO | ∅ |  |
| state_hash | varchar(96) | NO | ∅ |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |

| Index | Unique | Columns |
|---|---:|---|
| fk_execution_snapshot_tenant | no | tenant_id |
| fk_execution_snapshot_version_m6 | no | workflow_version_id |
| PRIMARY | yes | execution_id |

| Foreign key | Columns | References |
|---|---|---|
| fk_execution_snapshot_execution | execution_id | workflow_executions.id |
| fk_execution_snapshot_tenant | tenant_id | tenants.id |
| fk_execution_snapshot_version_m6 | workflow_version_id | workflow_versions.id |

### idempotency_records

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| tenant_id | binary(16) | NO | ∅ |  |
| operation_key | varchar(128) | NO | ∅ |  |
| idempotency_key | varchar(128) | NO | ∅ |  |
| request_hash | char(64) | NO | ∅ |  |
| resource_id | binary(16) | YES | ∅ |  |
| response_json | json | YES | ∅ |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |
| expires_at | timestamp(6) | NO | ∅ |  |

| Index | Unique | Columns |
|---|---:|---|
| idx_idempotency_expiry | no | expires_at |
| PRIMARY | yes | tenant_id, operation_key, idempotency_key |

| Foreign key | Columns | References |
|---|---|---|
| d | e | m.p |
| k | _ | i.d |
| e | n | a.n |
| e | n | a.n |
| d |  | . |

### invocation_events

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| tenant_id | binary(16) | NO | ∅ |  |
| invocation_id | binary(16) | NO | ∅ |  |
| sequence_number | bigint unsigned | NO | ∅ |  |
| event_type | varchar(64) | NO | ∅ |  |
| payload_json | json | NO | ∅ |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |

| Index | Unique | Columns |
|---|---:|---|
| fk_invocation_event_invocation | no | invocation_id |
| PRIMARY | yes | tenant_id, invocation_id, sequence_number |

| Foreign key | Columns | References |
|---|---|---|
| fk_invocation_event_invocation | invocation_id | application_invocations.id |
| fk_invocation_event_tenant | tenant_id | tenants.id |

### item_lineage

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| tenant_id | binary(16) | NO | ∅ |  |
| execution_id | binary(16) | NO | ∅ |  |
| delivery_id | binary(16) | NO | ∅ |  |
| target_item_index | int unsigned | NO | ∅ |  |
| source_node_execution_id | binary(16) | NO | ∅ |  |
| source_run_index | int unsigned | NO | ∅ |  |
| source_output_index | int unsigned | NO | ∅ |  |
| source_item_index | int unsigned | NO | ∅ |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |

| Index | Unique | Columns |
|---|---:|---|
| fk_item_lineage_execution | no | execution_id |
| fk_item_lineage_source | no | source_node_execution_id |
| idx_item_lineage_execution | no | tenant_id, execution_id, source_node_execution_id |
| PRIMARY | yes | delivery_id, target_item_index, source_node_execution_id, source_output_index, source_item_index |

| Foreign key | Columns | References |
|---|---|---|
| fk_item_lineage_delivery | delivery_id | execution_edge_deliveries.id |
| fk_item_lineage_execution | execution_id | workflow_executions.id |
| fk_item_lineage_source | source_node_execution_id | node_executions.id |
| fk_item_lineage_tenant | tenant_id | tenants.id |

### mcp_discovery_runs

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| server_id | binary(16) | NO | ∅ |  |
| server_version_id | binary(16) | NO | ∅ |  |
| status | enum('running','succeeded','failed') | NO | ∅ |  |
| discovered_count | int unsigned | NO | 0 |  |
| error_code | varchar(128) | YES | ∅ |  |
| error_message | varchar(512) | YES | ∅ |  |
| started_by | binary(16) | NO | ∅ |  |
| started_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |
| finished_at | timestamp(6) | YES | ∅ |  |

| Index | Unique | Columns |
|---|---:|---|
| fk_mcp_discovery_server | no | server_id |
| fk_mcp_discovery_user | no | started_by |
| fk_mcp_discovery_version | no | server_version_id |
| idx_mcp_discovery_server | no | tenant_id, server_id, started_at |
| PRIMARY | yes | id |

| Foreign key | Columns | References |
|---|---|---|
| fk_mcp_discovery_server | server_id | mcp_servers.id |
| fk_mcp_discovery_tenant | tenant_id | tenants.id |
| fk_mcp_discovery_user | started_by | users.id |
| fk_mcp_discovery_version | server_version_id | mcp_server_versions.id |

### mcp_server_versions

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| server_id | binary(16) | NO | ∅ |  |
| version_number | bigint unsigned | NO | ∅ |  |
| transport | enum('streamable_http','sse') | NO | ∅ |  |
| endpoint | varchar(2048) | NO | ∅ |  |
| credential_id | binary(16) | YES | ∅ |  |
| configuration_json | json | NO | ∅ |  |
| configuration_hash | char(64) | NO | ∅ |  |
| created_by | binary(16) | NO | ∅ |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |

| Index | Unique | Columns |
|---|---:|---|
| fk_mcp_server_version_credential | no | credential_id |
| fk_mcp_server_version_server | no | server_id |
| fk_mcp_server_version_user | no | created_by |
| PRIMARY | yes | id |
| uq_mcp_server_version | yes | tenant_id, server_id, version_number |
| uq_mcp_server_version_hash | yes | tenant_id, server_id, configuration_hash |

| Foreign key | Columns | References |
|---|---|---|
| fk_mcp_server_version_credential | credential_id | credentials.id |
| fk_mcp_server_version_server | server_id | mcp_servers.id |
| fk_mcp_server_version_tenant | tenant_id | tenants.id |
| fk_mcp_server_version_user | created_by | users.id |

### mcp_servers

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| name | varchar(160) | NO | ∅ |  |
| description | varchar(1000) | YES | ∅ |  |
| owner_department_id | binary(16) | NO | ∅ |  |
| current_version_number | bigint unsigned | NO | 1 |  |
| status | enum('active','disabled') | NO | active |  |
| version | bigint unsigned | NO | 1 |  |
| last_discovered_at | timestamp(6) | YES | ∅ |  |
| created_by | binary(16) | NO | ∅ |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |
| updated_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED on update CURRENT_TIMESTAMP(6) |

| Index | Unique | Columns |
|---|---:|---|
| fk_mcp_server_department | no | owner_department_id |
| fk_mcp_server_user | no | created_by |
| idx_mcp_servers_tenant | no | tenant_id, status, updated_at |
| PRIMARY | yes | id |
| uq_mcp_server_name | yes | tenant_id, name |

| Foreign key | Columns | References |
|---|---|---|
| fk_mcp_server_department | owner_department_id | departments.id |
| fk_mcp_server_tenant | tenant_id | tenants.id |
| fk_mcp_server_user | created_by | users.id |

### mcp_tool_policies

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| tenant_id | binary(16) | NO | ∅ |  |
| tool_id | binary(16) | NO | ∅ |  |
| enabled | tinyint(1) | NO | 1 |  |
| debug_enabled | tinyint(1) | NO | 1 |  |
| timeout_seconds | int unsigned | NO | 30 |  |
| side_effect | enum('unknown','none','read_only','idempotent','non_idempotent','irreversible') | NO | unknown |  |
| version | bigint unsigned | NO | 1 |  |
| updated_by | binary(16) | NO | ∅ |  |
| updated_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED on update CURRENT_TIMESTAMP(6) |

| Index | Unique | Columns |
|---|---:|---|
| fk_mcp_policy_tool | no | tool_id |
| fk_mcp_policy_user | no | updated_by |
| PRIMARY | yes | tenant_id, tool_id |

| Foreign key | Columns | References |
|---|---|---|
| fk_mcp_policy_tenant | tenant_id | tenants.id |
| fk_mcp_policy_tool | tool_id | mcp_tools.id |
| fk_mcp_policy_user | updated_by | users.id |

### mcp_tool_versions

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| tool_id | binary(16) | NO | ∅ |  |
| discovery_run_id | binary(16) | NO | ∅ |  |
| version_number | bigint unsigned | NO | ∅ |  |
| input_schema | json | NO | ∅ |  |
| output_schema | json | YES | ∅ |  |
| annotations_json | json | NO | ∅ |  |
| schema_hash | char(64) | NO | ∅ |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |

| Index | Unique | Columns |
|---|---:|---|
| fk_mcp_tool_version_discovery | no | discovery_run_id |
| fk_mcp_tool_version_tool | no | tool_id |
| PRIMARY | yes | id |
| uq_mcp_tool_schema_hash | yes | tenant_id, tool_id, schema_hash |
| uq_mcp_tool_version | yes | tenant_id, tool_id, version_number |

| Foreign key | Columns | References |
|---|---|---|
| fk_mcp_tool_version_discovery | discovery_run_id | mcp_discovery_runs.id |
| fk_mcp_tool_version_tenant | tenant_id | tenants.id |
| fk_mcp_tool_version_tool | tool_id | mcp_tools.id |

### mcp_tools

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| server_id | binary(16) | NO | ∅ |  |
| name | varchar(255) | NO | ∅ |  |
| title | varchar(255) | YES | ∅ |  |
| description | text | YES | ∅ |  |
| current_version_number | bigint unsigned | NO | ∅ |  |
| availability | enum('available','unavailable') | NO | available |  |
| version | bigint unsigned | NO | 1 |  |
| last_seen_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |
| updated_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED on update CURRENT_TIMESTAMP(6) |

| Index | Unique | Columns |
|---|---:|---|
| fk_mcp_tool_server | no | server_id |
| idx_mcp_tools_server | no | tenant_id, server_id, availability |
| PRIMARY | yes | id |
| uq_mcp_tool_name | yes | tenant_id, server_id, name |

| Foreign key | Columns | References |
|---|---|---|
| fk_mcp_tool_server | server_id | mcp_servers.id |
| fk_mcp_tool_tenant | tenant_id | tenants.id |

### memory_connections

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| name | varchar(160) | NO | ∅ |  |
| endpoint | varchar(2048) | NO | ∅ |  |
| health_path | varchar(512) | NO | /health |  |
| credential_id | binary(16) | YES | ∅ |  |
| owner_department_id | binary(16) | NO | ∅ |  |
| configuration_json | json | NO | ∅ |  |
| status | enum('active','disabled') | NO | active |  |
| version | bigint unsigned | NO | 1 |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |
| updated_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED on update CURRENT_TIMESTAMP(6) |

| Index | Unique | Columns |
|---|---:|---|
| fk_memory_connection_credential | no | credential_id |
| fk_memory_connection_department | no | owner_department_id |
| idx_memory_connections_tenant | no | tenant_id, status, updated_at |
| PRIMARY | yes | id |

| Foreign key | Columns | References |
|---|---|---|
| fk_memory_connection_credential | credential_id | credentials.id |
| fk_memory_connection_department | owner_department_id | departments.id |
| fk_memory_connection_tenant | tenant_id | tenants.id |

### memory_namespaces

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| connection_id | binary(16) | NO | ∅ |  |
| name | varchar(160) | NO | ∅ |  |
| external_namespace | varchar(512) | NO | ∅ |  |
| access_mode | enum('read','read_write') | NO | read |  |
| owner_department_id | binary(16) | NO | ∅ |  |
| status | enum('active','disabled') | NO | active |  |
| version | bigint unsigned | NO | 1 |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |
| updated_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED on update CURRENT_TIMESTAMP(6) |

| Index | Unique | Columns |
|---|---:|---|
| fk_memory_namespace_connection | no | connection_id |
| fk_memory_namespace_department | no | owner_department_id |
| idx_memory_namespaces_department | no | tenant_id, owner_department_id, status |
| PRIMARY | yes | id |
| uq_memory_namespace_external | yes | tenant_id, connection_id, external_namespace |

| Foreign key | Columns | References |
|---|---|---|
| fk_memory_namespace_connection | connection_id | memory_connections.id |
| fk_memory_namespace_department | owner_department_id | departments.id |
| fk_memory_namespace_tenant | tenant_id | tenants.id |

### model_alias_deployment_history

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| alias_id | binary(16) | NO | ∅ |  |
| previous_deployment_id | binary(16) | YES | ∅ |  |
| deployment_id | binary(16) | NO | ∅ |  |
| changed_by | binary(16) | NO | ∅ |  |
| changed_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |

| Index | Unique | Columns |
|---|---:|---|
| fk_model_alias_history_alias | no | alias_id |
| fk_model_alias_history_deployment | no | deployment_id |
| fk_model_alias_history_previous | no | previous_deployment_id |
| fk_model_alias_history_user | no | changed_by |
| idx_model_alias_history | no | tenant_id, alias_id, changed_at |
| PRIMARY | yes | id |

| Foreign key | Columns | References |
|---|---|---|
| fk_model_alias_history_alias | alias_id | model_aliases.id |
| fk_model_alias_history_deployment | deployment_id | model_deployments.id |
| fk_model_alias_history_previous | previous_deployment_id | model_deployments.id |
| fk_model_alias_history_tenant | tenant_id | tenants.id |
| fk_model_alias_history_user | changed_by | users.id |

### model_aliases

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| alias | varchar(128) | NO | ∅ |  |
| deployment_id | binary(16) | NO | ∅ |  |
| status | enum('active','disabled') | NO | active |  |
| version | bigint unsigned | NO | 1 |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |
| updated_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED on update CURRENT_TIMESTAMP(6) |

| Index | Unique | Columns |
|---|---:|---|
| fk_model_alias_deployment | no | deployment_id |
| PRIMARY | yes | id |
| uq_model_alias | yes | tenant_id, alias |

| Foreign key | Columns | References |
|---|---|---|
| fk_model_alias_deployment | deployment_id | model_deployments.id |
| fk_model_alias_tenant | tenant_id | tenants.id |

### model_deployments

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| connection_name | varchar(160) | NO | ∅ |  |
| provider_type | enum('openai_compatible','custom_http') | NO | ∅ |  |
| endpoint | varchar(2048) | NO | ∅ |  |
| credential_id | binary(16) | YES | ∅ |  |
| owner_department_id | binary(16) | NO | ∅ |  |
| model_name | varchar(255) | NO | ∅ |  |
| max_input_tokens | bigint unsigned | NO | 1050000 |  |
| max_output_tokens | bigint unsigned | NO | 128000 |  |
| default_parameters | json | NO | ∅ |  |
| status | enum('active','disabled') | NO | active |  |
| revision_number | bigint unsigned | NO | 1 |  |
| supersedes_deployment_id | binary(16) | YES | ∅ |  |
| version | bigint unsigned | NO | 1 |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |
| updated_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED on update CURRENT_TIMESTAMP(6) |

| Index | Unique | Columns |
|---|---:|---|
| fk_model_deployment_credential | no | credential_id |
| fk_model_deployment_department | no | owner_department_id |
| fk_model_deployment_previous | no | supersedes_deployment_id |
| idx_model_deployments_credential | no | tenant_id, credential_id |
| idx_model_deployments_department | no | tenant_id, owner_department_id, status, updated_at |
| PRIMARY | yes | id |

| Foreign key | Columns | References |
|---|---|---|
| fk_model_deployment_credential | credential_id | credentials.id |
| fk_model_deployment_department | owner_department_id | departments.id |
| fk_model_deployment_previous | supersedes_deployment_id | model_deployments.id |
| fk_model_deployment_tenant | tenant_id | tenants.id |

### model_price_versions

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| deployment_id | binary(16) | NO | ∅ |  |
| version_number | bigint unsigned | NO | ∅ |  |
| currency | char(3) | NO | ∅ |  |
| input_per_million | decimal(20,8) | NO | ∅ |  |
| output_per_million | decimal(20,8) | NO | ∅ |  |
| created_by | binary(16) | NO | ∅ |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |

| Index | Unique | Columns |
|---|---:|---|
| fk_model_price_deployment | no | deployment_id |
| fk_model_price_user | no | created_by |
| PRIMARY | yes | id |
| uq_model_price_version | yes | tenant_id, deployment_id, version_number |

| Foreign key | Columns | References |
|---|---|---|
| fk_model_price_deployment | deployment_id | model_deployments.id |
| fk_model_price_tenant | tenant_id | tenants.id |
| fk_model_price_user | created_by | users.id |

### node_attempts

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| execution_id | binary(16) | NO | ∅ |  |
| node_execution_id | binary(16) | NO | ∅ |  |
| attempt_number | int unsigned | NO | ∅ |  |
| status | enum('queued','running','suspended','succeeded','failed','cancelled','timed_out','lease_expired') | NO | ∅ |  |
| idempotency_key | varchar(192) | NO | ∅ |  |
| lease_token | binary(16) | YES | ∅ |  |
| worker_instance_id | varchar(160) | YES | ∅ |  |
| deadline_at | timestamp(6) | YES | ∅ |  |
| input_json | json | YES | ∅ |  |
| output_json | json | YES | ∅ |  |
| log_artifact_id | binary(16) | YES | ∅ |  |
| error_code | varchar(128) | YES | ∅ |  |
| error_message | varchar(1000) | YES | ∅ |  |
| started_at | timestamp(6) | YES | ∅ |  |
| ended_at | timestamp(6) | YES | ∅ |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |

| Index | Unique | Columns |
|---|---:|---|
| fk_node_attempt_execution | no | execution_id |
| fk_node_attempt_log | no | log_artifact_id |
| idx_node_attempt_execution | no | tenant_id, execution_id, created_at |
| PRIMARY | yes | id |
| uq_node_attempt_idempotency | yes | tenant_id, idempotency_key |
| uq_node_attempt_number | yes | node_execution_id, attempt_number |

| Foreign key | Columns | References |
|---|---|---|
| fk_node_attempt_execution | execution_id | workflow_executions.id |
| fk_node_attempt_log | log_artifact_id | artifacts.id |
| fk_node_attempt_node_execution | node_execution_id | node_executions.id |
| fk_node_attempt_tenant | tenant_id | tenants.id |

### node_definition_versions

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| node_definition_id | binary(16) | NO | ∅ |  |
| version_number | int unsigned | NO | ∅ |  |
| protocol_version | varchar(32) | NO | ∅ |  |
| manifest_json | json | NO | ∅ |  |
| manifest_hash | varchar(96) | NO | ∅ |  |
| capability | varchar(64) | NO | ∅ |  |
| execution_style | enum('action','trigger','suspend','sub_workflow') | NO | ∅ |  |
| side_effect_level | enum('none','idempotent','reversible','irreversible') | NO | none |  |
| resume_policy | json | YES | ∅ |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |

| Index | Unique | Columns |
|---|---:|---|
| PRIMARY | yes | id |
| uq_node_definition_version | yes | node_definition_id, version_number |

| Foreign key | Columns | References |
|---|---|---|
| o | d | e._ |
| k | _ | n.o |
| o | d | e._ |
| o | d | e._ |
| d |  | . |

### node_definitions

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | YES | ∅ |  |
| node_type | varchar(128) | NO | ∅ |  |
| display_name | varchar(160) | NO | ∅ |  |
| source_type | enum('platform','remote','composite') | NO | ∅ |  |
| status | enum('active','disabled') | NO | active |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |
| updated_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED on update CURRENT_TIMESTAMP(6) |

| Index | Unique | Columns |
|---|---:|---|
| idx_node_definition_lookup | no | node_type, status |
| PRIMARY | yes | id |
| uq_node_definition | yes | tenant_id, node_type |

| Foreign key | Columns | References |
|---|---|---|
| o | d | e._ |
| k | _ | n.o |
| e | n | a.n |
| e | n | a.n |
| d |  | . |

### node_executions

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| execution_id | binary(16) | NO | ∅ |  |
| node_id | varchar(128) | NO | ∅ |  |
| node_key | varchar(128) | NO | ∅ |  |
| node_name | varchar(160) | NO | ∅ |  |
| node_type | varchar(128) | NO | ∅ |  |
| node_version | int unsigned | NO | ∅ |  |
| generation | int unsigned | NO | ∅ |  |
| activation_slot | int unsigned | NO | ∅ |  |
| run_index | int unsigned | NO | ∅ |  |
| iteration_index | int unsigned | NO | 0 |  |
| status | enum('ready','queued','running','waiting','succeeded','failed','skipped','cancelled','timed_out') | NO | ∅ |  |
| capability | varchar(64) | NO | ∅ |  |
| side_effect_level | enum('none','idempotent','reversible','irreversible') | NO | none |  |
| input_json | json | YES | ∅ |  |
| output_json | json | YES | ∅ |  |
| error_code | varchar(128) | YES | ∅ |  |
| error_message | varchar(1000) | YES | ∅ |  |
| started_at | timestamp(6) | YES | ∅ |  |
| ended_at | timestamp(6) | YES | ∅ |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |
| updated_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED on update CURRENT_TIMESTAMP(6) |

| Index | Unique | Columns |
|---|---:|---|
| idx_node_execution_key | no | tenant_id, execution_id, node_key, run_index |
| idx_node_execution_list | no | tenant_id, execution_id, run_index, created_at |
| idx_node_execution_ready | no | tenant_id, status, capability, created_at |
| PRIMARY | yes | id |
| uq_node_execution_activation | yes | execution_id, node_id, generation, activation_slot |

| Foreign key | Columns | References |
|---|---|---|
| fk_node_execution_execution | execution_id | workflow_executions.id |
| fk_node_execution_tenant | tenant_id | tenants.id |

### node_invocation_handles

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| execution_id | binary(16) | NO | ∅ |  |
| node_execution_id | binary(16) | NO | ∅ |  |
| attempt_id | binary(16) | NO | ∅ |  |
| lease_token | binary(16) | NO | ∅ |  |
| token_hash | char(64) | NO | ∅ |  |
| handle_kind | enum('credential','artifact','cancellation') | NO | ∅ |  |
| resource_id | binary(16) | YES | ∅ |  |
| resource_version | bigint unsigned | YES | ∅ |  |
| scope_json | json | NO | ∅ |  |
| sandbox_lease_id | binary(16) | YES | ∅ |  |
| expires_at | timestamp(6) | NO | ∅ |  |
| consumed_at | timestamp(6) | YES | ∅ |  |
| revoked_at | timestamp(6) | YES | ∅ |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |

| Index | Unique | Columns |
|---|---:|---|
| fk_node_invocation_handle_attempt | no | attempt_id |
| fk_node_invocation_handle_execution | no | execution_id |
| fk_node_invocation_handle_node | no | node_execution_id |
| idx_node_invocation_handle_attempt | no | tenant_id, attempt_id, handle_kind |
| idx_node_invocation_handle_expiry | no | expires_at, consumed_at |
| idx_node_invocation_handle_sandbox | no | sandbox_lease_id, revoked_at |
| PRIMARY | yes | id |
| uq_node_invocation_handle_token | yes | token_hash |

| Foreign key | Columns | References |
|---|---|---|
| fk_node_invocation_handle_attempt | attempt_id | node_attempts.id |
| fk_node_invocation_handle_execution | execution_id | workflow_executions.id |
| fk_node_invocation_handle_node | node_execution_id | node_executions.id |
| fk_node_invocation_handle_sandbox | sandbox_lease_id | sandbox_leases.id |
| fk_node_invocation_handle_tenant | tenant_id | tenants.id |

### notification_receipts

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| tenant_id | binary(16) | NO | ∅ |  |
| notification_id | binary(16) | NO | ∅ |  |
| user_id | binary(16) | NO | ∅ |  |
| read_at | timestamp(6) | YES | ∅ |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |

| Index | Unique | Columns |
|---|---:|---|
| fk_notification_receipt_notification | no | notification_id |
| fk_notification_receipt_user | no | user_id |
| idx_notification_inbox | no | tenant_id, user_id, read_at, created_at |
| PRIMARY | yes | tenant_id, notification_id, user_id |

| Foreign key | Columns | References |
|---|---|---|
| fk_notification_receipt_notification | notification_id | notifications.id |
| fk_notification_receipt_tenant | tenant_id | tenants.id |
| fk_notification_receipt_user | user_id | users.id |

### notifications

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| source_event_id | binary(16) | NO | ∅ |  |
| notification_type | varchar(64) | NO | ∅ |  |
| title_key | varchar(160) | NO | ∅ |  |
| body_key | varchar(160) | NO | ∅ |  |
| arguments_json | json | NO | ∅ |  |
| target_type | varchar(64) | NO | ∅ |  |
| target_id | binary(16) | NO | ∅ |  |
| target_path | varchar(512) | NO | ∅ |  |
| tone | enum('primary','success','warning','danger') | NO | primary |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |

| Index | Unique | Columns |
|---|---:|---|
| PRIMARY | yes | id |
| uq_notification_source | yes | tenant_id, source_event_id, notification_type |

| Foreign key | Columns | References |
|---|---|---|
| o | t | i.f |
| k | _ | n.o |
| e | n | a.n |
| e | n | a.n |
| d |  | . |

### outbox_events

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| event_type | varchar(128) | NO | ∅ |  |
| schema_version | varchar(16) | NO | 1.0 |  |
| aggregate_type | varchar(128) | NO | ∅ |  |
| aggregate_id | varchar(128) | NO | ∅ |  |
| execution_id | binary(16) | YES | ∅ |  |
| sequence_number | bigint unsigned | YES | ∅ |  |
| payload_json | json | NO | ∅ |  |
| occurred_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |
| available_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |
| published_at | timestamp(6) | YES | ∅ |  |
| attempt_count | int unsigned | NO | 0 |  |
| last_error | varchar(1024) | YES | ∅ |  |
| locked_by | binary(16) | YES | ∅ |  |
| locked_until | timestamp(6) | YES | ∅ |  |

| Index | Unique | Columns |
|---|---:|---|
| fk_outbox_execution | no | execution_id |
| idx_outbox_execution | no | tenant_id, execution_id, sequence_number |
| idx_outbox_lease | no | published_at, available_at, locked_until |
| idx_outbox_pending | no | published_at, available_at, occurred_at |
| idx_outbox_tenant_aggregate | no | tenant_id, aggregate_type, aggregate_id |
| PRIMARY | yes | id |

| Foreign key | Columns | References |
|---|---|---|
| fk_outbox_execution | execution_id | workflow_executions.id |
| fk_outbox_tenant | tenant_id | tenants.id |

### permissions

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| permission_key | varchar(96) | NO | ∅ |  |
| name | varchar(100) | NO | ∅ |  |
| description | varchar(500) | YES | ∅ |  |

| Index | Unique | Columns |
|---|---:|---|
| PRIMARY | yes | id |
| uq_permissions_key | yes | permission_key |

| Foreign key | Columns | References |
|---|---|---|
| — | — | — |

### projection_receipts

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| projector_name | varchar(128) | NO | ∅ |  |
| event_id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| processed_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |

| Index | Unique | Columns |
|---|---:|---|
| idx_projection_receipts_tenant | no | tenant_id, processed_at |
| PRIMARY | yes | projector_name, event_id |

| Foreign key | Columns | References |
|---|---|---|
| r | o | j.e |
| k | _ | p.r |
| e | n | a.n |
| e | n | a.n |
| d |  | . |

### quota_policies

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| tenant_id | binary(16) | NO | ∅ |  |
| dimension_key | varchar(64) | NO | ∅ |  |
| hard_limit | decimal(24,6) | NO | ∅ |  |
| period_seconds | bigint unsigned | YES | ∅ |  |
| version | bigint unsigned | NO | 1 |  |
| updated_by | binary(16) | NO | ∅ |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |
| updated_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED on update CURRENT_TIMESTAMP(6) |

| Index | Unique | Columns |
|---|---:|---|
| fk_quota_policy_user | no | updated_by |
| PRIMARY | yes | tenant_id, dimension_key |

| Foreign key | Columns | References |
|---|---|---|
| fk_quota_policy_tenant | tenant_id | tenants.id |
| fk_quota_policy_user | updated_by | users.id |

### quota_reservations

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| dimension_key | varchar(64) | NO | ∅ |  |
| scope_type | varchar(64) | NO | ∅ |  |
| scope_id | varchar(128) | NO | ∅ |  |
| amount | decimal(24,6) | NO | ∅ |  |
| status | enum('active','settled','released','expired') | NO | active |  |
| expires_at | timestamp(6) | NO | ∅ |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |
| settled_at | timestamp(6) | YES | ∅ |  |

| Index | Unique | Columns |
|---|---:|---|
| idx_quota_reservation_reaper | no | status, expires_at |
| PRIMARY | yes | id |
| uq_quota_reservation_scope | yes | tenant_id, dimension_key, scope_type, scope_id |

| Foreign key | Columns | References |
|---|---|---|
| u | o | t.a |
| k | _ | q.u |
| e | n | a.n |
| e | n | a.n |
| d |  | . |

### quota_usage_ledger

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| dimension_key | varchar(64) | NO | ∅ |  |
| scope_type | varchar(64) | NO | ∅ |  |
| scope_id | varchar(128) | NO | ∅ |  |
| amount | decimal(24,6) | NO | ∅ |  |
| idempotency_key | varchar(192) | NO | ∅ |  |
| occurred_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |

| Index | Unique | Columns |
|---|---:|---|
| idx_quota_usage_window | no | tenant_id, dimension_key, occurred_at |
| PRIMARY | yes | id |
| uq_quota_usage_idempotency | yes | tenant_id, idempotency_key |

| Foreign key | Columns | References |
|---|---|---|
| u | o | t.a |
| k | _ | q.u |
| e | n | a.n |
| e | n | a.n |
| d |  | . |

### rag_connections

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| name | varchar(160) | NO | ∅ |  |
| endpoint | varchar(2048) | NO | ∅ |  |
| health_path | varchar(512) | NO | /health |  |
| credential_id | binary(16) | YES | ∅ |  |
| owner_department_id | binary(16) | NO | ∅ |  |
| configuration_json | json | NO | ∅ |  |
| status | enum('active','disabled') | NO | active |  |
| version | bigint unsigned | NO | 1 |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |
| updated_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED on update CURRENT_TIMESTAMP(6) |

| Index | Unique | Columns |
|---|---:|---|
| fk_rag_connection_credential | no | credential_id |
| fk_rag_connection_department | no | owner_department_id |
| idx_rag_connections_tenant | no | tenant_id, status, updated_at |
| PRIMARY | yes | id |

| Foreign key | Columns | References |
|---|---|---|
| fk_rag_connection_credential | credential_id | credentials.id |
| fk_rag_connection_department | owner_department_id | departments.id |
| fk_rag_connection_tenant | tenant_id | tenants.id |

### rag_resources

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| connection_id | binary(16) | NO | ∅ |  |
| name | varchar(160) | NO | ∅ |  |
| external_resource_id | varchar(512) | NO | ∅ |  |
| owner_department_id | binary(16) | NO | ∅ |  |
| sync_status | enum('unknown','syncing','synced','failed') | NO | unknown |  |
| status | enum('active','disabled') | NO | active |  |
| version | bigint unsigned | NO | 1 |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |
| updated_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED on update CURRENT_TIMESTAMP(6) |

| Index | Unique | Columns |
|---|---:|---|
| fk_rag_resource_connection | no | connection_id |
| fk_rag_resource_department | no | owner_department_id |
| idx_rag_resources_department | no | tenant_id, owner_department_id, status |
| PRIMARY | yes | id |
| uq_rag_resource_external | yes | tenant_id, connection_id, external_resource_id |

| Foreign key | Columns | References |
|---|---|---|
| fk_rag_resource_connection | connection_id | rag_connections.id |
| fk_rag_resource_department | owner_department_id | departments.id |
| fk_rag_resource_tenant | tenant_id | tenants.id |

### refresh_sessions

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| user_id | binary(16) | NO | ∅ |  |
| token_family_id | binary(16) | NO | ∅ |  |
| jti_hash | char(64) | NO | ∅ |  |
| token_version | bigint unsigned | NO | ∅ |  |
| expires_at | timestamp(6) | NO | ∅ |  |
| rotated_at | timestamp(6) | YES | ∅ |  |
| revoked_at | timestamp(6) | YES | ∅ |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |

| Index | Unique | Columns |
|---|---:|---|
| fk_refresh_sessions_user | no | user_id |
| idx_refresh_sessions_family | no | token_family_id, revoked_at |
| idx_refresh_sessions_user | no | tenant_id, user_id, revoked_at |
| PRIMARY | yes | id |
| uq_refresh_sessions_jti_hash | yes | jti_hash |

| Foreign key | Columns | References |
|---|---|---|
| fk_refresh_sessions_tenant | tenant_id | tenants.id |
| fk_refresh_sessions_user | user_id | users.id |

### release_schema_contract

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| contract_name | varchar(64) | NO | ∅ |  |
| schema_version | varchar(32) | NO | ∅ |  |
| minimum_application_version | varchar(32) | NO | ∅ |  |
| applied_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |

| Index | Unique | Columns |
|---|---:|---|
| PRIMARY | yes | contract_name |

| Foreign key | Columns | References |
|---|---|---|
| — | — | — |

### resource_grant_request_items

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| request_id | binary(16) | NO | ∅ |  |
| resource_type | varchar(32) | NO | ∅ |  |
| resource_id | binary(16) | NO | ∅ |  |
| resource_version_id | binary(16) | YES | ∅ |  |
| operation_key | varchar(32) | NO | ∅ |  |
| required_by_resource_id | binary(16) | YES | ∅ |  |
| owner_department_id | binary(16) | NO | ∅ |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |

| Index | Unique | Columns |
|---|---:|---|
| fk_resource_grant_request_item_department | no | owner_department_id |
| idx_resource_grant_request_item_department | no | tenant_id, owner_department_id, request_id |
| PRIMARY | yes | id |
| uq_resource_grant_request_item | yes | request_id, resource_type, resource_id, operation_key |

| Foreign key | Columns | References |
|---|---|---|
| fk_resource_grant_request_item_department | owner_department_id | departments.id |
| fk_resource_grant_request_item_request | request_id | resource_grant_requests.id |
| fk_resource_grant_request_item_tenant | tenant_id | tenants.id |

### resource_grant_request_reviews

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| request_id | binary(16) | NO | ∅ |  |
| owner_department_id | binary(16) | NO | ∅ |  |
| status | enum('pending','approved','rejected') | NO | pending |  |
| reviewed_by | binary(16) | YES | ∅ |  |
| review_comment | varchar(1000) | YES | ∅ |  |
| reviewed_at | timestamp(6) | YES | ∅ |  |
| version | bigint unsigned | NO | 1 |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |
| updated_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED on update CURRENT_TIMESTAMP(6) |

| Index | Unique | Columns |
|---|---:|---|
| fk_resource_grant_request_review_department | no | owner_department_id |
| fk_resource_grant_request_review_user | no | reviewed_by |
| idx_resource_grant_request_review_inbox | no | tenant_id, owner_department_id, status, updated_at |
| PRIMARY | yes | id |
| uq_resource_grant_request_review | yes | request_id, owner_department_id |

| Foreign key | Columns | References |
|---|---|---|
| fk_resource_grant_request_review_department | owner_department_id | departments.id |
| fk_resource_grant_request_review_request | request_id | resource_grant_requests.id |
| fk_resource_grant_request_review_tenant | tenant_id | tenants.id |
| fk_resource_grant_request_review_user | reviewed_by | users.id |

### resource_grant_requests

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| subject_type | enum('department','workflow_service_identity') | NO | ∅ |  |
| subject_id | binary(16) | NO | ∅ |  |
| workflow_id | binary(16) | YES | ∅ |  |
| workflow_service_identity_id | binary(16) | YES | ∅ |  |
| primary_resource_type | varchar(32) | NO | ∅ |  |
| primary_resource_id | binary(16) | NO | ∅ |  |
| primary_resource_version_id | binary(16) | YES | ∅ |  |
| operation_key | varchar(32) | NO | ∅ |  |
| source_node_id | varchar(128) | YES | ∅ |  |
| source_revision | bigint unsigned | YES | ∅ |  |
| request_message | varchar(1000) | YES | ∅ |  |
| dependency_hash | varchar(80) | NO | ∅ |  |
| open_dedupe_key | varchar(80) | YES | ∅ |  |
| status | enum('pending','approved','rejected','cancelled','stale') | NO | pending |  |
| requested_by | binary(16) | NO | ∅ |  |
| resolved_at | timestamp(6) | YES | ∅ |  |
| version | bigint unsigned | NO | 1 |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |
| updated_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED on update CURRENT_TIMESTAMP(6) |

| Index | Unique | Columns |
|---|---:|---|
| idx_resource_grant_request_inbox | no | tenant_id, status, updated_at |
| idx_resource_grant_request_subject | no | tenant_id, subject_type, subject_id, status |
| idx_resource_grant_request_workflow | no | tenant_id, workflow_id, status |
| PRIMARY | yes | id |
| uq_resource_grant_request_open | yes | tenant_id, open_dedupe_key |

`subject_type + subject_id` 是申请授权主体；Workflow 两列只在 `workflow_service_identity` Subject 下存在，并由 Check Constraint 保证一致。当前初始 Schema 不为多态 Subject 建立跨表外键。

### resource_grants

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| subject_type | enum('department','workflow_service_identity') | NO | ∅ |  |
| subject_id | binary(16) | NO | ∅ |  |
| resource_type | varchar(32) | NO | ∅ |  |
| resource_id | binary(16) | NO | ∅ |  |
| resource_version_id | binary(16) | YES | ∅ |  |
| operation_key | enum('view','use','read','write','manage') | NO | ∅ |  |
| created_by | binary(16) | NO | ∅ |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |

| Index | Unique | Columns |
|---|---:|---|
| fk_resource_grant_user | no | created_by |
| idx_resource_grant_resource | no | tenant_id, resource_type, resource_id |
| idx_resource_grant_subject | no | tenant_id, subject_type, subject_id |
| PRIMARY | yes | id |
| uq_resource_grant | yes | tenant_id, subject_type, subject_id, resource_type, resource_id, operation_key |

| Foreign key | Columns | References |
|---|---|---|
| fk_resource_grant_tenant | tenant_id | tenants.id |
| fk_resource_grant_user | created_by | users.id |

### resource_health_checks

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| resource_type | varchar(32) | NO | ∅ |  |
| resource_id | binary(16) | NO | ∅ |  |
| check_sequence | bigint unsigned | NO | ∅ |  |
| status | enum('healthy','unhealthy','unsupported') | NO | ∅ |  |
| latency_ms | bigint unsigned | YES | ∅ |  |
| error_code | varchar(128) | YES | ∅ |  |
| error_message | varchar(512) | YES | ∅ |  |
| checked_by | binary(16) | NO | ∅ |  |
| checked_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |

| Index | Unique | Columns |
|---|---:|---|
| fk_resource_health_user | no | checked_by |
| idx_resource_health_latest | no | tenant_id, resource_type, resource_id, checked_at |
| PRIMARY | yes | id |
| uq_resource_health_sequence | yes | tenant_id, resource_type, resource_id, check_sequence |

| Foreign key | Columns | References |
|---|---|---|
| fk_resource_health_tenant | tenant_id | tenants.id |
| fk_resource_health_user | checked_by | users.id |

### resume_webhook_bindings

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| wait_subscription_id | binary(16) | NO | ∅ |  |
| path_token_hash | char(64) | NO | ∅ |  |
| http_method | enum('GET','POST') | NO | POST |  |
| authentication_config_hash | char(64) | YES | ∅ |  |
| status | enum('active','used','expired','cancelled') | NO | active |  |
| expires_at | timestamp(6) | YES | ∅ |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |

| Index | Unique | Columns |
|---|---:|---|
| fk_resume_webhook_tenant | no | tenant_id |
| fk_resume_webhook_wait | no | wait_subscription_id |
| PRIMARY | yes | id |
| uq_resume_webhook_path | yes | path_token_hash |

| Foreign key | Columns | References |
|---|---|---|
| fk_resume_webhook_tenant | tenant_id | tenants.id |
| fk_resume_webhook_wait | wait_subscription_id | wait_subscriptions.id |

### retention_items

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| retention_run_id | binary(16) | NO | ∅ |  |
| data_type | varchar(64) | NO | ∅ |  |
| target_id | varchar(128) | NO | ∅ |  |
| status | enum('candidate','blocked','deleted','failed') | NO | ∅ |  |
| reason | varchar(255) | YES | ∅ |  |
| attempt_count | int unsigned | NO | 0 |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |
| updated_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED on update CURRENT_TIMESTAMP(6) |

| Index | Unique | Columns |
|---|---:|---|
| idx_retention_item_status | no | tenant_id, retention_run_id, status |
| PRIMARY | yes | id |
| uq_retention_item_target | yes | retention_run_id, data_type, target_id |

| Foreign key | Columns | References |
|---|---|---|
| fk_retention_item_run | retention_run_id | retention_runs.id |
| fk_retention_item_tenant | tenant_id | tenants.id |

### retention_policies

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| tenant_id | binary(16) | NO | ∅ |  |
| data_type | varchar(64) | NO | ∅ |  |
| retention_days | int unsigned | NO | ∅ |  |
| enabled | tinyint(1) | NO | 1 |  |
| version | bigint unsigned | NO | 1 |  |
| updated_by | binary(16) | NO | ∅ |  |
| updated_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED on update CURRENT_TIMESTAMP(6) |

| Index | Unique | Columns |
|---|---:|---|
| fk_retention_policy_user | no | updated_by |
| PRIMARY | yes | tenant_id, data_type |

| Foreign key | Columns | References |
|---|---|---|
| fk_retention_policy_tenant | tenant_id | tenants.id |
| fk_retention_policy_user | updated_by | users.id |

### retention_runs

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| dry_run | tinyint(1) | NO | ∅ |  |
| status | enum('queued','running','completed','failed','cancelled') | NO | queued |  |
| requested_by | binary(16) | NO | ∅ |  |
| candidate_count | bigint unsigned | NO | 0 |  |
| deleted_count | bigint unsigned | NO | 0 |  |
| error_message | varchar(1000) | YES | ∅ |  |
| attempt_count | int unsigned | NO | 0 |  |
| available_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |
| locked_by | binary(16) | YES | ∅ |  |
| locked_until | timestamp(6) | YES | ∅ |  |
| started_at | timestamp(6) | YES | ∅ |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |
| completed_at | timestamp(6) | YES | ∅ |  |

| Index | Unique | Columns |
|---|---:|---|
| fk_retention_run_tenant | no | tenant_id |
| fk_retention_run_user | no | requested_by |
| idx_retention_run_status | no | status, available_at, locked_until, created_at |
| PRIMARY | yes | id |

| Foreign key | Columns | References |
|---|---|---|
| fk_retention_run_tenant | tenant_id | tenants.id |
| fk_retention_run_user | requested_by | users.id |

### role_permissions

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| tenant_id | binary(16) | NO | ∅ |  |
| role_id | binary(16) | NO | ∅ |  |
| permission_id | binary(16) | NO | ∅ |  |

| Index | Unique | Columns |
|---|---:|---|
| fk_role_permissions_permission | no | permission_id |
| idx_role_permissions_tenant | no | tenant_id, role_id |
| PRIMARY | yes | role_id, permission_id |

| Foreign key | Columns | References |
|---|---|---|
| fk_role_permissions_permission | permission_id | permissions.id |
| fk_role_permissions_role | role_id | roles.id |
| fk_role_permissions_tenant | tenant_id | tenants.id |

### roles

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| code | varchar(64) | NO | ∅ |  |
| name | varchar(100) | NO | ∅ |  |
| description | varchar(500) | YES | ∅ |  |
| data_scope | enum('company','department_tree','own') | NO | ∅ |  |
| is_builtin | tinyint(1) | NO | 0 |  |
| status | enum('active','disabled') | NO | active |  |
| version | bigint unsigned | NO | 1 |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |
| updated_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED on update CURRENT_TIMESTAMP(6) |

| Index | Unique | Columns |
|---|---:|---|
| PRIMARY | yes | id |
| uq_roles_tenant_code | yes | tenant_id, code |

| Foreign key | Columns | References |
|---|---|---|
| o | l | e.s |
| k | _ | r.o |
| e | n | a.n |
| e | n | a.n |
| d |  | . |

### runtime_calls

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| execution_id | binary(16) | NO | ∅ |  |
| node_execution_id | binary(16) | NO | ∅ |  |
| attempt_id | binary(16) | NO | ∅ |  |
| agent_run_id | binary(16) | YES | ∅ |  |
| iteration_index | int unsigned | NO | 0 |  |
| call_index | int unsigned | NO | ∅ |  |
| call_kind | enum('model','mcp_tool','rag','memory','sandbox') | NO | ∅ |  |
| idempotency_key | varchar(192) | NO | ∅ |  |
| request_fingerprint | char(64) | NO | ∅ |  |
| resource_type | varchar(32) | YES | ∅ |  |
| resource_id | binary(16) | YES | ∅ |  |
| resource_version_id | binary(16) | YES | ∅ |  |
| side_effect | varchar(32) | NO | none |  |
| status | enum('reserved','sent','succeeded','failed','cancelled','unknown') | NO | ∅ |  |
| reserved_input_tokens | bigint unsigned | NO | 0 |  |
| reserved_output_tokens | bigint unsigned | NO | 0 |  |
| reserved_cost_micros | bigint unsigned | NO | 0 |  |
| input_tokens | bigint unsigned | NO | 0 |  |
| output_tokens | bigint unsigned | NO | 0 |  |
| cost_micros | bigint unsigned | NO | 0 |  |
| usage_estimated | tinyint(1) | NO | 0 |  |
| response_artifact_id | binary(16) | YES | ∅ |  |
| error_code | varchar(128) | YES | ∅ |  |
| error_message | varchar(1000) | YES | ∅ |  |
| started_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |
| ended_at | timestamp(6) | YES | ∅ |  |

| Index | Unique | Columns |
|---|---:|---|
| fk_runtime_call_agent | no | agent_run_id |
| fk_runtime_call_attempt | no | attempt_id |
| fk_runtime_call_execution | no | execution_id |
| fk_runtime_call_node | no | node_execution_id |
| fk_runtime_call_response | no | response_artifact_id |
| idx_runtime_call_node | no | tenant_id, node_execution_id, iteration_index, call_index |
| idx_runtime_call_resource | no | tenant_id, resource_type, resource_id, started_at |
| PRIMARY | yes | id |
| uq_runtime_call_idempotency | yes | tenant_id, idempotency_key |

| Foreign key | Columns | References |
|---|---|---|
| fk_runtime_call_agent | agent_run_id | agent_runs.id |
| fk_runtime_call_attempt | attempt_id | node_attempts.id |
| fk_runtime_call_execution | execution_id | workflow_executions.id |
| fk_runtime_call_node | node_execution_id | node_executions.id |
| fk_runtime_call_response | response_artifact_id | artifacts.id |
| fk_runtime_call_tenant | tenant_id | tenants.id |

### runtime_commands

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| command_type | varchar(64) | NO | ∅ |  |
| aggregate_type | varchar(64) | NO | ∅ |  |
| aggregate_id | varchar(128) | NO | ∅ |  |
| idempotency_key | varchar(192) | NO | ∅ |  |
| payload_json | json | NO | ∅ |  |
| status | enum('pending','processing','completed','failed') | NO | pending |  |
| available_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |
| locked_by | binary(16) | YES | ∅ |  |
| locked_until | timestamp(6) | YES | ∅ |  |
| attempt_count | int unsigned | NO | 0 |  |
| result_json | json | YES | ∅ |  |
| error_code | varchar(128) | YES | ∅ |  |
| error_message | varchar(1000) | YES | ∅ |  |
| completed_at | timestamp(6) | YES | ∅ |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |
| updated_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED on update CURRENT_TIMESTAMP(6) |

| Index | Unique | Columns |
|---|---:|---|
| idx_runtime_commands_aggregate | no | tenant_id, aggregate_type, aggregate_id, created_at |
| idx_runtime_commands_pending | no | status, available_at, locked_until, created_at |
| PRIMARY | yes | id |
| uq_runtime_command_idempotency | yes | tenant_id, command_type, idempotency_key |

| Foreign key | Columns | References |
|---|---|---|
| u | n | t.i |
| k | _ | r.u |
| e | n | a.n |
| e | n | a.n |
| d |  | . |

### runtime_idempotency_keys

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| tenant_id | binary(16) | NO | ∅ |  |
| scope | varchar(96) | NO | ∅ |  |
| idempotency_key | varchar(192) | NO | ∅ |  |
| request_hash | varchar(96) | NO | ∅ |  |
| status | enum('processing','completed','failed') | NO | ∅ |  |
| response_json | json | YES | ∅ |  |
| expires_at | timestamp(6) | NO | ∅ |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |
| updated_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED on update CURRENT_TIMESTAMP(6) |

| Index | Unique | Columns |
|---|---:|---|
| idx_runtime_idempotency_expiry | no | expires_at |
| PRIMARY | yes | tenant_id, scope, idempotency_key |

| Foreign key | Columns | References |
|---|---|---|
| u | n | t.i |
| k | _ | r.u |
| e | n | a.n |
| e | n | a.n |
| d |  | . |

### runtime_service_heartbeats

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| tenant_id | binary(16) | NO | ∅ |  |
| service_type | enum('coordinator','worker','sandbox','trace_writer') | NO | ∅ |  |
| instance_id | varchar(160) | NO | ∅ |  |
| status | enum('ready','degraded','unavailable') | NO | ∅ |  |
| detail_json | json | NO | ∅ |  |
| heartbeat_at | timestamp(6) | NO | ∅ |  |

| Index | Unique | Columns |
|---|---:|---|
| idx_runtime_heartbeat | no | tenant_id, service_type, heartbeat_at |
| PRIMARY | yes | tenant_id, service_type, instance_id |

| Foreign key | Columns | References |
|---|---|---|
| u | n | t.i |
| k | _ | r.u |
| e | n | a.n |
| e | n | a.n |
| d |  | . |

### sandbox_leases

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| execution_id | binary(16) | NO | ∅ |  |
| node_execution_id | binary(16) | NO | ∅ |  |
| attempt_id | binary(16) | NO | ∅ |  |
| worker_lease_token | binary(16) | NO | ∅ |  |
| sandbox_id | varchar(255) | YES | ∅ |  |
| lease_token_hash | char(64) | NO | ∅ |  |
| profile_version_id | binary(16) | NO | ∅ |  |
| idempotency_key | varchar(192) | NO | ∅ |  |
| status | enum('creating','ready','running','interrupting','terminating','terminated','orphaned','failed') | NO | ∅ |  |
| endpoint_auth_key_id | varchar(64) | YES | ∅ |  |
| endpoint_auth_nonce | varbinary(32) | YES | ∅ |  |
| endpoint_auth_ciphertext | mediumblob | YES | ∅ |  |
| expires_at | timestamp(6) | NO | ∅ |  |
| heartbeat_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |
| termination_attempts | int unsigned | NO | 0 |  |
| last_error | varchar(1000) | YES | ∅ |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |
| terminated_at | timestamp(6) | YES | ∅ |  |

| Index | Unique | Columns |
|---|---:|---|
| fk_sandbox_lease_attempt | no | attempt_id |
| fk_sandbox_lease_execution | no | execution_id |
| fk_sandbox_lease_node | no | node_execution_id |
| fk_sandbox_lease_profile | no | profile_version_id |
| idx_sandbox_lease_attempt | no | tenant_id, attempt_id |
| idx_sandbox_lease_reaper | no | status, expires_at, heartbeat_at |
| PRIMARY | yes | id |
| uq_sandbox_lease_idempotency | yes | tenant_id, idempotency_key |
| uq_sandbox_lease_token | yes | lease_token_hash |

| Foreign key | Columns | References |
|---|---|---|
| fk_sandbox_lease_attempt | attempt_id | node_attempts.id |
| fk_sandbox_lease_execution | execution_id | workflow_executions.id |
| fk_sandbox_lease_node | node_execution_id | node_executions.id |
| fk_sandbox_lease_profile | profile_version_id | sandbox_profile_versions.id |
| fk_sandbox_lease_tenant | tenant_id | tenants.id |

### sandbox_process_frames

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| process_session_id | binary(16) | NO | ∅ |  |
| sequence | bigint unsigned | NO | ∅ |  |
| stream | enum('stdout','stderr') | NO | ∅ |  |
| payload_json | json | YES | ∅ |  |
| artifact_id | binary(16) | YES | ∅ |  |
| truncated | boolean | NO | FALSE |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |

| Index | Unique | Columns |
|---|---:|---|
| PRIMARY | yes | process_session_id, sequence |
| idx_sandbox_process_frame_artifact | no | artifact_id |

| Foreign key | Columns | References |
|---|---|---|
| none | ∅ | Process Session identity is validated by Sandbox Manager |

### sandbox_process_sessions

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| process_session_id | binary(16) | NO | ∅ |  |
| identity_hash | char(64) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| agent_run_id | binary(16) | NO | ∅ |  |
| mcp_server_version_id | binary(16) | NO | ∅ |  |
| sandbox_profile_version_id | binary(16) | NO | ∅ |  |
| profile_json | json | NO | ∅ |  |
| command_json | json | NO | ∅ |  |
| credential_refs_json | json | NO | ∅ |  |
| sandbox_id | varchar(255) | YES | ∅ |  |
| provider_operation_id | varchar(255) | YES | ∅ |  |
| provider_output_offset | bigint unsigned | NO | 0 |  |
| status | enum('acquiring','starting','running','interrupting','terminating','exited','failed','unknown_outcome','expired') | NO | ∅ |  |
| lease_id | binary(16) | NO | ∅ |  |
| attempt_id | binary(16) | NO | ∅ |  |
| worker_id | binary(16) | NO | ∅ |  |
| fencing_token | bigint unsigned | NO | ∅ |  |
| lease_expires_at | timestamp(6) | NO | ∅ |  |
| process_expires_at | timestamp(6) | NO | ∅ |  |
| exit_code | bigint | YES | ∅ |  |
| last_error | varchar(1000) | YES | ∅ |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |
| updated_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED on update CURRENT_TIMESTAMP(6) |

| Index | Unique | Columns |
|---|---:|---|
| PRIMARY | yes | process_session_id |
| uq_sandbox_process_identity | yes | identity_hash |
| idx_sandbox_process_reaper | no | status, process_expires_at, lease_expires_at |
| idx_sandbox_process_run | no | tenant_id, agent_run_id, mcp_server_version_id |

| Foreign key | Columns | References |
|---|---|---|
| none | ∅ | immutable Bundle and Attempt lease identities are validated by Sandbox Manager |

### sandbox_workspaces

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| workspace_id | binary(16) | NO | ∅ |  |
| identity_hash | char(64) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| application_id | binary(16) | YES | ∅ |  |
| session_key | varchar(255) | NO | ∅ |  |
| stable_agent_node_key | varchar(255) | NO | ∅ |  |
| profile_version_id | varchar(191) | NO | ∅ |  |
| profile_json | json | NO | ∅ |  |
| sandbox_id | varchar(255) | YES | ∅ |  |
| status | enum('creating','active','releasing','expired','unknown_outcome') | NO | creating |  |
| lease_id | binary(16) | YES | ∅ |  |
| attempt_id | binary(16) | YES | ∅ |  |
| worker_id | binary(16) | YES | ∅ |  |
| fencing_token | bigint unsigned | NO | 0 |  |
| lease_expires_at | timestamp(6) | YES | ∅ |  |
| workspace_expires_at | timestamp(6) | NO | ∅ |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |
| updated_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED on update CURRENT_TIMESTAMP(6) |

| Index | Unique | Columns |
|---|---:|---|
| PRIMARY | yes | workspace_id |
| uq_sandbox_workspace_identity | yes | identity_hash |
| idx_sandbox_workspace_reaper | no | status, workspace_expires_at, lease_expires_at |
| idx_sandbox_workspace_tenant | no | tenant_id, application_id, session_key |

| Foreign key | Columns | References |
|---|---|---|
| none | ∅ | immutable Bundle snapshot identities are validated by Sandbox Manager |

### sandbox_profile_versions

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| profile_id | binary(16) | NO | ∅ |  |
| version_number | bigint unsigned | NO | ∅ |  |
| runner | enum('python','javascript','shell','browser') | NO | ∅ |  |
| image_digest | varchar(255) | NO | ∅ |  |
| cpu_millis | int unsigned | NO | ∅ |  |
| memory_bytes | bigint unsigned | NO | ∅ |  |
| pids_limit | int unsigned | NO | ∅ |  |
| disk_bytes | bigint unsigned | NO | ∅ |  |
| timeout_seconds | int unsigned | NO | ∅ |  |
| output_limit_bytes | bigint unsigned | NO | ∅ |  |
| network_policy_json | json | NO | ∅ |  |
| configuration_hash | char(64) | NO | ∅ |  |
| created_by | binary(16) | NO | ∅ |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |

| Index | Unique | Columns |
|---|---:|---|
| fk_sandbox_profile_version_profile | no | profile_id |
| fk_sandbox_profile_version_user | no | created_by |
| PRIMARY | yes | id |
| uq_sandbox_profile_hash | yes | tenant_id, profile_id, configuration_hash |
| uq_sandbox_profile_version | yes | tenant_id, profile_id, version_number |

| Foreign key | Columns | References |
|---|---|---|
| fk_sandbox_profile_version_profile | profile_id | sandbox_profiles.id |
| fk_sandbox_profile_version_tenant | tenant_id | tenants.id |
| fk_sandbox_profile_version_user | created_by | users.id |

### sandbox_profiles

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| name | varchar(160) | NO | ∅ |  |
| description | varchar(1000) | YES | ∅ |  |
| status | enum('active','disabled') | NO | active |  |
| current_version_number | bigint unsigned | NO | 1 |  |
| owner_department_id | binary(16) | NO | ∅ |  |
| created_by | binary(16) | NO | ∅ |  |
| version | bigint unsigned | NO | 1 |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |
| updated_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED on update CURRENT_TIMESTAMP(6) |

| Index | Unique | Columns |
|---|---:|---|
| fk_sandbox_profile_department | no | owner_department_id |
| fk_sandbox_profile_user | no | created_by |
| idx_sandbox_profile_status | no | tenant_id, status, updated_at |
| PRIMARY | yes | id |
| uq_sandbox_profile_name | yes | tenant_id, name |

| Foreign key | Columns | References |
|---|---|---|
| fk_sandbox_profile_department | owner_department_id | departments.id |
| fk_sandbox_profile_tenant | tenant_id | tenants.id |
| fk_sandbox_profile_user | created_by | users.id |

### side_effect_confirmations

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| execution_id | binary(16) | NO | ∅ |  |
| node_execution_id | binary(16) | NO | ∅ |  |
| checkpoint_id | binary(16) | YES | ∅ |  |
| actor_user_id | binary(16) | NO | ∅ |  |
| decision | enum('execute','reuse_output','dry_run') | NO | ∅ |  |
| idempotency_key | varchar(192) | NO | ∅ |  |
| detail_json | json | NO | ∅ |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |

| Index | Unique | Columns |
|---|---:|---|
| fk_side_effect_actor | no | actor_user_id |
| fk_side_effect_checkpoint | no | checkpoint_id |
| fk_side_effect_execution | no | execution_id |
| fk_side_effect_node | no | node_execution_id |
| PRIMARY | yes | id |
| uq_side_effect_decision | yes | tenant_id, execution_id, node_execution_id |

| Foreign key | Columns | References |
|---|---|---|
| fk_side_effect_actor | actor_user_id | users.id |
| fk_side_effect_checkpoint | checkpoint_id | checkpoints.id |
| fk_side_effect_execution | execution_id | workflow_executions.id |
| fk_side_effect_node | node_execution_id | node_executions.id |
| fk_side_effect_tenant | tenant_id | tenants.id |

### skill_dependencies

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| skill_version_id | binary(16) | NO | ∅ |  |
| resource_type | varchar(32) | NO | ∅ |  |
| resource_id | binary(16) | NO | ∅ |  |
| resource_version_id | binary(16) | YES | ∅ |  |
| operation_key | varchar(32) | NO | use |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |

| Index | Unique | Columns |
|---|---:|---|
| idx_skill_dependency_resource | no | tenant_id, resource_type, resource_id |
| PRIMARY | yes | id |
| uq_skill_dependency | yes | skill_version_id, resource_type, resource_id, operation_key |

| Foreign key | Columns | References |
|---|---|---|
| fk_skill_dependency_tenant | tenant_id | tenants.id |
| fk_skill_dependency_version | skill_version_id | skill_versions.id |

### skill_file_references

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| skill_version_id | binary(16) | NO | ∅ |  |
| source_path | varchar(2048) | NO | ∅ |  |
| target_path | varchar(2048) | NO | ∅ |  |
| target_path_hash | char(64) | NO | ∅ |  |
| target_content_hash | char(64) | NO | ∅ |  |
| reference_type | enum('link','image') | NO | ∅ |  |

| Index | Unique | Columns |
|---|---:|---|
| fk_skill_reference_version | no | skill_version_id |
| idx_skill_reference_target | no | tenant_id, skill_version_id, target_path_hash |
| PRIMARY | yes | id |

| Foreign key | Columns | References |
|---|---|---|
| fk_skill_reference_tenant | tenant_id | tenants.id |
| fk_skill_reference_version | skill_version_id | skill_versions.id |

### skill_file_revisions

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| skill_id | binary(16) | NO | ∅ |  |
| entry_id | binary(16) | NO | ∅ |  |
| workspace_revision | bigint unsigned | NO | ∅ |  |
| artifact_id | binary(16) | NO | ∅ |  |
| content_hash | char(64) | NO | ∅ |  |
| size_bytes | bigint unsigned | NO | ∅ |  |
| created_by | binary(16) | NO | ∅ |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |

| Index | Unique | Columns |
|---|---:|---|
| fk_skill_file_revision_artifact | no | artifact_id |
| fk_skill_file_revision_entry | no | entry_id |
| fk_skill_file_revision_skill | no | skill_id |
| fk_skill_file_revision_user | no | created_by |
| PRIMARY | yes | id |
| uq_skill_file_revision | yes | tenant_id, skill_id, entry_id, workspace_revision |

| Foreign key | Columns | References |
|---|---|---|
| fk_skill_file_revision_artifact | artifact_id | artifacts.id |
| fk_skill_file_revision_entry | entry_id | skill_workspace_entries.id |
| fk_skill_file_revision_skill | skill_id | skills.id |
| fk_skill_file_revision_tenant | tenant_id | tenants.id |
| fk_skill_file_revision_user | created_by | users.id |

### skill_version_files

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| skill_version_id | binary(16) | NO | ∅ |  |
| path | varchar(2048) | NO | ∅ |  |
| path_hash | char(64) | NO | ∅ |  |
| mime_type | varchar(255) | NO | ∅ |  |
| artifact_id | binary(16) | NO | ∅ |  |
| content_hash | char(64) | NO | ∅ |  |
| size_bytes | bigint unsigned | NO | ∅ |  |

| Index | Unique | Columns |
|---|---:|---|
| fk_skill_version_file_artifact | no | artifact_id |
| fk_skill_version_file_version | no | skill_version_id |
| PRIMARY | yes | id |
| uq_skill_version_file | yes | tenant_id, skill_version_id, path_hash |

| Foreign key | Columns | References |
|---|---|---|
| fk_skill_version_file_artifact | artifact_id | artifacts.id |
| fk_skill_version_file_tenant | tenant_id | tenants.id |
| fk_skill_version_file_version | skill_version_id | skill_versions.id |

### skill_versions

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| skill_id | binary(16) | NO | ∅ |  |
| version_number | bigint unsigned | NO | ∅ |  |
| source_revision | bigint unsigned | NO | ∅ |  |
| manifest_json | json | NO | ∅ |  |
| content_hash | varchar(80) | NO | ∅ |  |
| created_by | binary(16) | NO | ∅ |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |

| Index | Unique | Columns |
|---|---:|---|
| fk_skill_version_skill | no | skill_id |
| fk_skill_version_user | no | created_by |
| PRIMARY | yes | id |
| uq_skill_version | yes | tenant_id, skill_id, version_number |
| uq_skill_version_hash | yes | tenant_id, skill_id, content_hash |

| Foreign key | Columns | References |
|---|---|---|
| fk_skill_version_skill | skill_id | skills.id |
| fk_skill_version_tenant | tenant_id | tenants.id |
| fk_skill_version_user | created_by | users.id |

### skill_workspace_entries

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| skill_id | binary(16) | NO | ∅ |  |
| parent_id | binary(16) | YES | ∅ |  |
| name | varchar(255) | NO | ∅ |  |
| path | varchar(2048) | NO | ∅ |  |
| path_hash | char(64) | NO | ∅ |  |
| entry_type | enum('directory','file') | NO | ∅ |  |
| mime_type | varchar(255) | YES | ∅ |  |
| artifact_id | binary(16) | YES | ∅ |  |
| content_hash | char(64) | YES | ∅ |  |
| size_bytes | bigint unsigned | NO | 0 |  |
| editable | tinyint(1) | NO | 0 |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |
| updated_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED on update CURRENT_TIMESTAMP(6) |

| Index | Unique | Columns |
|---|---:|---|
| fk_skill_workspace_artifact | no | artifact_id |
| fk_skill_workspace_parent | no | parent_id |
| fk_skill_workspace_skill | no | skill_id |
| idx_skill_workspace_parent | no | tenant_id, skill_id, parent_id, name |
| PRIMARY | yes | id |
| uq_skill_workspace_path | yes | tenant_id, skill_id, path_hash |

| Foreign key | Columns | References |
|---|---|---|
| fk_skill_workspace_artifact | artifact_id | artifacts.id |
| fk_skill_workspace_parent | parent_id | skill_workspace_entries.id |
| fk_skill_workspace_skill | skill_id | skills.id |
| fk_skill_workspace_tenant | tenant_id | tenants.id |

### skills

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| name | varchar(160) | NO | ∅ |  |
| alias | varchar(160) | NO | ∅ |  |
| description | varchar(1000) | YES | ∅ |  |
| owner_department_id | binary(16) | NO | ∅ |  |
| status | enum('draft','active','disabled') | NO | draft |  |
| draft_revision | bigint unsigned | NO | 1 |  |
| version | bigint unsigned | NO | 1 |  |
| created_by | binary(16) | NO | ∅ |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |
| updated_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED on update CURRENT_TIMESTAMP(6) |

| Index | Unique | Columns |
|---|---:|---|
| fk_skills_department | no | owner_department_id |
| fk_skills_user | no | created_by |
| idx_skills_tenant_status | no | tenant_id, status, updated_at |
| PRIMARY | yes | id |
| uq_skill_alias | yes | tenant_id, alias |
| uq_skill_name | yes | tenant_id, name |

| Foreign key | Columns | References |
|---|---|---|
| fk_skills_department | owner_department_id | departments.id |
| fk_skills_tenant | tenant_id | tenants.id |
| fk_skills_user | created_by | users.id |

### tenant_settings

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| tenant_id | binary(16) | NO | ∅ |  |
| locale | varchar(16) | NO | ∅ |  |
| timezone | varchar(64) | NO | ∅ |  |
| version | bigint unsigned | NO | 1 |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |
| updated_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED on update CURRENT_TIMESTAMP(6) |

| Index | Unique | Columns |
|---|---:|---|
| PRIMARY | yes | tenant_id |

| Foreign key | Columns | References |
|---|---|---|
| e | n | a.n |
| k | _ | t.e |
| e | n | a.n |
| e | n | a.n |
| d |  | . |

### tenants

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| name | varchar(100) | NO | ∅ |  |
| normalized_name | varchar(100) | NO | ∅ |  |
| status | enum('active','disabled') | NO | active |  |
| version | bigint unsigned | NO | 1 |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |
| updated_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED on update CURRENT_TIMESTAMP(6) |

| Index | Unique | Columns |
|---|---:|---|
| PRIMARY | yes | id |

| Foreign key | Columns | References |
|---|---|---|
| — | — | — |

### trace_delivery_offsets

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| consumer_name | varchar(128) | NO | ∅ |  |
| stream_id | varchar(128) | NO | ∅ |  |
| updated_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED on update CURRENT_TIMESTAMP(6) |

| Index | Unique | Columns |
|---|---:|---|
| PRIMARY | yes | consumer_name |

| Foreign key | Columns | References |
|---|---|---|
| — | — | — |

### trace_delivery_outbox

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| event_id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| execution_id | binary(16) | NO | ∅ |  |
| payload_json | json | NO | ∅ |  |
| status | enum('pending','streamed','delivered') | NO | pending |  |
| attempt_count | int unsigned | NO | 0 |  |
| available_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |
| streamed_at | timestamp(6) | YES | ∅ |  |
| delivered_at | timestamp(6) | YES | ∅ |  |
| last_error | varchar(1000) | YES | ∅ |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |

| Index | Unique | Columns |
|---|---:|---|
| fk_trace_delivery_execution | no | execution_id |
| fk_trace_delivery_tenant | no | tenant_id |
| idx_trace_delivery_pending | no | status, available_at, created_at |
| PRIMARY | yes | event_id |

| Foreign key | Columns | References |
|---|---|---|
| fk_trace_delivery_execution | execution_id | workflow_executions.id |
| fk_trace_delivery_tenant | tenant_id | tenants.id |

### trigger_bindings

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| application_id | binary(16) | NO | ∅ |  |
| application_deployment_id | binary(16) | NO | ∅ |  |
| workflow_version_id | binary(16) | NO | ∅ |  |
| node_id | varchar(128) | NO | ∅ |  |
| trigger_kind | enum('webhook','schedule','poll','lifecycle') | NO | ∅ |  |
| configuration_json | json | NO | ∅ |  |
| status | enum('activating','active','deactivating','disabled','failed') | NO | ∅ |  |
| next_poll_at | timestamp(6) | YES | ∅ |  |
| last_poll_at | timestamp(6) | YES | ∅ |  |
| locked_by | binary(16) | YES | ∅ |  |
| locked_until | timestamp(6) | YES | ∅ |  |
| last_error | varchar(1000) | YES | ∅ |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |
| updated_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED on update CURRENT_TIMESTAMP(6) |

| Index | Unique | Columns |
|---|---:|---|
| fk_trigger_binding_application | no | application_id |
| fk_trigger_binding_deployment | no | application_deployment_id |
| fk_trigger_binding_version | no | workflow_version_id |
| idx_trigger_binding_scan | no | status, trigger_kind, next_poll_at, locked_until |
| PRIMARY | yes | id |
| uq_trigger_binding_node | yes | tenant_id, application_deployment_id, node_id, trigger_kind |

| Foreign key | Columns | References |
|---|---|---|
| fk_trigger_binding_application | application_id | applications.id |
| fk_trigger_binding_deployment | application_deployment_id | application_deployments.id |
| fk_trigger_binding_tenant | tenant_id | tenants.id |
| fk_trigger_binding_version | workflow_version_id | workflow_versions.id |

### user_credentials

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| user_id | binary(16) | NO | ∅ |  |
| password_hash | varchar(255) | NO | ∅ |  |
| password_changed_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |

| Index | Unique | Columns |
|---|---:|---|
| PRIMARY | yes | user_id |

| Foreign key | Columns | References |
|---|---|---|
| s | e | r._ |
| k | _ | u.s |
| s | e | r._ |
| s | e | r.s |
| d |  | . |

### user_departments

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| tenant_id | binary(16) | NO | ∅ |  |
| user_id | binary(16) | NO | ∅ |  |
| department_id | binary(16) | NO | ∅ |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |

| Index | Unique | Columns |
|---|---:|---|
| fk_user_departments_department | no | department_id |
| idx_user_departments_department | no | tenant_id, department_id, user_id |
| PRIMARY | yes | user_id |

| Foreign key | Columns | References |
|---|---|---|
| fk_user_departments_department | department_id | departments.id |
| fk_user_departments_tenant | tenant_id | tenants.id |
| fk_user_departments_user | user_id | users.id |

### user_roles

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| user_id | binary(16) | NO | ∅ |  |
| role_id | binary(16) | NO | ∅ |  |
| scope_department_id | binary(16) | YES | ∅ |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |

| Index | Unique | Columns |
|---|---:|---|
| fk_user_roles_role | no | role_id |
| fk_user_roles_scope_department | no | scope_department_id |
| fk_user_roles_user | no | user_id |
| idx_user_roles_role | no | tenant_id, role_id |
| idx_user_roles_user | no | tenant_id, user_id |
| PRIMARY | yes | id |

| Foreign key | Columns | References |
|---|---|---|
| fk_user_roles_role | role_id | roles.id |
| fk_user_roles_scope_department | scope_department_id | departments.id |
| fk_user_roles_tenant | tenant_id | tenants.id |
| fk_user_roles_user | user_id | users.id |

### users

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| username | varchar(64) | NO | ∅ |  |
| username_normalized | varchar(64) | NO | ∅ |  |
| display_name | varchar(100) | NO | ∅ |  |
| status | enum('invited','active','disabled') | NO | invited |  |
| password_change_required | tinyint(1) | NO | 0 |  |
| token_version | bigint unsigned | NO | 1 |  |
| version | bigint unsigned | NO | 1 |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |
| updated_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED on update CURRENT_TIMESTAMP(6) |

| Index | Unique | Columns |
|---|---:|---|
| idx_users_tenant_status | no | tenant_id, status, created_at |
| PRIMARY | yes | id |
| uq_users_tenant_username | yes | tenant_id, username_normalized |

| Foreign key | Columns | References |
|---|---|---|
| s | e | r.s |
| k | _ | u.s |
| e | n | a.n |
| e | n | a.n |
| d |  | . |

### wait_subscriptions

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| execution_id | binary(16) | NO | ∅ |  |
| node_execution_id | binary(16) | NO | ∅ |  |
| resume_token_id | binary(16) | NO | ∅ |  |
| wait_kind | enum('duration','datetime','webhook','form','approval') | NO | ∅ |  |
| status | enum('waiting','resumed','timed_out','cancelled') | NO | waiting |  |
| wake_at | timestamp(6) | YES | ∅ |  |
| timeout_at | timestamp(6) | YES | ∅ |  |
| authentication_mode | enum('none','header','basic','signed') | NO | signed |  |
| response_mode | enum('accepted','last_node') | NO | accepted |  |
| payload_schema_json | json | YES | ∅ |  |
| resumed_at | timestamp(6) | YES | ∅ |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |
| updated_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED on update CURRENT_TIMESTAMP(6) |

| Index | Unique | Columns |
|---|---:|---|
| fk_wait_execution | no | execution_id |
| fk_wait_resume_token | no | resume_token_id |
| fk_wait_tenant | no | tenant_id |
| idx_wait_scheduler | no | status, wake_at, timeout_at |
| PRIMARY | yes | id |
| uq_wait_node | yes | node_execution_id |

| Foreign key | Columns | References |
|---|---|---|
| fk_wait_execution | execution_id | workflow_executions.id |
| fk_wait_node | node_execution_id | node_executions.id |
| fk_wait_resume_token | resume_token_id | execution_resume_tokens.id |
| fk_wait_tenant | tenant_id | tenants.id |

### worker_capabilities

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| instance_id | varchar(160) | NO | ∅ |  |
| capability | varchar(64) | NO | ∅ |  |
| node_protocol_version | varchar(32) | NO | ∅ |  |
| ir_schema_versions_json | json | NO | ∅ |  |
| compiler_version_min | varchar(64) | NO | ∅ |  |
| compiler_version_max | varchar(64) | NO | ∅ |  |
| manifest_hashes_json | json | NO | ∅ |  |
| status | enum('ready','draining','unavailable') | NO | ready |  |
| heartbeat_at | timestamp(6) | NO | ∅ |  |

| Index | Unique | Columns |
|---|---:|---|
| idx_worker_capability_ready | no | capability, status, heartbeat_at |
| PRIMARY | yes | instance_id, capability |

| Foreign key | Columns | References |
|---|---|---|
| — | — | — |

### worker_leases

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| node_attempt_id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| node_execution_id | binary(16) | NO | ∅ |  |
| lease_token | binary(16) | NO | ∅ |  |
| worker_instance_id | varchar(160) | NO | ∅ |  |
| capability | varchar(64) | NO | ∅ |  |
| acquired_at | timestamp(6) | NO | ∅ |  |
| heartbeat_at | timestamp(6) | NO | ∅ |  |
| expires_at | timestamp(6) | NO | ∅ |  |
| released_at | timestamp(6) | YES | ∅ |  |

| Index | Unique | Columns |
|---|---:|---|
| fk_worker_lease_node | no | node_execution_id |
| fk_worker_lease_tenant | no | tenant_id |
| idx_worker_lease_reaper | no | expires_at, released_at |
| PRIMARY | yes | node_attempt_id |
| uq_worker_lease_token | yes | lease_token |

| Foreign key | Columns | References |
|---|---|---|
| fk_worker_lease_attempt | node_attempt_id | node_attempts.id |
| fk_worker_lease_node | node_execution_id | node_executions.id |
| fk_worker_lease_tenant | tenant_id | tenants.id |

### workflow_context_patches

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| execution_id | binary(16) | NO | ∅ |  |
| node_execution_id | binary(16) | NO | ∅ |  |
| attempt_id | binary(16) | NO | ∅ |  |
| patch_index | int unsigned | NO | ∅ |  |
| operation_key | varchar(32) | NO | ∅ |  |
| context_path | varchar(512) | NO | ∅ |  |
| value_json | json | NO | ∅ |  |
| context_version_before | bigint unsigned | NO | ∅ |  |
| context_version_after | bigint unsigned | NO | ∅ |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |

| Index | Unique | Columns |
|---|---:|---|
| fk_workflow_context_patch_attempt | no | attempt_id |
| fk_workflow_context_patch_execution | no | execution_id |
| fk_workflow_context_patch_node_execution | no | node_execution_id |
| idx_workflow_context_patch_execution | no | tenant_id, execution_id, created_at |
| PRIMARY | yes | id |
| uq_workflow_context_patch | yes | tenant_id, node_execution_id, patch_index |

| Foreign key | Columns | References |
|---|---|---|
| fk_workflow_context_patch_attempt | attempt_id | node_attempts.id |
| fk_workflow_context_patch_execution | execution_id | workflow_executions.id |
| fk_workflow_context_patch_node_execution | node_execution_id | node_executions.id |
| fk_workflow_context_patch_tenant | tenant_id | tenants.id |

### workflow_debug_overlays

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| workflow_id | binary(16) | NO | ∅ |  |
| node_id | varchar(128) | NO | ∅ |  |
| kind | enum('pin_data','mock_output','temporary_input','history_output','artifact') | NO | ∅ |  |
| payload_json | json | NO | ∅ |  |
| artifact_id | binary(16) | YES | ∅ |  |
| schema_hash | varchar(96) | YES | ∅ |  |
| stale | tinyint(1) | NO | 0 |  |
| updated_by | binary(16) | NO | ∅ |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |
| updated_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED on update CURRENT_TIMESTAMP(6) |

| Index | Unique | Columns |
|---|---:|---|
| fk_debug_overlay_artifact | no | artifact_id |
| fk_debug_overlay_user | no | updated_by |
| fk_debug_overlay_workflow | no | workflow_id |
| idx_workflow_debug_overlay_workflow | no | tenant_id, workflow_id, updated_at |
| PRIMARY | yes | id |
| uq_workflow_debug_overlay_node | yes | tenant_id, workflow_id, node_id |

| Foreign key | Columns | References |
|---|---|---|
| fk_debug_overlay_artifact | artifact_id | artifacts.id |
| fk_debug_overlay_tenant | tenant_id | tenants.id |
| fk_debug_overlay_user | updated_by | users.id |
| fk_debug_overlay_workflow | workflow_id | workflows.id |

### workflow_deployment_heads

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| tenant_id | binary(16) | NO | ∅ |  |
| workflow_id | binary(16) | NO | ∅ |  |
| environment_id | binary(16) | NO | ∅ |  |
| active_deployment_id | binary(16) | NO | ∅ |  |
| version | bigint unsigned | NO | 1 |  |
| updated_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED on update CURRENT_TIMESTAMP(6) |

| Index | Unique | Columns |
|---|---:|---|
| fk_workflow_head_environment | no | environment_id |
| fk_workflow_head_workflow | no | workflow_id |
| PRIMARY | yes | tenant_id, workflow_id, environment_id |
| uq_workflow_active_deployment | yes | active_deployment_id |

| Foreign key | Columns | References |
|---|---|---|
| fk_workflow_head_deployment | active_deployment_id | workflow_deployments.id |
| fk_workflow_head_environment | environment_id | workflow_environments.id |
| fk_workflow_head_tenant | tenant_id | tenants.id |
| fk_workflow_head_workflow | workflow_id | workflows.id |

### workflow_deployments

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| workflow_id | binary(16) | NO | ∅ |  |
| environment_id | binary(16) | NO | ∅ |  |
| workflow_version_id | binary(16) | NO | ∅ |  |
| sequence_number | bigint unsigned | NO | ∅ |  |
| status | enum('active','superseded','rolled_back') | NO | active |  |
| source | enum('publish','rollback') | NO | publish |  |
| created_by | binary(16) | NO | ∅ |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |

| Index | Unique | Columns |
|---|---:|---|
| fk_workflow_deployment_environment | no | environment_id |
| fk_workflow_deployment_user | no | created_by |
| fk_workflow_deployment_version | no | workflow_version_id |
| fk_workflow_deployment_workflow | no | workflow_id |
| idx_workflow_deployment_history | no | tenant_id, workflow_id, environment_id, created_at |
| PRIMARY | yes | id |
| uq_workflow_deployment_sequence | yes | tenant_id, workflow_id, environment_id, sequence_number |

| Foreign key | Columns | References |
|---|---|---|
| fk_workflow_deployment_environment | environment_id | workflow_environments.id |
| fk_workflow_deployment_tenant | tenant_id | tenants.id |
| fk_workflow_deployment_user | created_by | users.id |
| fk_workflow_deployment_version | workflow_version_id | workflow_versions.id |
| fk_workflow_deployment_workflow | workflow_id | workflows.id |

### workflow_draft_resources

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| workflow_id | binary(16) | NO | ∅ |  |
| draft_id | binary(16) | NO | ∅ |  |
| node_id | varchar(128) | NO | ∅ |  |
| node_name | varchar(160) | YES | ∅ |  |
| resource_type | varchar(32) | NO | ∅ |  |
| resource_id | binary(16) | NO | ∅ |  |
| resource_version_id | binary(16) | YES | ∅ |  |
| operation_key | varchar(32) | NO | ∅ |  |
| relation | varchar(64) | NO | resource_reference |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |

| Index | Unique | Columns |
|---|---:|---|
| fk_workflow_draft_resource_workflow | no | workflow_id |
| idx_workflow_draft_resources_target | no | tenant_id, resource_type, resource_id, workflow_id |
| idx_workflow_draft_resources_workflow | no | tenant_id, workflow_id, node_id |
| PRIMARY | yes | id |
| uq_workflow_draft_resource | yes | draft_id, node_id, resource_type, resource_id, operation_key, relation |

| Foreign key | Columns | References |
|---|---|---|
| fk_workflow_draft_resource_draft | draft_id | workflow_drafts.id |
| fk_workflow_draft_resource_tenant | tenant_id | tenants.id |
| fk_workflow_draft_resource_workflow | workflow_id | workflows.id |

### workflow_draft_revisions

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| workflow_id | binary(16) | NO | ∅ |  |
| draft_id | binary(16) | NO | ∅ |  |
| revision | bigint unsigned | NO | ∅ |  |
| schema_version | varchar(32) | NO | ∅ |  |
| definition_json | json | NO | ∅ |  |
| editor_json | json | YES | ∅ |  |
| content_hash | varchar(80) | NO | ∅ |  |
| editor_hash | varchar(80) | YES | ∅ |  |
| created_by | binary(16) | NO | ∅ |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |

| Index | Unique | Columns |
|---|---:|---|
| fk_workflow_revision_draft | no | draft_id |
| fk_workflow_revision_user | no | created_by |
| fk_workflow_revision_workflow | no | workflow_id |
| idx_workflow_revisions_time | no | tenant_id, workflow_id, created_at |
| PRIMARY | yes | id |
| uq_workflow_draft_revision | yes | tenant_id, workflow_id, revision |

| Foreign key | Columns | References |
|---|---|---|
| fk_workflow_revision_draft | draft_id | workflow_drafts.id |
| fk_workflow_revision_tenant | tenant_id | tenants.id |
| fk_workflow_revision_user | created_by | users.id |
| fk_workflow_revision_workflow | workflow_id | workflows.id |

### workflow_drafts

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| workflow_id | binary(16) | NO | ∅ |  |
| schema_version | varchar(32) | NO | ∅ |  |
| revision | bigint unsigned | NO | 0 |  |
| definition_json | json | NO | ∅ |  |
| editor_json | json | YES | ∅ |  |
| content_hash | varchar(80) | NO | ∅ |  |
| editor_hash | varchar(80) | YES | ∅ |  |
| updated_by | binary(16) | NO | ∅ |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |
| updated_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED on update CURRENT_TIMESTAMP(6) |

| Index | Unique | Columns |
|---|---:|---|
| fk_workflow_draft_user | no | updated_by |
| fk_workflow_draft_workflow | no | workflow_id |
| PRIMARY | yes | id |
| uq_workflow_draft | yes | tenant_id, workflow_id |

| Foreign key | Columns | References |
|---|---|---|
| fk_workflow_draft_tenant | tenant_id | tenants.id |
| fk_workflow_draft_user | updated_by | users.id |
| fk_workflow_draft_workflow | workflow_id | workflows.id |

### workflow_environments

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| code | varchar(64) | NO | ∅ |  |
| name | varchar(100) | NO | ∅ |  |
| is_builtin | tinyint(1) | NO | 0 |  |
| status | enum('active','disabled') | NO | active |  |
| version | bigint unsigned | NO | 1 |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |
| updated_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED on update CURRENT_TIMESTAMP(6) |

| Index | Unique | Columns |
|---|---:|---|
| PRIMARY | yes | id |
| uq_workflow_environment_code | yes | tenant_id, code |

| Foreign key | Columns | References |
|---|---|---|
| o | r | k.f |
| k | _ | w.o |
| e | n | a.n |
| e | n | a.n |
| d |  | . |

### workflow_executions

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| workflow_id | binary(16) | NO | ∅ |  |
| workflow_version_id | binary(16) | YES | ∅ |  |
| source_kind | varchar(32) | NO | version |  |
| source_id | binary(16) | YES | ∅ |  |
| source_revision | bigint unsigned | YES | ∅ |  |
| invocation_id | binary(16) | YES | ∅ |  |
| parent_execution_id | binary(16) | YES | ∅ |  |
| caller_execution_id | binary(16) | YES | ∅ |  |
| caller_node_execution_id | binary(16) | YES | ∅ |  |
| fork_checkpoint_id | binary(16) | YES | ∅ |  |
| fork_mode | enum('whole','node','to_node','from_node') | YES | ∅ |  |
| session_id | binary(16) | YES | ∅ |  |
| application_deployment_id | binary(16) | YES | ∅ |  |
| trace_id | binary(16) | NO | ∅ |  |
| trigger_type | varchar(32) | NO | ∅ |  |
| execution_type | enum('whole','node','to_node','from_node','fork','sub_workflow') | NO | whole |  |
| requested_by | binary(16) | YES | ∅ |  |
| input_json | json | YES | ∅ |  |
| context_json | json | NO | ∅ |  |
| context_base_json | json | NO | ∅ |  |
| context_version | bigint unsigned | NO | 0 |  |
| session_context_version | bigint unsigned | NO | 0 |  |
| result_json | json | YES | ∅ |  |
| result_artifact_id | binary(16) | YES | ∅ |  |
| result_hash | varchar(96) | YES | ∅ |  |
| terminal_event_emitted | tinyint(1) | NO | 0 |  |
| status | enum('created','queued','running','waiting','waiting_approval','suspended','succeeded','failed','cancelled','timed_out') | NO | ∅ |  |
| started_at | timestamp(6) | NO | ∅ |  |
| ended_at | timestamp(6) | YES | ∅ |  |
| duration_ms | bigint unsigned | YES | ∅ |  |
| cost_micros | bigint unsigned | NO | 0 |  |
| input_tokens | bigint unsigned | NO | 0 |  |
| output_tokens | bigint unsigned | NO | 0 |  |
| error_code | varchar(128) | YES | ∅ |  |
| error_message | varchar(1000) | YES | ∅ |  |
| cancellation_requested_at | timestamp(6) | YES | ∅ |  |
| state_version | bigint unsigned | NO | 1 |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |
| updated_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED on update CURRENT_TIMESTAMP(6) |

| Index | Unique | Columns |
|---|---:|---|
| fk_execution_caller | no | caller_execution_id |
| fk_execution_fork_checkpoint | no | fork_checkpoint_id |
| fk_execution_invocation | no | invocation_id |
| fk_execution_parent | no | parent_execution_id |
| fk_execution_requested_by | no | requested_by |
| fk_execution_result_artifact | no | result_artifact_id |
| fk_execution_session | no | session_id |
| fk_execution_version_m6 | no | workflow_version_id |
| fk_execution_workflow | no | workflow_id |
| fk_workflow_execution_application_deployment | no | application_deployment_id |
| idx_execution_parent | no | tenant_id, parent_execution_id, started_at |
| idx_workflow_execution_parent_node | no | tenant_id, caller_execution_id, caller_node_execution_id |
| idx_workflow_execution_session_context | no | tenant_id, application_deployment_id, session_id |
| idx_workflow_execution_trace | no | tenant_id, trace_id |
| idx_workflow_executions_list | no | tenant_id, status, started_at |
| idx_workflow_executions_workflow | no | tenant_id, workflow_id, started_at |
| PRIMARY | yes | id |

| Foreign key | Columns | References |
|---|---|---|
| fk_execution_caller | caller_execution_id | workflow_executions.id |
| fk_execution_fork_checkpoint | fork_checkpoint_id | checkpoints.id |
| fk_execution_invocation | invocation_id | application_invocations.id |
| fk_execution_parent | parent_execution_id | workflow_executions.id |
| fk_execution_requested_by | requested_by | users.id |
| fk_execution_result_artifact | result_artifact_id | artifacts.id |
| fk_execution_session | session_id | application_sessions.id |
| fk_execution_tenant | tenant_id | tenants.id |
| fk_execution_version_m6 | workflow_version_id | workflow_versions.id |
| fk_execution_workflow | workflow_id | workflows.id |
| fk_workflow_execution_application_deployment | application_deployment_id | application_deployments.id |

### workflow_members

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| tenant_id | binary(16) | NO | ∅ |  |
| workflow_id | binary(16) | NO | ∅ |  |
| user_id | binary(16) | NO | ∅ |  |
| member_role | enum('viewer','editor','manager') | NO | ∅ |  |
| created_by | binary(16) | NO | ∅ |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |

| Index | Unique | Columns |
|---|---:|---|
| fk_workflow_members_creator | no | created_by |
| fk_workflow_members_user | no | user_id |
| idx_workflow_members_user | no | tenant_id, user_id, workflow_id |
| PRIMARY | yes | workflow_id, user_id |

| Foreign key | Columns | References |
|---|---|---|
| fk_workflow_members_creator | created_by | users.id |
| fk_workflow_members_tenant | tenant_id | tenants.id |
| fk_workflow_members_user | user_id | users.id |
| fk_workflow_members_workflow | workflow_id | workflows.id |

### workflow_service_identities

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| workflow_id | binary(16) | NO | ∅ |  |
| status | enum('active','disabled') | NO | active |  |
| version | bigint unsigned | NO | 1 |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |
| updated_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED on update CURRENT_TIMESTAMP(6) |

| Index | Unique | Columns |
|---|---:|---|
| fk_workflow_identity_workflow | no | workflow_id |
| PRIMARY | yes | id |
| uq_workflow_service_identity | yes | tenant_id, workflow_id |

| Foreign key | Columns | References |
|---|---|---|
| fk_workflow_identity_tenant | tenant_id | tenants.id |
| fk_workflow_identity_workflow | workflow_id | workflows.id |

### workflow_version_resources

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| workflow_version_id | binary(16) | NO | ∅ |  |
| node_id | varchar(128) | NO | ∅ |  |
| binding_id | varchar(128) | YES | ∅ |  |
| binding_role | varchar(64) | YES | ∅ |  |
| resource_type | varchar(32) | NO | ∅ |  |
| resource_id | binary(16) | NO | ∅ |  |
| resource_version_id | binary(16) | YES | ∅ |  |
| operation_key | varchar(32) | NO | ∅ |  |
| snapshot_json | json | NO | ∅ |  |
| snapshot_hash | varchar(80) | NO | ∅ |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |

| Index | Unique | Columns |
|---|---:|---|
| idx_workflow_version_resources_lookup | no | tenant_id, resource_type, resource_id |
| PRIMARY | yes | id |
| uq_workflow_version_resource | yes | workflow_version_id, node_id, resource_type, resource_id, operation_key |

| Foreign key | Columns | References |
|---|---|---|
| fk_workflow_version_resource_tenant | tenant_id | tenants.id |
| fk_workflow_version_resource_version | workflow_version_id | workflow_versions.id |

### workflow_versions

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| workflow_id | binary(16) | NO | ∅ |  |
| version_number | bigint unsigned | NO | ∅ |  |
| source_revision | bigint unsigned | NO | ∅ |  |
| schema_version | varchar(32) | NO | ∅ |  |
| definition_json | json | NO | ∅ |  |
| editor_json | json | YES | ∅ |  |
| content_hash | varchar(80) | NO | ∅ |  |
| editor_hash | varchar(80) | YES | ∅ |  |
| compiled_ir_json | json | YES | ∅ |  |
| compiled_ir_hash | varchar(96) | YES | ∅ |  |
| compiler_version | varchar(64) | YES | ∅ |  |
| compiled_at | timestamp(6) | YES | ∅ |  |
| created_by | binary(16) | NO | ∅ |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |

| Index | Unique | Columns |
|---|---:|---|
| fk_workflow_version_user | no | created_by |
| fk_workflow_version_workflow | no | workflow_id |
| idx_workflow_version_compiler | no | tenant_id, compiler_version, compiled_at |
| PRIMARY | yes | id |
| uq_workflow_version_content | yes | tenant_id, workflow_id, source_revision, content_hash |
| uq_workflow_version_number | yes | tenant_id, workflow_id, version_number |

| Foreign key | Columns | References |
|---|---|---|
| fk_workflow_version_tenant | tenant_id | tenants.id |
| fk_workflow_version_user | created_by | users.id |
| fk_workflow_version_workflow | workflow_id | workflows.id |

### workflows

| Column | Type | Nullable | Default | Extra |
|---|---|---:|---|---|
| id | binary(16) | NO | ∅ |  |
| tenant_id | binary(16) | NO | ∅ |  |
| name | varchar(160) | NO | ∅ |  |
| description | varchar(1000) | YES | ∅ |  |
| status | enum('active','archived') | NO | active |  |
| visibility | enum('private','department','company') | NO | private |  |
| owner_user_id | binary(16) | NO | ∅ |  |
| owner_department_id | binary(16) | NO | ∅ |  |
| version | bigint unsigned | NO | 1 |  |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED |
| updated_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | DEFAULT_GENERATED on update CURRENT_TIMESTAMP(6) |
| archived_at | timestamp(6) | YES | ∅ |  |

| Index | Unique | Columns |
|---|---:|---|
| fk_workflows_department | no | owner_department_id |
| fk_workflows_owner | no | owner_user_id |
| idx_workflows_department | no | tenant_id, owner_department_id, status |
| idx_workflows_tenant_status | no | tenant_id, status, updated_at |
| PRIMARY | yes | id |

| Foreign key | Columns | References |
|---|---|---|
| fk_workflows_department | owner_department_id | departments.id |
| fk_workflows_owner | owner_user_id | users.id |
| fk_workflows_tenant | tenant_id | tenants.id |

### agent_session_entries

P3-05 replacement for the removed monolithic session state. Entries are append-only; large payloads use `payload_artifact_id`.

| Column | Type | Nullable | Default | Extra |
|---|---|---|---|---|
| entry_id | varchar(191) | NO | ∅ | PRIMARY KEY |
| tenant_id | binary(16) | NO | ∅ | |
| application_id | binary(16) | YES | ∅ | |
| session_key | varchar(255) | NO | ∅ | |
| stable_agent_node_key | varchar(255) | NO | ∅ | |
| session_id | varchar(255) | NO | ∅ | |
| lane | enum('main') | NO | main | |
| sequence_number | bigint unsigned | NO | ∅ | append-only order |
| parent_entry_id | varchar(191) | YES | ∅ | |
| entry_kind | varchar(64) | NO | ∅ | |
| payload_json | json | YES | ∅ | exactly one payload location |
| payload_artifact_id | binary(16) | YES | ∅ | exactly one payload location |
| operation_id | varchar(191) | YES | ∅ | |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | |

### agent_session_registers

The mutable CAS/Fencing recovery authority for one Session, Node and main Lane.

| Column | Type | Nullable | Default | Extra |
|---|---|---|---|---|
| tenant_id | binary(16) | NO | ∅ | PRIMARY KEY part |
| application_id | binary(16) | YES | ∅ | |
| session_key | varchar(255) | NO | ∅ | PRIMARY KEY part |
| stable_agent_node_key | varchar(255) | NO | ∅ | PRIMARY KEY part |
| session_id | varchar(255) | NO | ∅ | |
| bundle_hash | varchar(128) | NO | ∅ | |
| definition_hash | varchar(128) | NO | ∅ | |
| model_version | varchar(255) | NO | ∅ | |
| core_contract_version | varchar(32) | NO | ∅ | |
| leaf_entry_id | varchar(191) | YES | ∅ | |
| open_operation_id | varchar(191) | YES | ∅ | |
| state_version | bigint unsigned | NO | 0 | CAS version |
| fencing_token | bigint unsigned | NO | 0 | monotonic lease proof |
| terminal_state | varchar(64) | YES | ∅ | |
| operation_json | json | YES | ∅ | |
| register_json | json | NO | ∅ | |
| retention_json | json | YES | ∅ | |
| lease_expires_at | timestamp(6) | YES | ∅ | |
| created_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | |
| updated_at | timestamp(6) | NO | CURRENT_TIMESTAMP(6) | on update CURRENT_TIMESTAMP(6) |

### agent_session_operations

Durable Operation Intent/Effect/Settlement and recovery metadata.

### agent_session_usages

Unique Model, Compaction, Tool and Memory usage projections keyed by Operation and Effect.

### agent_session_pending_entries

 Durable steering, follow-up and retry queue with a hard 32-entry bound and idempotency key. Retry rows retain the originating Execution/Node/Attempt and an idempotent `wake_command_id`; Runtime recovery creates `resume_execution` only after the Session Register has no valid open Operation.

### agent_subject_memory_audit

Trusted Subject Memory operation audit; payloads are intentionally excluded.

### agent_subject_memory_clears

Trusted Subject Memory clear tombstones and idempotent receipts; tombstones prevent resurrection after clear.
