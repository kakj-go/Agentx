DELETE FROM skill_dependencies WHERE resource_type = 'model';
DELETE FROM resource_grants WHERE resource_type = 'model';
DELETE FROM resource_health_checks WHERE resource_type = 'model';

DROP TABLE model_alias_deployment_history;
DROP TABLE model_price_versions;
DROP TABLE model_aliases;
DROP TABLE model_deployments;
DROP TABLE model_providers;

CREATE TABLE model_deployments (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    connection_name VARCHAR(160) NOT NULL,
    provider_type ENUM('openai_compatible', 'custom_http') NOT NULL,
    endpoint VARCHAR(2048) NOT NULL,
    credential_id BINARY(16) NULL,
    owner_department_id BINARY(16) NOT NULL,
    model_name VARCHAR(255) NOT NULL,
    max_input_tokens BIGINT UNSIGNED NOT NULL DEFAULT 1050000,
    max_output_tokens BIGINT UNSIGNED NOT NULL DEFAULT 128000,
    default_parameters JSON NOT NULL,
    status ENUM('active', 'disabled') NOT NULL DEFAULT 'active',
    revision_number BIGINT UNSIGNED NOT NULL DEFAULT 1,
    supersedes_deployment_id BINARY(16) NULL,
    version BIGINT UNSIGNED NOT NULL DEFAULT 1,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    updated_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    KEY idx_model_deployments_department (tenant_id, owner_department_id, status, updated_at),
    KEY idx_model_deployments_credential (tenant_id, credential_id),
    CONSTRAINT fk_model_deployment_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_model_deployment_credential FOREIGN KEY (credential_id) REFERENCES credentials(id),
    CONSTRAINT fk_model_deployment_department FOREIGN KEY (owner_department_id) REFERENCES departments(id),
    CONSTRAINT fk_model_deployment_previous FOREIGN KEY (supersedes_deployment_id) REFERENCES model_deployments(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE model_aliases (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    alias VARCHAR(128) NOT NULL,
    deployment_id BINARY(16) NOT NULL,
    status ENUM('active', 'disabled') NOT NULL DEFAULT 'active',
    version BIGINT UNSIGNED NOT NULL DEFAULT 1,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    updated_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    UNIQUE KEY uq_model_alias (tenant_id, alias),
    CONSTRAINT fk_model_alias_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_model_alias_deployment FOREIGN KEY (deployment_id) REFERENCES model_deployments(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE model_price_versions (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    deployment_id BINARY(16) NOT NULL,
    version_number BIGINT UNSIGNED NOT NULL,
    currency CHAR(3) NOT NULL,
    input_per_million DECIMAL(20,8) NOT NULL,
    output_per_million DECIMAL(20,8) NOT NULL,
    created_by BINARY(16) NOT NULL,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    UNIQUE KEY uq_model_price_version (tenant_id, deployment_id, version_number),
    CONSTRAINT fk_model_price_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_model_price_deployment FOREIGN KEY (deployment_id) REFERENCES model_deployments(id),
    CONSTRAINT fk_model_price_user FOREIGN KEY (created_by) REFERENCES users(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE model_alias_deployment_history (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    alias_id BINARY(16) NOT NULL,
    previous_deployment_id BINARY(16) NULL,
    deployment_id BINARY(16) NOT NULL,
    changed_by BINARY(16) NOT NULL,
    changed_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    KEY idx_model_alias_history (tenant_id, alias_id, changed_at),
    CONSTRAINT fk_model_alias_history_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_model_alias_history_alias FOREIGN KEY (alias_id) REFERENCES model_aliases(id),
    CONSTRAINT fk_model_alias_history_previous FOREIGN KEY (previous_deployment_id) REFERENCES model_deployments(id),
    CONSTRAINT fk_model_alias_history_deployment FOREIGN KEY (deployment_id) REFERENCES model_deployments(id),
    CONSTRAINT fk_model_alias_history_user FOREIGN KEY (changed_by) REFERENCES users(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;
