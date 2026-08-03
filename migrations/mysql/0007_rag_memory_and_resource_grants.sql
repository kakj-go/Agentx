CREATE TABLE rag_connections (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    name VARCHAR(160) NOT NULL,
    endpoint VARCHAR(2048) NOT NULL,
    health_path VARCHAR(512) NOT NULL DEFAULT '/health',
    credential_id BINARY(16) NULL,
    owner_department_id BINARY(16) NOT NULL,
    configuration_json JSON NOT NULL,
    status ENUM('active', 'disabled') NOT NULL DEFAULT 'active',
    version BIGINT UNSIGNED NOT NULL DEFAULT 1,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    updated_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    KEY idx_rag_connections_tenant (tenant_id, status, updated_at),
    CONSTRAINT fk_rag_connection_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_rag_connection_credential FOREIGN KEY (credential_id) REFERENCES credentials(id),
    CONSTRAINT fk_rag_connection_department FOREIGN KEY (owner_department_id) REFERENCES departments(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE rag_resources (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    connection_id BINARY(16) NOT NULL,
    name VARCHAR(160) NOT NULL,
    external_resource_id VARCHAR(512) NOT NULL,
    owner_department_id BINARY(16) NOT NULL,
    sync_status ENUM('unknown', 'syncing', 'synced', 'failed') NOT NULL DEFAULT 'unknown',
    status ENUM('active', 'disabled') NOT NULL DEFAULT 'active',
    version BIGINT UNSIGNED NOT NULL DEFAULT 1,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    updated_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    UNIQUE KEY uq_rag_resource_external (tenant_id, connection_id, external_resource_id),
    KEY idx_rag_resources_department (tenant_id, owner_department_id, status),
    CONSTRAINT fk_rag_resource_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_rag_resource_connection FOREIGN KEY (connection_id) REFERENCES rag_connections(id),
    CONSTRAINT fk_rag_resource_department FOREIGN KEY (owner_department_id) REFERENCES departments(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE memory_connections (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    name VARCHAR(160) NOT NULL,
    endpoint VARCHAR(2048) NOT NULL,
    health_path VARCHAR(512) NOT NULL DEFAULT '/health',
    credential_id BINARY(16) NULL,
    owner_department_id BINARY(16) NOT NULL,
    configuration_json JSON NOT NULL,
    status ENUM('active', 'disabled') NOT NULL DEFAULT 'active',
    version BIGINT UNSIGNED NOT NULL DEFAULT 1,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    updated_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    KEY idx_memory_connections_tenant (tenant_id, status, updated_at),
    CONSTRAINT fk_memory_connection_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_memory_connection_credential FOREIGN KEY (credential_id) REFERENCES credentials(id),
    CONSTRAINT fk_memory_connection_department FOREIGN KEY (owner_department_id) REFERENCES departments(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE memory_namespaces (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    connection_id BINARY(16) NOT NULL,
    name VARCHAR(160) NOT NULL,
    external_namespace VARCHAR(512) NOT NULL,
    access_mode ENUM('read', 'read_write') NOT NULL DEFAULT 'read',
    owner_department_id BINARY(16) NOT NULL,
    status ENUM('active', 'disabled') NOT NULL DEFAULT 'active',
    version BIGINT UNSIGNED NOT NULL DEFAULT 1,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    updated_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    UNIQUE KEY uq_memory_namespace_external (tenant_id, connection_id, external_namespace),
    KEY idx_memory_namespaces_department (tenant_id, owner_department_id, status),
    CONSTRAINT fk_memory_namespace_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_memory_namespace_connection FOREIGN KEY (connection_id) REFERENCES memory_connections(id),
    CONSTRAINT fk_memory_namespace_department FOREIGN KEY (owner_department_id) REFERENCES departments(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE resource_grants (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    subject_type ENUM('department', 'workflow_service_identity') NOT NULL,
    subject_id BINARY(16) NOT NULL,
    resource_type VARCHAR(32) NOT NULL,
    resource_id BINARY(16) NOT NULL,
    resource_version_id BINARY(16) NULL,
    operation_key ENUM('view', 'use', 'read', 'write', 'manage') NOT NULL,
    created_by BINARY(16) NOT NULL,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    UNIQUE KEY uq_resource_grant (tenant_id, subject_type, subject_id, resource_type, resource_id, operation_key),
    KEY idx_resource_grant_resource (tenant_id, resource_type, resource_id),
    KEY idx_resource_grant_subject (tenant_id, subject_type, subject_id),
    CONSTRAINT fk_resource_grant_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_resource_grant_user FOREIGN KEY (created_by) REFERENCES users(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE resource_health_checks (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    resource_type VARCHAR(32) NOT NULL,
    resource_id BINARY(16) NOT NULL,
    check_sequence BIGINT UNSIGNED NOT NULL,
    status ENUM('healthy', 'unhealthy', 'unsupported') NOT NULL,
    latency_ms BIGINT UNSIGNED NULL,
    error_code VARCHAR(128) NULL,
    error_message VARCHAR(512) NULL,
    checked_by BINARY(16) NOT NULL,
    checked_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    UNIQUE KEY uq_resource_health_sequence (tenant_id, resource_type, resource_id, check_sequence),
    KEY idx_resource_health_latest (tenant_id, resource_type, resource_id, checked_at),
    CONSTRAINT fk_resource_health_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_resource_health_user FOREIGN KEY (checked_by) REFERENCES users(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

INSERT INTO permissions (id, permission_key, name) VALUES
    (UUID_TO_BIN(UUID()), 'workflow:view', 'View workflows'),
    (UUID_TO_BIN(UUID()), 'workflow:create', 'Create workflows'),
    (UUID_TO_BIN(UUID()), 'workflow:edit', 'Edit workflows'),
    (UUID_TO_BIN(UUID()), 'workflow:archive', 'Archive workflows'),
    (UUID_TO_BIN(UUID()), 'workflow:publish', 'Publish workflows'),
    (UUID_TO_BIN(UUID()), 'workflow:manage_member', 'Manage workflow members'),
    (UUID_TO_BIN(UUID()), 'workflow:manage_permission', 'Manage workflow permissions'),
    (UUID_TO_BIN(UUID()), 'credential:view', 'View credentials'),
    (UUID_TO_BIN(UUID()), 'credential:manage', 'Manage credentials'),
    (UUID_TO_BIN(UUID()), 'model:view', 'View models'),
    (UUID_TO_BIN(UUID()), 'model:manage', 'Manage models'),
    (UUID_TO_BIN(UUID()), 'mcp:view', 'View MCP servers and tools'),
    (UUID_TO_BIN(UUID()), 'mcp:manage', 'Manage MCP servers and tool policies'),
    (UUID_TO_BIN(UUID()), 'mcp:discover', 'Discover MCP tools'),
    (UUID_TO_BIN(UUID()), 'mcp:debug', 'Debug MCP tools'),
    (UUID_TO_BIN(UUID()), 'skill:view', 'View skills'),
    (UUID_TO_BIN(UUID()), 'skill:manage', 'Manage skills'),
    (UUID_TO_BIN(UUID()), 'knowledge:view', 'View knowledge resources'),
    (UUID_TO_BIN(UUID()), 'knowledge:manage', 'Manage knowledge resources'),
    (UUID_TO_BIN(UUID()), 'memory:view', 'View memory resources'),
    (UUID_TO_BIN(UUID()), 'memory:manage', 'Manage memory resources'),
    (UUID_TO_BIN(UUID()), 'resource:grant', 'Grant resources')
ON DUPLICATE KEY UPDATE name = VALUES(name);

INSERT IGNORE INTO role_permissions (tenant_id, role_id, permission_id)
SELECT r.tenant_id, r.id, p.id
FROM roles r CROSS JOIN permissions p
WHERE r.code = 'company_admin';

INSERT IGNORE INTO role_permissions (tenant_id, role_id, permission_id)
SELECT r.tenant_id, r.id, p.id
FROM roles r JOIN permissions p ON p.permission_key IN (
    'workflow:view', 'credential:view', 'model:view', 'mcp:view',
    'skill:view', 'knowledge:view', 'memory:view'
)
WHERE r.code = 'department_admin';
