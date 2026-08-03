ALTER TABLE workflow_executions
    ADD COLUMN execution_type ENUM('whole', 'node', 'to_node', 'from_node', 'fork', 'sub_workflow') NOT NULL DEFAULT 'whole' AFTER trigger_type,
    ADD COLUMN parent_execution_id BINARY(16) NULL AFTER invocation_id,
    ADD COLUMN caller_execution_id BINARY(16) NULL AFTER parent_execution_id,
    ADD COLUMN fork_checkpoint_id BINARY(16) NULL AFTER caller_execution_id,
    ADD COLUMN fork_mode ENUM('whole', 'node', 'to_node', 'from_node') NULL AFTER fork_checkpoint_id,
    ADD CONSTRAINT fk_execution_parent FOREIGN KEY (parent_execution_id) REFERENCES workflow_executions(id),
    ADD CONSTRAINT fk_execution_caller FOREIGN KEY (caller_execution_id) REFERENCES workflow_executions(id),
    ADD CONSTRAINT fk_execution_fork_checkpoint FOREIGN KEY (fork_checkpoint_id) REFERENCES checkpoints(id),
    ADD KEY idx_execution_parent (tenant_id, parent_execution_id, started_at);

CREATE TABLE checkpoint_artifacts (
    tenant_id BINARY(16) NOT NULL,
    checkpoint_id BINARY(16) NOT NULL,
    artifact_id BINARY(16) NOT NULL,
    role ENUM('state', 'input', 'output', 'binary', 'log') NOT NULL,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    PRIMARY KEY (checkpoint_id, artifact_id, role),
    CONSTRAINT fk_checkpoint_artifact_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_checkpoint_artifact_checkpoint FOREIGN KEY (checkpoint_id) REFERENCES checkpoints(id),
    CONSTRAINT fk_checkpoint_artifact_value FOREIGN KEY (artifact_id) REFERENCES artifacts(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE execution_resume_tokens (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    execution_id BINARY(16) NOT NULL,
    node_execution_id BINARY(16) NOT NULL,
    token_hash CHAR(64) NOT NULL,
    resume_kind ENUM('time', 'webhook', 'form', 'approval') NOT NULL,
    status ENUM('active', 'used', 'expired', 'cancelled') NOT NULL DEFAULT 'active',
    idempotency_key VARCHAR(192) NULL,
    expires_at TIMESTAMP(6) NULL,
    used_at TIMESTAMP(6) NULL,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    UNIQUE KEY uq_resume_token_hash (token_hash),
    UNIQUE KEY uq_resume_node_kind (node_execution_id, resume_kind),
    KEY idx_resume_token_expiry (status, expires_at),
    CONSTRAINT fk_resume_token_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_resume_token_execution FOREIGN KEY (execution_id) REFERENCES workflow_executions(id),
    CONSTRAINT fk_resume_token_node FOREIGN KEY (node_execution_id) REFERENCES node_executions(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE wait_subscriptions (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    execution_id BINARY(16) NOT NULL,
    node_execution_id BINARY(16) NOT NULL,
    resume_token_id BINARY(16) NOT NULL,
    wait_kind ENUM('duration', 'datetime', 'webhook', 'form', 'approval') NOT NULL,
    status ENUM('waiting', 'resumed', 'timed_out', 'cancelled') NOT NULL DEFAULT 'waiting',
    wake_at TIMESTAMP(6) NULL,
    timeout_at TIMESTAMP(6) NULL,
    authentication_mode ENUM('none', 'header', 'basic', 'signed') NOT NULL DEFAULT 'signed',
    response_mode ENUM('accepted', 'last_node') NOT NULL DEFAULT 'accepted',
    payload_schema_json JSON NULL,
    resumed_at TIMESTAMP(6) NULL,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    updated_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    UNIQUE KEY uq_wait_node (node_execution_id),
    KEY idx_wait_scheduler (status, wake_at, timeout_at),
    CONSTRAINT fk_wait_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_wait_execution FOREIGN KEY (execution_id) REFERENCES workflow_executions(id),
    CONSTRAINT fk_wait_node FOREIGN KEY (node_execution_id) REFERENCES node_executions(id),
    CONSTRAINT fk_wait_resume_token FOREIGN KEY (resume_token_id) REFERENCES execution_resume_tokens(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE resume_webhook_bindings (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    wait_subscription_id BINARY(16) NOT NULL,
    path_token_hash CHAR(64) NOT NULL,
    http_method ENUM('GET', 'POST') NOT NULL DEFAULT 'POST',
    authentication_config_hash CHAR(64) NULL,
    status ENUM('active', 'used', 'expired', 'cancelled') NOT NULL DEFAULT 'active',
    expires_at TIMESTAMP(6) NULL,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    UNIQUE KEY uq_resume_webhook_path (path_token_hash),
    CONSTRAINT fk_resume_webhook_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_resume_webhook_wait FOREIGN KEY (wait_subscription_id) REFERENCES wait_subscriptions(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE side_effect_confirmations (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    execution_id BINARY(16) NOT NULL,
    node_execution_id BINARY(16) NOT NULL,
    checkpoint_id BINARY(16) NULL,
    actor_user_id BINARY(16) NOT NULL,
    decision ENUM('execute', 'reuse_output', 'dry_run') NOT NULL,
    idempotency_key VARCHAR(192) NOT NULL,
    detail_json JSON NOT NULL,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    UNIQUE KEY uq_side_effect_decision (tenant_id, execution_id, node_execution_id),
    CONSTRAINT fk_side_effect_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_side_effect_execution FOREIGN KEY (execution_id) REFERENCES workflow_executions(id),
    CONSTRAINT fk_side_effect_node FOREIGN KEY (node_execution_id) REFERENCES node_executions(id),
    CONSTRAINT fk_side_effect_checkpoint FOREIGN KEY (checkpoint_id) REFERENCES checkpoints(id),
    CONSTRAINT fk_side_effect_actor FOREIGN KEY (actor_user_id) REFERENCES users(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

ALTER TABLE approval_tasks
    ADD COLUMN node_execution_id BINARY(16) NULL AFTER node_id,
    ADD COLUMN resume_token_id BINARY(16) NULL AFTER node_execution_id,
    ADD UNIQUE KEY uq_approval_node_execution (node_execution_id),
    ADD CONSTRAINT fk_approval_node_execution FOREIGN KEY (node_execution_id) REFERENCES node_executions(id),
    ADD CONSTRAINT fk_approval_resume_token FOREIGN KEY (resume_token_id) REFERENCES execution_resume_tokens(id);

INSERT INTO permissions (id, permission_key, name) VALUES
    (UUID_TO_BIN(UUID()), 'execution:run', 'Run workflow versions'),
    (UUID_TO_BIN(UUID()), 'execution:fork', 'Fork workflow executions'),
    (UUID_TO_BIN(UUID()), 'execution:resume', 'Resume workflow executions')
ON DUPLICATE KEY UPDATE name = VALUES(name);

INSERT IGNORE INTO role_permissions (tenant_id, role_id, permission_id)
SELECT r.tenant_id, r.id, p.id FROM roles r CROSS JOIN permissions p
WHERE r.code = 'company_admin' AND p.permission_key IN ('execution:run', 'execution:fork', 'execution:resume');

INSERT IGNORE INTO role_permissions (tenant_id, role_id, permission_id)
SELECT r.tenant_id, r.id, p.id FROM roles r JOIN permissions p
ON p.permission_key IN ('execution:run', 'execution:fork')
WHERE r.code = 'department_admin';
