CREATE TABLE mcp_servers (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    name VARCHAR(160) NOT NULL,
    description VARCHAR(1000) NULL,
    owner_department_id BINARY(16) NOT NULL,
    current_version_number BIGINT UNSIGNED NOT NULL DEFAULT 1,
    status ENUM('active', 'disabled') NOT NULL DEFAULT 'active',
    version BIGINT UNSIGNED NOT NULL DEFAULT 1,
    last_discovered_at TIMESTAMP(6) NULL,
    created_by BINARY(16) NOT NULL,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    updated_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    UNIQUE KEY uq_mcp_server_name (tenant_id, name),
    KEY idx_mcp_servers_tenant (tenant_id, status, updated_at),
    CONSTRAINT fk_mcp_server_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_mcp_server_department FOREIGN KEY (owner_department_id) REFERENCES departments(id),
    CONSTRAINT fk_mcp_server_user FOREIGN KEY (created_by) REFERENCES users(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE mcp_server_versions (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    server_id BINARY(16) NOT NULL,
    version_number BIGINT UNSIGNED NOT NULL,
    transport ENUM('streamable_http', 'sse') NOT NULL,
    endpoint VARCHAR(2048) NOT NULL,
    credential_id BINARY(16) NULL,
    configuration_json JSON NOT NULL,
    configuration_hash CHAR(64) NOT NULL,
    created_by BINARY(16) NOT NULL,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    UNIQUE KEY uq_mcp_server_version (tenant_id, server_id, version_number),
    UNIQUE KEY uq_mcp_server_version_hash (tenant_id, server_id, configuration_hash),
    CONSTRAINT fk_mcp_server_version_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_mcp_server_version_server FOREIGN KEY (server_id) REFERENCES mcp_servers(id),
    CONSTRAINT fk_mcp_server_version_credential FOREIGN KEY (credential_id) REFERENCES credentials(id),
    CONSTRAINT fk_mcp_server_version_user FOREIGN KEY (created_by) REFERENCES users(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE mcp_discovery_runs (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    server_id BINARY(16) NOT NULL,
    server_version_id BINARY(16) NOT NULL,
    status ENUM('running', 'succeeded', 'failed') NOT NULL,
    discovered_count INT UNSIGNED NOT NULL DEFAULT 0,
    error_code VARCHAR(128) NULL,
    error_message VARCHAR(512) NULL,
    started_by BINARY(16) NOT NULL,
    started_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    finished_at TIMESTAMP(6) NULL,
    PRIMARY KEY (id),
    KEY idx_mcp_discovery_server (tenant_id, server_id, started_at),
    CONSTRAINT fk_mcp_discovery_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_mcp_discovery_server FOREIGN KEY (server_id) REFERENCES mcp_servers(id),
    CONSTRAINT fk_mcp_discovery_version FOREIGN KEY (server_version_id) REFERENCES mcp_server_versions(id),
    CONSTRAINT fk_mcp_discovery_user FOREIGN KEY (started_by) REFERENCES users(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE mcp_tools (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    server_id BINARY(16) NOT NULL,
    name VARCHAR(255) NOT NULL,
    title VARCHAR(255) NULL,
    description TEXT NULL,
    current_version_number BIGINT UNSIGNED NOT NULL,
    availability ENUM('available', 'unavailable') NOT NULL DEFAULT 'available',
    version BIGINT UNSIGNED NOT NULL DEFAULT 1,
    last_seen_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    updated_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    UNIQUE KEY uq_mcp_tool_name (tenant_id, server_id, name),
    KEY idx_mcp_tools_server (tenant_id, server_id, availability),
    CONSTRAINT fk_mcp_tool_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_mcp_tool_server FOREIGN KEY (server_id) REFERENCES mcp_servers(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE mcp_tool_versions (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    tool_id BINARY(16) NOT NULL,
    discovery_run_id BINARY(16) NOT NULL,
    version_number BIGINT UNSIGNED NOT NULL,
    input_schema JSON NOT NULL,
    output_schema JSON NULL,
    annotations_json JSON NOT NULL,
    schema_hash CHAR(64) NOT NULL,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    UNIQUE KEY uq_mcp_tool_version (tenant_id, tool_id, version_number),
    UNIQUE KEY uq_mcp_tool_schema_hash (tenant_id, tool_id, schema_hash),
    CONSTRAINT fk_mcp_tool_version_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_mcp_tool_version_tool FOREIGN KEY (tool_id) REFERENCES mcp_tools(id),
    CONSTRAINT fk_mcp_tool_version_discovery FOREIGN KEY (discovery_run_id) REFERENCES mcp_discovery_runs(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE mcp_tool_policies (
    tenant_id BINARY(16) NOT NULL,
    tool_id BINARY(16) NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    debug_enabled BOOLEAN NOT NULL DEFAULT TRUE,
    timeout_seconds INT UNSIGNED NOT NULL DEFAULT 30,
    side_effect ENUM('unknown', 'none', 'read_only', 'idempotent', 'non_idempotent', 'irreversible') NOT NULL DEFAULT 'unknown',
    version BIGINT UNSIGNED NOT NULL DEFAULT 1,
    updated_by BINARY(16) NOT NULL,
    updated_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6),
    PRIMARY KEY (tenant_id, tool_id),
    CONSTRAINT fk_mcp_policy_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_mcp_policy_tool FOREIGN KEY (tool_id) REFERENCES mcp_tools(id),
    CONSTRAINT fk_mcp_policy_user FOREIGN KEY (updated_by) REFERENCES users(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE skills (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    name VARCHAR(160) NOT NULL,
    description VARCHAR(1000) NULL,
    owner_department_id BINARY(16) NOT NULL,
    status ENUM('draft', 'active', 'disabled') NOT NULL DEFAULT 'draft',
    draft_revision BIGINT UNSIGNED NOT NULL DEFAULT 1,
    version BIGINT UNSIGNED NOT NULL DEFAULT 1,
    created_by BINARY(16) NOT NULL,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    updated_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    UNIQUE KEY uq_skill_name (tenant_id, name),
    KEY idx_skills_tenant_status (tenant_id, status, updated_at),
    CONSTRAINT fk_skills_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_skills_department FOREIGN KEY (owner_department_id) REFERENCES departments(id),
    CONSTRAINT fk_skills_user FOREIGN KEY (created_by) REFERENCES users(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE skill_workspace_entries (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    skill_id BINARY(16) NOT NULL,
    parent_id BINARY(16) NULL,
    name VARCHAR(255) NOT NULL,
    path VARCHAR(2048) NOT NULL,
    path_hash CHAR(64) NOT NULL,
    entry_type ENUM('directory', 'file') NOT NULL,
    mime_type VARCHAR(255) NULL,
    artifact_id BINARY(16) NULL,
    content_hash CHAR(64) NULL,
    size_bytes BIGINT UNSIGNED NOT NULL DEFAULT 0,
    editable BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    updated_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    UNIQUE KEY uq_skill_workspace_path (tenant_id, skill_id, path_hash),
    KEY idx_skill_workspace_parent (tenant_id, skill_id, parent_id, name),
    CONSTRAINT fk_skill_workspace_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_skill_workspace_skill FOREIGN KEY (skill_id) REFERENCES skills(id),
    CONSTRAINT fk_skill_workspace_parent FOREIGN KEY (parent_id) REFERENCES skill_workspace_entries(id),
    CONSTRAINT fk_skill_workspace_artifact FOREIGN KEY (artifact_id) REFERENCES artifacts(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE skill_file_revisions (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    skill_id BINARY(16) NOT NULL,
    entry_id BINARY(16) NOT NULL,
    workspace_revision BIGINT UNSIGNED NOT NULL,
    artifact_id BINARY(16) NOT NULL,
    content_hash CHAR(64) NOT NULL,
    size_bytes BIGINT UNSIGNED NOT NULL,
    created_by BINARY(16) NOT NULL,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    UNIQUE KEY uq_skill_file_revision (tenant_id, skill_id, entry_id, workspace_revision),
    CONSTRAINT fk_skill_file_revision_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_skill_file_revision_skill FOREIGN KEY (skill_id) REFERENCES skills(id),
    CONSTRAINT fk_skill_file_revision_entry FOREIGN KEY (entry_id) REFERENCES skill_workspace_entries(id) ON DELETE CASCADE,
    CONSTRAINT fk_skill_file_revision_artifact FOREIGN KEY (artifact_id) REFERENCES artifacts(id),
    CONSTRAINT fk_skill_file_revision_user FOREIGN KEY (created_by) REFERENCES users(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE skill_versions (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    skill_id BINARY(16) NOT NULL,
    version_number BIGINT UNSIGNED NOT NULL,
    source_revision BIGINT UNSIGNED NOT NULL,
    manifest_json JSON NOT NULL,
    content_hash VARCHAR(80) NOT NULL,
    created_by BINARY(16) NOT NULL,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    UNIQUE KEY uq_skill_version (tenant_id, skill_id, version_number),
    UNIQUE KEY uq_skill_version_hash (tenant_id, skill_id, content_hash),
    CONSTRAINT fk_skill_version_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_skill_version_skill FOREIGN KEY (skill_id) REFERENCES skills(id),
    CONSTRAINT fk_skill_version_user FOREIGN KEY (created_by) REFERENCES users(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE skill_version_files (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    skill_version_id BINARY(16) NOT NULL,
    path VARCHAR(2048) NOT NULL,
    path_hash CHAR(64) NOT NULL,
    mime_type VARCHAR(255) NOT NULL,
    artifact_id BINARY(16) NOT NULL,
    content_hash CHAR(64) NOT NULL,
    size_bytes BIGINT UNSIGNED NOT NULL,
    PRIMARY KEY (id),
    UNIQUE KEY uq_skill_version_file (tenant_id, skill_version_id, path_hash),
    CONSTRAINT fk_skill_version_file_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_skill_version_file_version FOREIGN KEY (skill_version_id) REFERENCES skill_versions(id),
    CONSTRAINT fk_skill_version_file_artifact FOREIGN KEY (artifact_id) REFERENCES artifacts(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE skill_file_references (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    skill_version_id BINARY(16) NOT NULL,
    source_path VARCHAR(2048) NOT NULL,
    target_path VARCHAR(2048) NOT NULL,
    target_path_hash CHAR(64) NOT NULL,
    target_content_hash CHAR(64) NOT NULL,
    reference_type ENUM('link', 'image') NOT NULL,
    PRIMARY KEY (id),
    KEY idx_skill_reference_target (tenant_id, skill_version_id, target_path_hash),
    CONSTRAINT fk_skill_reference_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_skill_reference_version FOREIGN KEY (skill_version_id) REFERENCES skill_versions(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE skill_dependencies (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    skill_version_id BINARY(16) NOT NULL,
    resource_type VARCHAR(32) NOT NULL,
    resource_id BINARY(16) NOT NULL,
    resource_version_id BINARY(16) NULL,
    operation_key VARCHAR(32) NOT NULL DEFAULT 'use',
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    UNIQUE KEY uq_skill_dependency (skill_version_id, resource_type, resource_id, operation_key),
    KEY idx_skill_dependency_resource (tenant_id, resource_type, resource_id),
    CONSTRAINT fk_skill_dependency_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_skill_dependency_version FOREIGN KEY (skill_version_id) REFERENCES skill_versions(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;
