CREATE TABLE workflows (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    name VARCHAR(160) NOT NULL,
    description VARCHAR(1000) NULL,
    status ENUM('active', 'archived') NOT NULL DEFAULT 'active',
    visibility ENUM('private', 'department', 'company') NOT NULL DEFAULT 'private',
    owner_user_id BINARY(16) NOT NULL,
    owner_department_id BINARY(16) NOT NULL,
    version BIGINT UNSIGNED NOT NULL DEFAULT 1,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    updated_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6),
    archived_at TIMESTAMP(6) NULL,
    PRIMARY KEY (id),
    KEY idx_workflows_tenant_status (tenant_id, status, updated_at),
    KEY idx_workflows_department (tenant_id, owner_department_id, status),
    CONSTRAINT fk_workflows_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_workflows_owner FOREIGN KEY (owner_user_id) REFERENCES users(id),
    CONSTRAINT fk_workflows_department FOREIGN KEY (owner_department_id) REFERENCES departments(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE workflow_service_identities (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    workflow_id BINARY(16) NOT NULL,
    status ENUM('active', 'disabled') NOT NULL DEFAULT 'active',
    version BIGINT UNSIGNED NOT NULL DEFAULT 1,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    updated_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    UNIQUE KEY uq_workflow_service_identity (tenant_id, workflow_id),
    CONSTRAINT fk_workflow_identity_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_workflow_identity_workflow FOREIGN KEY (workflow_id) REFERENCES workflows(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE workflow_members (
    tenant_id BINARY(16) NOT NULL,
    workflow_id BINARY(16) NOT NULL,
    user_id BINARY(16) NOT NULL,
    member_role ENUM('viewer', 'editor', 'manager') NOT NULL,
    created_by BINARY(16) NOT NULL,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    PRIMARY KEY (workflow_id, user_id),
    KEY idx_workflow_members_user (tenant_id, user_id, workflow_id),
    CONSTRAINT fk_workflow_members_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_workflow_members_workflow FOREIGN KEY (workflow_id) REFERENCES workflows(id),
    CONSTRAINT fk_workflow_members_user FOREIGN KEY (user_id) REFERENCES users(id),
    CONSTRAINT fk_workflow_members_creator FOREIGN KEY (created_by) REFERENCES users(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE workflow_drafts (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    workflow_id BINARY(16) NOT NULL,
    schema_version VARCHAR(32) NOT NULL,
    revision BIGINT UNSIGNED NOT NULL DEFAULT 0,
    definition_json JSON NOT NULL,
    content_hash VARCHAR(80) NOT NULL,
    updated_by BINARY(16) NOT NULL,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    updated_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    UNIQUE KEY uq_workflow_draft (tenant_id, workflow_id),
    CONSTRAINT fk_workflow_draft_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_workflow_draft_workflow FOREIGN KEY (workflow_id) REFERENCES workflows(id),
    CONSTRAINT fk_workflow_draft_user FOREIGN KEY (updated_by) REFERENCES users(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE workflow_draft_revisions (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    workflow_id BINARY(16) NOT NULL,
    draft_id BINARY(16) NOT NULL,
    revision BIGINT UNSIGNED NOT NULL,
    schema_version VARCHAR(32) NOT NULL,
    definition_json JSON NOT NULL,
    content_hash VARCHAR(80) NOT NULL,
    created_by BINARY(16) NOT NULL,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    UNIQUE KEY uq_workflow_draft_revision (tenant_id, workflow_id, revision),
    KEY idx_workflow_revisions_time (tenant_id, workflow_id, created_at),
    CONSTRAINT fk_workflow_revision_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_workflow_revision_workflow FOREIGN KEY (workflow_id) REFERENCES workflows(id),
    CONSTRAINT fk_workflow_revision_draft FOREIGN KEY (draft_id) REFERENCES workflow_drafts(id),
    CONSTRAINT fk_workflow_revision_user FOREIGN KEY (created_by) REFERENCES users(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE workflow_versions (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    workflow_id BINARY(16) NOT NULL,
    version_number BIGINT UNSIGNED NOT NULL,
    source_revision BIGINT UNSIGNED NOT NULL,
    schema_version VARCHAR(32) NOT NULL,
    definition_json JSON NOT NULL,
    content_hash VARCHAR(80) NOT NULL,
    created_by BINARY(16) NOT NULL,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    UNIQUE KEY uq_workflow_version_number (tenant_id, workflow_id, version_number),
    UNIQUE KEY uq_workflow_version_content (tenant_id, workflow_id, source_revision, content_hash),
    CONSTRAINT fk_workflow_version_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_workflow_version_workflow FOREIGN KEY (workflow_id) REFERENCES workflows(id),
    CONSTRAINT fk_workflow_version_user FOREIGN KEY (created_by) REFERENCES users(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE workflow_version_resources (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    workflow_version_id BINARY(16) NOT NULL,
    node_id VARCHAR(128) NOT NULL,
    resource_type VARCHAR(32) NOT NULL,
    resource_id BINARY(16) NOT NULL,
    resource_version_id BINARY(16) NULL,
    operation_key VARCHAR(32) NOT NULL,
    snapshot_json JSON NOT NULL,
    snapshot_hash VARCHAR(80) NOT NULL,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    UNIQUE KEY uq_workflow_version_resource (workflow_version_id, node_id, resource_type, resource_id, operation_key),
    KEY idx_workflow_version_resources_lookup (tenant_id, resource_type, resource_id),
    CONSTRAINT fk_workflow_version_resource_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_workflow_version_resource_version FOREIGN KEY (workflow_version_id) REFERENCES workflow_versions(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE workflow_environments (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    code VARCHAR(64) NOT NULL,
    name VARCHAR(100) NOT NULL,
    is_builtin BOOLEAN NOT NULL DEFAULT FALSE,
    status ENUM('active', 'disabled') NOT NULL DEFAULT 'active',
    version BIGINT UNSIGNED NOT NULL DEFAULT 1,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    updated_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    UNIQUE KEY uq_workflow_environment_code (tenant_id, code),
    CONSTRAINT fk_workflow_environment_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

INSERT INTO workflow_environments (id, tenant_id, code, name, is_builtin)
SELECT UUID_TO_BIN(UUID()), id, 'development', 'Development', TRUE FROM tenants;
INSERT INTO workflow_environments (id, tenant_id, code, name, is_builtin)
SELECT UUID_TO_BIN(UUID()), id, 'production', 'Production', TRUE FROM tenants;

CREATE TABLE workflow_deployments (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    workflow_id BINARY(16) NOT NULL,
    environment_id BINARY(16) NOT NULL,
    workflow_version_id BINARY(16) NOT NULL,
    sequence_number BIGINT UNSIGNED NOT NULL,
    status ENUM('active', 'superseded', 'rolled_back') NOT NULL DEFAULT 'active',
    source ENUM('publish', 'rollback') NOT NULL DEFAULT 'publish',
    created_by BINARY(16) NOT NULL,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    UNIQUE KEY uq_workflow_deployment_sequence (tenant_id, workflow_id, environment_id, sequence_number),
    KEY idx_workflow_deployment_history (tenant_id, workflow_id, environment_id, created_at),
    CONSTRAINT fk_workflow_deployment_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_workflow_deployment_workflow FOREIGN KEY (workflow_id) REFERENCES workflows(id),
    CONSTRAINT fk_workflow_deployment_environment FOREIGN KEY (environment_id) REFERENCES workflow_environments(id),
    CONSTRAINT fk_workflow_deployment_version FOREIGN KEY (workflow_version_id) REFERENCES workflow_versions(id),
    CONSTRAINT fk_workflow_deployment_user FOREIGN KEY (created_by) REFERENCES users(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE workflow_deployment_heads (
    tenant_id BINARY(16) NOT NULL,
    workflow_id BINARY(16) NOT NULL,
    environment_id BINARY(16) NOT NULL,
    active_deployment_id BINARY(16) NOT NULL,
    version BIGINT UNSIGNED NOT NULL DEFAULT 1,
    updated_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6),
    PRIMARY KEY (tenant_id, workflow_id, environment_id),
    UNIQUE KEY uq_workflow_active_deployment (active_deployment_id),
    CONSTRAINT fk_workflow_head_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_workflow_head_workflow FOREIGN KEY (workflow_id) REFERENCES workflows(id),
    CONSTRAINT fk_workflow_head_environment FOREIGN KEY (environment_id) REFERENCES workflow_environments(id),
    CONSTRAINT fk_workflow_head_deployment FOREIGN KEY (active_deployment_id) REFERENCES workflow_deployments(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE deployment_history (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    workflow_id BINARY(16) NOT NULL,
    environment_id BINARY(16) NOT NULL,
    deployment_id BINARY(16) NOT NULL,
    action ENUM('published', 'superseded', 'rolled_back') NOT NULL,
    actor_user_id BINARY(16) NOT NULL,
    occurred_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    KEY idx_deployment_history (tenant_id, workflow_id, environment_id, occurred_at),
    CONSTRAINT fk_deployment_history_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_deployment_history_workflow FOREIGN KEY (workflow_id) REFERENCES workflows(id),
    CONSTRAINT fk_deployment_history_environment FOREIGN KEY (environment_id) REFERENCES workflow_environments(id),
    CONSTRAINT fk_deployment_history_deployment FOREIGN KEY (deployment_id) REFERENCES workflow_deployments(id),
    CONSTRAINT fk_deployment_history_actor FOREIGN KEY (actor_user_id) REFERENCES users(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE idempotency_records (
    tenant_id BINARY(16) NOT NULL,
    operation_key VARCHAR(128) NOT NULL,
    idempotency_key VARCHAR(128) NOT NULL,
    request_hash CHAR(64) NOT NULL,
    resource_id BINARY(16) NULL,
    response_json JSON NULL,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    expires_at TIMESTAMP(6) NOT NULL,
    PRIMARY KEY (tenant_id, operation_key, idempotency_key),
    KEY idx_idempotency_expiry (expires_at),
    CONSTRAINT fk_idempotency_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;
