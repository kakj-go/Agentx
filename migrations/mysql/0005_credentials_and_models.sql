CREATE TABLE credentials (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    name VARCHAR(160) NOT NULL,
    credential_type ENUM('api_key', 'bearer', 'basic', 'custom_json') NOT NULL,
    storage_mode ENUM('local_encrypted', 'external_reference') NOT NULL DEFAULT 'local_encrypted',
    masked_hint VARCHAR(32) NOT NULL,
    status ENUM('active', 'disabled') NOT NULL DEFAULT 'active',
    current_secret_version BIGINT UNSIGNED NOT NULL DEFAULT 1,
    owner_department_id BINARY(16) NOT NULL,
    created_by BINARY(16) NOT NULL,
    version BIGINT UNSIGNED NOT NULL DEFAULT 1,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    updated_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    KEY idx_credentials_tenant_status (tenant_id, status, updated_at),
    CONSTRAINT fk_credentials_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_credentials_department FOREIGN KEY (owner_department_id) REFERENCES departments(id),
    CONSTRAINT fk_credentials_user FOREIGN KEY (created_by) REFERENCES users(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE credential_secret_versions (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    credential_id BINARY(16) NOT NULL,
    version_number BIGINT UNSIGNED NOT NULL,
    algorithm VARCHAR(32) NOT NULL,
    key_id VARCHAR(64) NOT NULL,
    nonce VARBINARY(32) NOT NULL,
    ciphertext MEDIUMBLOB NOT NULL,
    created_by BINARY(16) NOT NULL,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    UNIQUE KEY uq_credential_secret_version (tenant_id, credential_id, version_number),
    CONSTRAINT fk_credential_secret_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_credential_secret_credential FOREIGN KEY (credential_id) REFERENCES credentials(id),
    CONSTRAINT fk_credential_secret_user FOREIGN KEY (created_by) REFERENCES users(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE model_providers (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    name VARCHAR(160) NOT NULL,
    provider_type ENUM('openai_compatible', 'custom_http') NOT NULL,
    endpoint VARCHAR(2048) NOT NULL,
    credential_id BINARY(16) NULL,
    status ENUM('active', 'disabled') NOT NULL DEFAULT 'active',
    owner_department_id BINARY(16) NOT NULL,
    version BIGINT UNSIGNED NOT NULL DEFAULT 1,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    updated_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    KEY idx_model_providers_tenant (tenant_id, status, updated_at),
    CONSTRAINT fk_model_provider_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_model_provider_credential FOREIGN KEY (credential_id) REFERENCES credentials(id),
    CONSTRAINT fk_model_provider_department FOREIGN KEY (owner_department_id) REFERENCES departments(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE model_deployments (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    provider_id BINARY(16) NOT NULL,
    name VARCHAR(160) NOT NULL,
    model_name VARCHAR(255) NOT NULL,
    endpoint_override VARCHAR(2048) NULL,
    credential_id BINARY(16) NULL,
    default_parameters JSON NOT NULL,
    status ENUM('active', 'disabled') NOT NULL DEFAULT 'active',
    revision_number BIGINT UNSIGNED NOT NULL DEFAULT 1,
    supersedes_deployment_id BINARY(16) NULL,
    version BIGINT UNSIGNED NOT NULL DEFAULT 1,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    updated_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    KEY idx_model_deployments_provider (tenant_id, provider_id, status),
    CONSTRAINT fk_model_deployment_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_model_deployment_provider FOREIGN KEY (provider_id) REFERENCES model_providers(id),
    CONSTRAINT fk_model_deployment_credential FOREIGN KEY (credential_id) REFERENCES credentials(id)
    ,CONSTRAINT fk_model_deployment_previous FOREIGN KEY (supersedes_deployment_id) REFERENCES model_deployments(id)
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
