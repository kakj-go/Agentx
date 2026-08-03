CREATE TABLE applications (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    workflow_id BINARY(16) NOT NULL,
    name VARCHAR(160) NOT NULL,
    slug VARCHAR(96) NOT NULL,
    description VARCHAR(1000) NULL,
    visibility ENUM('private', 'department', 'company') NOT NULL DEFAULT 'private',
    owner_user_id BINARY(16) NOT NULL,
    owner_department_id BINARY(16) NOT NULL,
    status ENUM('draft', 'active', 'disabled') NOT NULL DEFAULT 'draft',
    version BIGINT UNSIGNED NOT NULL DEFAULT 1,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    updated_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    UNIQUE KEY uq_application_slug (tenant_id, slug),
    KEY idx_applications_tenant (tenant_id, status, updated_at),
    KEY idx_applications_department (tenant_id, owner_department_id, status),
    CONSTRAINT fk_application_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_application_workflow FOREIGN KEY (workflow_id) REFERENCES workflows(id),
    CONSTRAINT fk_application_owner FOREIGN KEY (owner_user_id) REFERENCES users(id),
    CONSTRAINT fk_application_department FOREIGN KEY (owner_department_id) REFERENCES departments(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE application_deployments (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    application_id BINARY(16) NOT NULL,
    workflow_version_id BINARY(16) NOT NULL,
    environment_id BINARY(16) NOT NULL,
    sequence_number BIGINT UNSIGNED NOT NULL,
    input_schema_json JSON NOT NULL,
    output_schema_json JSON NOT NULL,
    session_version_policy ENUM('pinned', 'follow_deployment', 'manual_upgrade') NOT NULL DEFAULT 'pinned',
    status ENUM('active', 'superseded') NOT NULL DEFAULT 'active',
    created_by BINARY(16) NOT NULL,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    UNIQUE KEY uq_application_deployment_sequence (tenant_id, application_id, sequence_number),
    KEY idx_application_deployment_version (tenant_id, workflow_version_id),
    CONSTRAINT fk_application_deployment_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_application_deployment_application FOREIGN KEY (application_id) REFERENCES applications(id),
    CONSTRAINT fk_application_deployment_version FOREIGN KEY (workflow_version_id) REFERENCES workflow_versions(id),
    CONSTRAINT fk_application_deployment_environment FOREIGN KEY (environment_id) REFERENCES workflow_environments(id),
    CONSTRAINT fk_application_deployment_creator FOREIGN KEY (created_by) REFERENCES users(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE application_deployment_heads (
    tenant_id BINARY(16) NOT NULL,
    application_id BINARY(16) NOT NULL,
    deployment_id BINARY(16) NOT NULL,
    version BIGINT UNSIGNED NOT NULL DEFAULT 1,
    updated_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6),
    PRIMARY KEY (tenant_id, application_id),
    UNIQUE KEY uq_application_deployment_head (deployment_id),
    CONSTRAINT fk_application_head_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_application_head_application FOREIGN KEY (application_id) REFERENCES applications(id),
    CONSTRAINT fk_application_head_deployment FOREIGN KEY (deployment_id) REFERENCES application_deployments(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE application_api_keys (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    application_id BINARY(16) NOT NULL,
    family_id BINARY(16) NOT NULL,
    name VARCHAR(160) NOT NULL,
    key_prefix VARCHAR(48) NOT NULL,
    secret_hash BINARY(32) NOT NULL,
    status ENUM('active', 'revoked') NOT NULL DEFAULT 'active',
    created_by BINARY(16) NOT NULL,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    last_used_at TIMESTAMP(6) NULL,
    revoked_at TIMESTAMP(6) NULL,
    PRIMARY KEY (id),
    UNIQUE KEY uq_application_api_key_prefix (key_prefix),
    KEY idx_application_api_keys (tenant_id, application_id, status),
    CONSTRAINT fk_application_api_key_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_application_api_key_application FOREIGN KEY (application_id) REFERENCES applications(id),
    CONSTRAINT fk_application_api_key_creator FOREIGN KEY (created_by) REFERENCES users(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE application_webhooks (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    application_id BINARY(16) NOT NULL,
    name VARCHAR(160) NOT NULL,
    public_id VARCHAR(64) NOT NULL,
    secret_algorithm VARCHAR(32) NOT NULL,
    secret_key_id VARCHAR(64) NOT NULL,
    secret_nonce VARBINARY(32) NOT NULL,
    secret_ciphertext BLOB NOT NULL,
    status ENUM('active', 'disabled') NOT NULL DEFAULT 'active',
    version BIGINT UNSIGNED NOT NULL DEFAULT 1,
    created_by BINARY(16) NOT NULL,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    updated_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    UNIQUE KEY uq_application_webhook_public (public_id),
    KEY idx_application_webhooks (tenant_id, application_id, status),
    CONSTRAINT fk_application_webhook_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_application_webhook_application FOREIGN KEY (application_id) REFERENCES applications(id),
    CONSTRAINT fk_application_webhook_creator FOREIGN KEY (created_by) REFERENCES users(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE application_schedules (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    application_id BINARY(16) NOT NULL,
    name VARCHAR(160) NOT NULL,
    cron_expression VARCHAR(128) NOT NULL,
    timezone VARCHAR(64) NOT NULL,
    input_json JSON NOT NULL,
    status ENUM('active', 'disabled') NOT NULL DEFAULT 'disabled',
    version BIGINT UNSIGNED NOT NULL DEFAULT 1,
    created_by BINARY(16) NOT NULL,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    updated_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    KEY idx_application_schedules (tenant_id, application_id, status),
    CONSTRAINT fk_application_schedule_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_application_schedule_application FOREIGN KEY (application_id) REFERENCES applications(id),
    CONSTRAINT fk_application_schedule_creator FOREIGN KEY (created_by) REFERENCES users(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE application_sessions (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    application_id BINARY(16) NOT NULL,
    application_deployment_id BINARY(16) NOT NULL,
    workflow_version_id BINARY(16) NULL,
    version_policy ENUM('pinned', 'follow_deployment', 'manual_upgrade') NOT NULL,
    external_user_id VARCHAR(255) NULL,
    title VARCHAR(255) NULL,
    status ENUM('active', 'closed') NOT NULL DEFAULT 'active',
    version BIGINT UNSIGNED NOT NULL DEFAULT 1,
    created_by_user_id BINARY(16) NULL,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    updated_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    KEY idx_application_sessions (tenant_id, application_id, updated_at),
    CONSTRAINT fk_session_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_session_application FOREIGN KEY (application_id) REFERENCES applications(id),
    CONSTRAINT fk_session_deployment FOREIGN KEY (application_deployment_id) REFERENCES application_deployments(id),
    CONSTRAINT fk_session_workflow_version FOREIGN KEY (workflow_version_id) REFERENCES workflow_versions(id),
    CONSTRAINT fk_session_creator FOREIGN KEY (created_by_user_id) REFERENCES users(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE application_session_version_history (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    session_id BINARY(16) NOT NULL,
    from_workflow_version_id BINARY(16) NULL,
    to_workflow_version_id BINARY(16) NOT NULL,
    changed_by BINARY(16) NOT NULL,
    changed_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    KEY idx_session_version_history (tenant_id, session_id, changed_at),
    CONSTRAINT fk_session_history_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_session_history_session FOREIGN KEY (session_id) REFERENCES application_sessions(id),
    CONSTRAINT fk_session_history_from_version FOREIGN KEY (from_workflow_version_id) REFERENCES workflow_versions(id),
    CONSTRAINT fk_session_history_to_version FOREIGN KEY (to_workflow_version_id) REFERENCES workflow_versions(id),
    CONSTRAINT fk_session_history_user FOREIGN KEY (changed_by) REFERENCES users(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE application_invocations (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    application_id BINARY(16) NOT NULL,
    session_id BINARY(16) NULL,
    workflow_version_id BINARY(16) NOT NULL,
    execution_id BINARY(16) NULL,
    caller_type ENUM('user', 'api_key', 'webhook', 'schedule') NOT NULL,
    caller_id BINARY(16) NULL,
    request_hash CHAR(64) NOT NULL,
    idempotency_key VARCHAR(128) NOT NULL,
    status ENUM('queued', 'running', 'completed', 'failed', 'cancelled') NOT NULL,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    completed_at TIMESTAMP(6) NULL,
    PRIMARY KEY (id),
    UNIQUE KEY uq_invocation_idempotency (tenant_id, application_id, caller_type, caller_id, idempotency_key),
    KEY idx_invocations_session (tenant_id, session_id, created_at),
    CONSTRAINT fk_invocation_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_invocation_application FOREIGN KEY (application_id) REFERENCES applications(id),
    CONSTRAINT fk_invocation_session FOREIGN KEY (session_id) REFERENCES application_sessions(id),
    CONSTRAINT fk_invocation_workflow_version FOREIGN KEY (workflow_version_id) REFERENCES workflow_versions(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE application_messages (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    session_id BINARY(16) NOT NULL,
    invocation_id BINARY(16) NULL,
    sequence_number BIGINT UNSIGNED NOT NULL,
    role ENUM('user', 'assistant', 'system', 'tool') NOT NULL,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    UNIQUE KEY uq_message_sequence (tenant_id, session_id, sequence_number),
    CONSTRAINT fk_message_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_message_session FOREIGN KEY (session_id) REFERENCES application_sessions(id),
    CONSTRAINT fk_message_invocation FOREIGN KEY (invocation_id) REFERENCES application_invocations(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE application_message_parts (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    message_id BINARY(16) NOT NULL,
    part_index INT UNSIGNED NOT NULL,
    part_type ENUM('text', 'json', 'image', 'audio', 'file', 'tool_call', 'tool_result') NOT NULL,
    content_json JSON NULL,
    artifact_id BINARY(16) NULL,
    PRIMARY KEY (id),
    UNIQUE KEY uq_message_part_index (message_id, part_index),
    CONSTRAINT fk_message_part_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_message_part_message FOREIGN KEY (message_id) REFERENCES application_messages(id),
    CONSTRAINT fk_message_part_artifact FOREIGN KEY (artifact_id) REFERENCES artifacts(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE invocation_events (
    tenant_id BINARY(16) NOT NULL,
    invocation_id BINARY(16) NOT NULL,
    sequence_number BIGINT UNSIGNED NOT NULL,
    event_type VARCHAR(64) NOT NULL,
    payload_json JSON NOT NULL,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    PRIMARY KEY (tenant_id, invocation_id, sequence_number),
    CONSTRAINT fk_invocation_event_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_invocation_event_invocation FOREIGN KEY (invocation_id) REFERENCES application_invocations(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

INSERT INTO permissions (id, permission_key, name) VALUES
    (UUID_TO_BIN(UUID()), 'application:view', 'View applications'),
    (UUID_TO_BIN(UUID()), 'application:manage', 'Manage applications'),
    (UUID_TO_BIN(UUID()), 'application:invoke', 'Invoke applications'),
    (UUID_TO_BIN(UUID()), 'application:manage_key', 'Manage application API keys')
ON DUPLICATE KEY UPDATE name = VALUES(name);

INSERT IGNORE INTO role_permissions (tenant_id, role_id, permission_id)
SELECT r.tenant_id, r.id, p.id FROM roles r CROSS JOIN permissions p WHERE r.code = 'company_admin';

INSERT IGNORE INTO role_permissions (tenant_id, role_id, permission_id)
SELECT r.tenant_id, r.id, p.id FROM roles r JOIN permissions p ON p.permission_key IN ('application:view', 'application:invoke')
WHERE r.code IN ('department_admin', 'member');
