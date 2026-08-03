CREATE TABLE workflow_executions (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    workflow_id BINARY(16) NOT NULL,
    workflow_version_id BINARY(16) NOT NULL,
    invocation_id BINARY(16) NULL,
    session_id BINARY(16) NULL,
    trace_id BINARY(16) NOT NULL,
    trigger_type VARCHAR(32) NOT NULL,
    status ENUM('created', 'queued', 'running', 'waiting', 'waiting_approval', 'suspended', 'succeeded', 'failed', 'cancelled', 'timed_out') NOT NULL,
    started_at TIMESTAMP(6) NOT NULL,
    ended_at TIMESTAMP(6) NULL,
    duration_ms BIGINT UNSIGNED NULL,
    cost_micros BIGINT UNSIGNED NOT NULL DEFAULT 0,
    error_code VARCHAR(128) NULL,
    error_message VARCHAR(1000) NULL,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    updated_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    UNIQUE KEY uq_workflow_execution_trace (tenant_id, trace_id),
    KEY idx_workflow_executions_list (tenant_id, status, started_at),
    KEY idx_workflow_executions_workflow (tenant_id, workflow_id, started_at),
    CONSTRAINT fk_execution_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_execution_workflow FOREIGN KEY (workflow_id) REFERENCES workflows(id),
    CONSTRAINT fk_execution_version FOREIGN KEY (workflow_version_id) REFERENCES workflow_versions(id),
    CONSTRAINT fk_execution_invocation FOREIGN KEY (invocation_id) REFERENCES application_invocations(id),
    CONSTRAINT fk_execution_session FOREIGN KEY (session_id) REFERENCES application_sessions(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

ALTER TABLE evaluation_case_results
    ADD CONSTRAINT fk_evaluation_case_execution FOREIGN KEY (execution_id) REFERENCES workflow_executions(id);

CREATE TABLE execution_events (
    tenant_id BINARY(16) NOT NULL,
    execution_id BINARY(16) NOT NULL,
    sequence_number BIGINT UNSIGNED NOT NULL,
    event_type VARCHAR(64) NOT NULL,
    status VARCHAR(32) NOT NULL,
    summary_json JSON NOT NULL,
    occurred_at TIMESTAMP(6) NOT NULL,
    PRIMARY KEY (tenant_id, execution_id, sequence_number),
    CONSTRAINT fk_execution_event_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_execution_event_execution FOREIGN KEY (execution_id) REFERENCES workflow_executions(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE approval_tasks (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    execution_id BINARY(16) NOT NULL,
    workflow_id BINARY(16) NOT NULL,
    node_id VARCHAR(128) NOT NULL,
    title VARCHAR(255) NOT NULL,
    description VARCHAR(2000) NULL,
    request_payload_json JSON NULL,
    status ENUM('pending', 'claimed', 'approved', 'rejected', 'cancelled', 'timed_out') NOT NULL DEFAULT 'pending',
    claimed_by BINARY(16) NULL,
    claimed_at TIMESTAMP(6) NULL,
    resume_status ENUM('not_requested', 'pending', 'succeeded', 'blocked_runtime', 'failed') NOT NULL DEFAULT 'not_requested',
    deadline_at TIMESTAMP(6) NULL,
    version BIGINT UNSIGNED NOT NULL DEFAULT 1,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    updated_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    KEY idx_approval_tasks_inbox (tenant_id, status, deadline_at, created_at),
    CONSTRAINT fk_approval_task_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_approval_task_execution FOREIGN KEY (execution_id) REFERENCES workflow_executions(id),
    CONSTRAINT fk_approval_task_workflow FOREIGN KEY (workflow_id) REFERENCES workflows(id),
    CONSTRAINT fk_approval_task_claimed_by FOREIGN KEY (claimed_by) REFERENCES users(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE approval_candidates (
    tenant_id BINARY(16) NOT NULL,
    approval_task_id BINARY(16) NOT NULL,
    candidate_type ENUM('user', 'role', 'department') NOT NULL,
    candidate_id BINARY(16) NOT NULL,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    PRIMARY KEY (tenant_id, approval_task_id, candidate_type, candidate_id),
    CONSTRAINT fk_approval_candidate_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_approval_candidate_task FOREIGN KEY (approval_task_id) REFERENCES approval_tasks(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE approval_actions (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    approval_task_id BINARY(16) NOT NULL,
    actor_user_id BINARY(16) NOT NULL,
    action_type ENUM('claim', 'release', 'reassign', 'approve', 'reject', 'cancel', 'timeout') NOT NULL,
    input_json JSON NULL,
    from_status VARCHAR(32) NOT NULL,
    to_status VARCHAR(32) NOT NULL,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    KEY idx_approval_actions_task (tenant_id, approval_task_id, created_at),
    CONSTRAINT fk_approval_action_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_approval_action_task FOREIGN KEY (approval_task_id) REFERENCES approval_tasks(id),
    CONSTRAINT fk_approval_action_actor FOREIGN KEY (actor_user_id) REFERENCES users(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE notifications (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    source_event_id BINARY(16) NOT NULL,
    notification_type VARCHAR(64) NOT NULL,
    title_key VARCHAR(160) NOT NULL,
    body_key VARCHAR(160) NOT NULL,
    arguments_json JSON NOT NULL,
    target_type VARCHAR(64) NOT NULL,
    target_id BINARY(16) NOT NULL,
    target_path VARCHAR(512) NOT NULL,
    tone ENUM('primary', 'success', 'warning', 'danger') NOT NULL DEFAULT 'primary',
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    UNIQUE KEY uq_notification_source (tenant_id, source_event_id, notification_type),
    CONSTRAINT fk_notification_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE notification_receipts (
    tenant_id BINARY(16) NOT NULL,
    notification_id BINARY(16) NOT NULL,
    user_id BINARY(16) NOT NULL,
    read_at TIMESTAMP(6) NULL,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    PRIMARY KEY (tenant_id, notification_id, user_id),
    KEY idx_notification_inbox (tenant_id, user_id, read_at, created_at),
    CONSTRAINT fk_notification_receipt_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_notification_receipt_notification FOREIGN KEY (notification_id) REFERENCES notifications(id),
    CONSTRAINT fk_notification_receipt_user FOREIGN KEY (user_id) REFERENCES users(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE trace_delivery_outbox (
    event_id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    execution_id BINARY(16) NOT NULL,
    payload_json JSON NOT NULL,
    status ENUM('pending', 'streamed', 'delivered') NOT NULL DEFAULT 'pending',
    attempt_count INT UNSIGNED NOT NULL DEFAULT 0,
    available_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    streamed_at TIMESTAMP(6) NULL,
    delivered_at TIMESTAMP(6) NULL,
    last_error VARCHAR(1000) NULL,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    PRIMARY KEY (event_id),
    KEY idx_trace_delivery_pending (status, available_at, created_at),
    CONSTRAINT fk_trace_delivery_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_trace_delivery_execution FOREIGN KEY (execution_id) REFERENCES workflow_executions(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE trace_delivery_offsets (
    consumer_name VARCHAR(128) NOT NULL,
    stream_id VARCHAR(128) NOT NULL,
    updated_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6),
    PRIMARY KEY (consumer_name)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE runtime_service_heartbeats (
    tenant_id BINARY(16) NOT NULL,
    service_type ENUM('coordinator', 'worker', 'sandbox', 'trace_writer') NOT NULL,
    instance_id VARCHAR(160) NOT NULL,
    status ENUM('ready', 'degraded', 'unavailable') NOT NULL,
    detail_json JSON NOT NULL,
    heartbeat_at TIMESTAMP(6) NOT NULL,
    PRIMARY KEY (tenant_id, service_type, instance_id),
    KEY idx_runtime_heartbeat (tenant_id, service_type, heartbeat_at),
    CONSTRAINT fk_runtime_heartbeat_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

INSERT INTO permissions (id, permission_key, name) VALUES
    (UUID_TO_BIN(UUID()), 'approval:view', 'View approval tasks'),
    (UUID_TO_BIN(UUID()), 'approval:act', 'Claim and decide approval tasks'),
    (UUID_TO_BIN(UUID()), 'approval:manage', 'Manage approval tasks'),
    (UUID_TO_BIN(UUID()), 'notification:view', 'View own notifications'),
    (UUID_TO_BIN(UUID()), 'execution:view', 'View workflow executions'),
    (UUID_TO_BIN(UUID()), 'execution:cancel', 'Cancel workflow executions'),
    (UUID_TO_BIN(UUID()), 'trace:view', 'View workflow traces'),
    (UUID_TO_BIN(UUID()), 'runtime:view', 'View workflow runtime status')
ON DUPLICATE KEY UPDATE name = VALUES(name);

INSERT IGNORE INTO role_permissions (tenant_id, role_id, permission_id)
SELECT r.tenant_id, r.id, p.id FROM roles r CROSS JOIN permissions p WHERE r.code = 'company_admin';

INSERT IGNORE INTO role_permissions (tenant_id, role_id, permission_id)
SELECT r.tenant_id, r.id, p.id FROM roles r JOIN permissions p ON p.permission_key IN (
    'approval:view', 'approval:act', 'notification:view', 'execution:view', 'trace:view', 'runtime:view'
) WHERE r.code = 'department_admin';

INSERT IGNORE INTO role_permissions (tenant_id, role_id, permission_id)
SELECT r.tenant_id, r.id, p.id FROM roles r JOIN permissions p ON p.permission_key IN (
    'approval:view', 'approval:act', 'notification:view'
) WHERE r.code = 'member';
