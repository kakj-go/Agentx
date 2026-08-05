ALTER TABLE node_definition_versions
    MODIFY COLUMN capability VARCHAR(64) NOT NULL,
    ADD CONSTRAINT chk_node_definition_capability CHECK (REGEXP_LIKE(capability, '^[a-z][a-z0-9_]{0,63}$'));

ALTER TABLE node_executions
    MODIFY COLUMN capability VARCHAR(64) NOT NULL,
    ADD CONSTRAINT chk_node_execution_capability CHECK (REGEXP_LIKE(capability, '^[a-z][a-z0-9_]{0,63}$'));

ALTER TABLE execution_outbox
    MODIFY COLUMN capability VARCHAR(64) NULL,
    ADD CONSTRAINT chk_execution_outbox_capability CHECK (capability IS NULL OR REGEXP_LIKE(capability, '^[a-z][a-z0-9_]{0,63}$'));

ALTER TABLE worker_leases
    MODIFY COLUMN capability VARCHAR(64) NOT NULL,
    ADD CONSTRAINT chk_worker_lease_capability CHECK (REGEXP_LIKE(capability, '^[a-z][a-z0-9_]{0,63}$'));

ALTER TABLE workflow_executions
    ADD COLUMN input_tokens BIGINT UNSIGNED NOT NULL DEFAULT 0 AFTER cost_micros,
    ADD COLUMN output_tokens BIGINT UNSIGNED NOT NULL DEFAULT 0 AFTER input_tokens;

CREATE TABLE sandbox_profiles (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    name VARCHAR(160) NOT NULL,
    description VARCHAR(1000) NULL,
    status ENUM('active', 'disabled') NOT NULL DEFAULT 'active',
    current_version_number BIGINT UNSIGNED NOT NULL DEFAULT 1,
    owner_department_id BINARY(16) NOT NULL,
    created_by BINARY(16) NOT NULL,
    version BIGINT UNSIGNED NOT NULL DEFAULT 1,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    updated_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    UNIQUE KEY uq_sandbox_profile_name (tenant_id, name),
    KEY idx_sandbox_profile_status (tenant_id, status, updated_at),
    CONSTRAINT fk_sandbox_profile_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_sandbox_profile_department FOREIGN KEY (owner_department_id) REFERENCES departments(id),
    CONSTRAINT fk_sandbox_profile_user FOREIGN KEY (created_by) REFERENCES users(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE sandbox_profile_versions (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    profile_id BINARY(16) NOT NULL,
    version_number BIGINT UNSIGNED NOT NULL,
    runner ENUM('python', 'javascript', 'shell', 'browser') NOT NULL,
    image_digest VARCHAR(255) NOT NULL,
    cpu_millis INT UNSIGNED NOT NULL,
    memory_bytes BIGINT UNSIGNED NOT NULL,
    pids_limit INT UNSIGNED NOT NULL,
    disk_bytes BIGINT UNSIGNED NOT NULL,
    timeout_seconds INT UNSIGNED NOT NULL,
    output_limit_bytes BIGINT UNSIGNED NOT NULL,
    network_policy_json JSON NOT NULL,
    configuration_hash CHAR(64) NOT NULL,
    created_by BINARY(16) NOT NULL,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    UNIQUE KEY uq_sandbox_profile_version (tenant_id, profile_id, version_number),
    UNIQUE KEY uq_sandbox_profile_hash (tenant_id, profile_id, configuration_hash),
    CONSTRAINT chk_sandbox_image_digest CHECK (image_digest REGEXP '@sha256:[0-9a-f]{64}$'),
    CONSTRAINT fk_sandbox_profile_version_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_sandbox_profile_version_profile FOREIGN KEY (profile_id) REFERENCES sandbox_profiles(id),
    CONSTRAINT fk_sandbox_profile_version_user FOREIGN KEY (created_by) REFERENCES users(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE agent_runs (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    execution_id BINARY(16) NOT NULL,
    node_execution_id BINARY(16) NOT NULL,
    status ENUM('running', 'succeeded', 'failed', 'cancelled') NOT NULL DEFAULT 'running',
    budget_json JSON NOT NULL,
    iteration_count INT UNSIGNED NOT NULL DEFAULT 0,
    model_call_count INT UNSIGNED NOT NULL DEFAULT 0,
    tool_call_count INT UNSIGNED NOT NULL DEFAULT 0,
    reserved_tokens BIGINT UNSIGNED NOT NULL DEFAULT 0,
    input_tokens BIGINT UNSIGNED NOT NULL DEFAULT 0,
    output_tokens BIGINT UNSIGNED NOT NULL DEFAULT 0,
    reserved_cost_micros BIGINT UNSIGNED NOT NULL DEFAULT 0,
    cost_micros BIGINT UNSIGNED NOT NULL DEFAULT 0,
    state_artifact_id BINARY(16) NULL,
    state_hash CHAR(64) NULL,
    stop_reason VARCHAR(64) NULL,
    version BIGINT UNSIGNED NOT NULL DEFAULT 1,
    started_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    ended_at TIMESTAMP(6) NULL,
    PRIMARY KEY (id),
    UNIQUE KEY uq_agent_run_node (tenant_id, node_execution_id),
    KEY idx_agent_run_execution (tenant_id, execution_id, started_at),
    CONSTRAINT fk_agent_run_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_agent_run_execution FOREIGN KEY (execution_id) REFERENCES workflow_executions(id),
    CONSTRAINT fk_agent_run_node FOREIGN KEY (node_execution_id) REFERENCES node_executions(id),
    CONSTRAINT fk_agent_run_state FOREIGN KEY (state_artifact_id) REFERENCES artifacts(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE agent_iterations (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    agent_run_id BINARY(16) NOT NULL,
    iteration_index INT UNSIGNED NOT NULL,
    status ENUM('running', 'completed', 'failed', 'cancelled') NOT NULL DEFAULT 'running',
    state_before_hash CHAR(64) NOT NULL,
    state_after_hash CHAR(64) NULL,
    state_artifact_id BINARY(16) NULL,
    stop_reason VARCHAR(64) NULL,
    started_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    ended_at TIMESTAMP(6) NULL,
    PRIMARY KEY (id),
    UNIQUE KEY uq_agent_iteration (agent_run_id, iteration_index),
    CONSTRAINT fk_agent_iteration_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_agent_iteration_run FOREIGN KEY (agent_run_id) REFERENCES agent_runs(id),
    CONSTRAINT fk_agent_iteration_state FOREIGN KEY (state_artifact_id) REFERENCES artifacts(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE runtime_calls (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    execution_id BINARY(16) NOT NULL,
    node_execution_id BINARY(16) NOT NULL,
    attempt_id BINARY(16) NOT NULL,
    agent_run_id BINARY(16) NULL,
    iteration_index INT UNSIGNED NOT NULL DEFAULT 0,
    call_index INT UNSIGNED NOT NULL,
    call_kind ENUM('model', 'mcp_tool', 'rag', 'memory', 'sandbox') NOT NULL,
    idempotency_key VARCHAR(192) NOT NULL,
    request_fingerprint CHAR(64) NOT NULL,
    resource_type VARCHAR(32) NULL,
    resource_id BINARY(16) NULL,
    resource_version_id BINARY(16) NULL,
    side_effect VARCHAR(32) NOT NULL DEFAULT 'none',
    status ENUM('reserved', 'sent', 'succeeded', 'failed', 'cancelled', 'unknown') NOT NULL,
    reserved_input_tokens BIGINT UNSIGNED NOT NULL DEFAULT 0,
    reserved_output_tokens BIGINT UNSIGNED NOT NULL DEFAULT 0,
    reserved_cost_micros BIGINT UNSIGNED NOT NULL DEFAULT 0,
    input_tokens BIGINT UNSIGNED NOT NULL DEFAULT 0,
    output_tokens BIGINT UNSIGNED NOT NULL DEFAULT 0,
    cost_micros BIGINT UNSIGNED NOT NULL DEFAULT 0,
    usage_estimated BOOLEAN NOT NULL DEFAULT FALSE,
    response_artifact_id BINARY(16) NULL,
    error_code VARCHAR(128) NULL,
    error_message VARCHAR(1000) NULL,
    started_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    ended_at TIMESTAMP(6) NULL,
    PRIMARY KEY (id),
    UNIQUE KEY uq_runtime_call_idempotency (tenant_id, idempotency_key),
    KEY idx_runtime_call_node (tenant_id, node_execution_id, iteration_index, call_index),
    KEY idx_runtime_call_resource (tenant_id, resource_type, resource_id, started_at),
    CONSTRAINT fk_runtime_call_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_runtime_call_execution FOREIGN KEY (execution_id) REFERENCES workflow_executions(id),
    CONSTRAINT fk_runtime_call_node FOREIGN KEY (node_execution_id) REFERENCES node_executions(id),
    CONSTRAINT fk_runtime_call_attempt FOREIGN KEY (attempt_id) REFERENCES node_attempts(id),
    CONSTRAINT fk_runtime_call_agent FOREIGN KEY (agent_run_id) REFERENCES agent_runs(id),
    CONSTRAINT fk_runtime_call_response FOREIGN KEY (response_artifact_id) REFERENCES artifacts(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE sandbox_leases (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    execution_id BINARY(16) NOT NULL,
    node_execution_id BINARY(16) NOT NULL,
    attempt_id BINARY(16) NOT NULL,
    worker_lease_token BINARY(16) NOT NULL,
    sandbox_id VARCHAR(255) NULL,
    lease_token_hash CHAR(64) NOT NULL,
    profile_version_id BINARY(16) NOT NULL,
    idempotency_key VARCHAR(192) NOT NULL,
    status ENUM('creating', 'ready', 'running', 'interrupting', 'terminating', 'terminated', 'orphaned', 'failed') NOT NULL,
    endpoint_auth_key_id VARCHAR(64) NULL,
    endpoint_auth_nonce VARBINARY(32) NULL,
    endpoint_auth_ciphertext MEDIUMBLOB NULL,
    expires_at TIMESTAMP(6) NOT NULL,
    heartbeat_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    termination_attempts INT UNSIGNED NOT NULL DEFAULT 0,
    last_error VARCHAR(1000) NULL,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    terminated_at TIMESTAMP(6) NULL,
    PRIMARY KEY (id),
    UNIQUE KEY uq_sandbox_lease_idempotency (tenant_id, idempotency_key),
    UNIQUE KEY uq_sandbox_lease_token (lease_token_hash),
    KEY idx_sandbox_lease_reaper (status, expires_at, heartbeat_at),
    KEY idx_sandbox_lease_attempt (tenant_id, attempt_id),
    CONSTRAINT fk_sandbox_lease_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_sandbox_lease_execution FOREIGN KEY (execution_id) REFERENCES workflow_executions(id),
    CONSTRAINT fk_sandbox_lease_node FOREIGN KEY (node_execution_id) REFERENCES node_executions(id),
    CONSTRAINT fk_sandbox_lease_attempt FOREIGN KEY (attempt_id) REFERENCES node_attempts(id),
    CONSTRAINT fk_sandbox_lease_profile FOREIGN KEY (profile_version_id) REFERENCES sandbox_profile_versions(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

ALTER TABLE node_invocation_handles
    ADD COLUMN scope_json JSON NULL AFTER resource_version,
    ADD COLUMN sandbox_lease_id BINARY(16) NULL AFTER scope_json,
    ADD COLUMN revoked_at TIMESTAMP(6) NULL AFTER consumed_at,
    ADD KEY idx_node_invocation_handle_sandbox (sandbox_lease_id, revoked_at),
    ADD CONSTRAINT fk_node_invocation_handle_sandbox FOREIGN KEY (sandbox_lease_id) REFERENCES sandbox_leases(id);

UPDATE node_invocation_handles SET scope_json=JSON_OBJECT() WHERE scope_json IS NULL;

ALTER TABLE node_invocation_handles MODIFY COLUMN scope_json JSON NOT NULL;

INSERT INTO permissions (id, permission_key, name) VALUES
    (UUID_TO_BIN(UUID()), 'sandbox:view', 'View Sandbox Profiles'),
    (UUID_TO_BIN(UUID()), 'sandbox:manage', 'Manage Sandbox Profiles')
ON DUPLICATE KEY UPDATE name = VALUES(name);

INSERT IGNORE INTO role_permissions (tenant_id, role_id, permission_id)
SELECT r.tenant_id, r.id, p.id FROM roles r CROSS JOIN permissions p
WHERE r.code = 'company_admin' AND p.permission_key IN ('sandbox:view', 'sandbox:manage');

INSERT IGNORE INTO role_permissions (tenant_id, role_id, permission_id)
SELECT r.tenant_id, r.id, p.id FROM roles r JOIN permissions p ON p.permission_key = 'sandbox:view'
WHERE r.code = 'department_admin';
