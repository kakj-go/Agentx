CREATE TABLE datasets (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    name VARCHAR(160) NOT NULL,
    description VARCHAR(1000) NULL,
    visibility ENUM('private', 'department', 'company') NOT NULL DEFAULT 'private',
    owner_user_id BINARY(16) NOT NULL,
    owner_department_id BINARY(16) NOT NULL,
    status ENUM('active', 'disabled') NOT NULL DEFAULT 'active',
    revision BIGINT UNSIGNED NOT NULL DEFAULT 0,
    version BIGINT UNSIGNED NOT NULL DEFAULT 1,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    updated_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    KEY idx_datasets_tenant (tenant_id, status, updated_at),
    KEY idx_datasets_department (tenant_id, owner_department_id),
    CONSTRAINT fk_dataset_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_dataset_owner FOREIGN KEY (owner_user_id) REFERENCES users(id),
    CONSTRAINT fk_dataset_department FOREIGN KEY (owner_department_id) REFERENCES departments(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE dataset_cases (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    dataset_id BINARY(16) NOT NULL,
    case_key VARCHAR(128) NOT NULL,
    name VARCHAR(255) NOT NULL,
    input_json JSON NOT NULL,
    expected_output_json JSON NULL,
    context_json JSON NULL,
    tags_json JSON NOT NULL,
    evaluator_override_json JSON NULL,
    sort_order BIGINT UNSIGNED NOT NULL,
    version BIGINT UNSIGNED NOT NULL DEFAULT 1,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    updated_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    UNIQUE KEY uq_dataset_case_key (tenant_id, dataset_id, case_key),
    KEY idx_dataset_cases_order (tenant_id, dataset_id, sort_order),
    CONSTRAINT fk_dataset_case_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_dataset_case_dataset FOREIGN KEY (dataset_id) REFERENCES datasets(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE dataset_versions (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    dataset_id BINARY(16) NOT NULL,
    version_number BIGINT UNSIGNED NOT NULL,
    source_revision BIGINT UNSIGNED NOT NULL,
    content_hash CHAR(64) NOT NULL,
    case_count BIGINT UNSIGNED NOT NULL,
    created_by BINARY(16) NOT NULL,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    UNIQUE KEY uq_dataset_version_number (tenant_id, dataset_id, version_number),
    UNIQUE KEY uq_dataset_version_hash (tenant_id, dataset_id, source_revision, content_hash),
    CONSTRAINT fk_dataset_version_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_dataset_version_dataset FOREIGN KEY (dataset_id) REFERENCES datasets(id),
    CONSTRAINT fk_dataset_version_creator FOREIGN KEY (created_by) REFERENCES users(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE dataset_version_cases (
    tenant_id BINARY(16) NOT NULL,
    dataset_version_id BINARY(16) NOT NULL,
    source_case_id BINARY(16) NOT NULL,
    case_key VARCHAR(128) NOT NULL,
    name VARCHAR(255) NOT NULL,
    input_json JSON NOT NULL,
    expected_output_json JSON NULL,
    context_json JSON NULL,
    tags_json JSON NOT NULL,
    evaluator_override_json JSON NULL,
    sort_order BIGINT UNSIGNED NOT NULL,
    content_hash CHAR(64) NOT NULL,
    PRIMARY KEY (tenant_id, dataset_version_id, source_case_id),
    KEY idx_dataset_version_case_order (tenant_id, dataset_version_id, sort_order),
    CONSTRAINT fk_dataset_version_case_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_dataset_version_case_version FOREIGN KEY (dataset_version_id) REFERENCES dataset_versions(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE evaluation_profiles (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    name VARCHAR(160) NOT NULL,
    description VARCHAR(1000) NULL,
    owner_user_id BINARY(16) NOT NULL,
    owner_department_id BINARY(16) NOT NULL,
    visibility ENUM('private', 'department', 'company') NOT NULL DEFAULT 'private',
    status ENUM('active', 'disabled') NOT NULL DEFAULT 'active',
    version BIGINT UNSIGNED NOT NULL DEFAULT 1,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    updated_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    KEY idx_evaluation_profiles (tenant_id, updated_at),
    CONSTRAINT fk_evaluation_profile_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_evaluation_profile_owner FOREIGN KEY (owner_user_id) REFERENCES users(id),
    CONSTRAINT fk_evaluation_profile_department FOREIGN KEY (owner_department_id) REFERENCES departments(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE evaluation_profile_versions (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    profile_id BINARY(16) NOT NULL,
    version_number BIGINT UNSIGNED NOT NULL,
    aggregation ENUM('all', 'any', 'weighted') NOT NULL DEFAULT 'all',
    pass_threshold DECIMAL(8,6) NOT NULL DEFAULT 1,
    content_hash CHAR(64) NOT NULL,
    created_by BINARY(16) NOT NULL,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    UNIQUE KEY uq_evaluation_profile_version (tenant_id, profile_id, version_number),
    UNIQUE KEY uq_evaluation_profile_version_hash (tenant_id, profile_id, content_hash),
    CONSTRAINT fk_evaluation_profile_version_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_evaluation_profile_version_profile FOREIGN KEY (profile_id) REFERENCES evaluation_profiles(id),
    CONSTRAINT fk_evaluation_profile_version_creator FOREIGN KEY (created_by) REFERENCES users(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE evaluation_profile_rules (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    profile_version_id BINARY(16) NOT NULL,
    rule_key VARCHAR(128) NOT NULL,
    name VARCHAR(160) NOT NULL,
    evaluator_type ENUM('exact', 'contains', 'regex', 'json_schema', 'llm_judge', 'custom_code') NOT NULL,
    configuration_json JSON NOT NULL,
    weight DECIMAL(10,4) NOT NULL DEFAULT 1,
    required BOOLEAN NOT NULL DEFAULT TRUE,
    sort_order INT UNSIGNED NOT NULL,
    PRIMARY KEY (id),
    UNIQUE KEY uq_evaluation_profile_rule (tenant_id, profile_version_id, rule_key),
    KEY idx_evaluation_profile_rule_order (tenant_id, profile_version_id, sort_order),
    CONSTRAINT fk_evaluation_profile_rule_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_evaluation_profile_rule_version FOREIGN KEY (profile_version_id) REFERENCES evaluation_profile_versions(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE evaluation_runs (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    name VARCHAR(160) NOT NULL,
    workflow_version_id BINARY(16) NOT NULL,
    dataset_version_id BINARY(16) NOT NULL,
    evaluation_profile_version_id BINARY(16) NOT NULL,
    parameters_json JSON NOT NULL,
    status ENUM('created', 'queued', 'running', 'completed', 'failed', 'cancelled') NOT NULL DEFAULT 'created',
    created_by BINARY(16) NOT NULL,
    owner_department_id BINARY(16) NOT NULL,
    visibility ENUM('private', 'department', 'company') NOT NULL DEFAULT 'private',
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    started_at TIMESTAMP(6) NULL,
    completed_at TIMESTAMP(6) NULL,
    PRIMARY KEY (id),
    KEY idx_evaluation_runs (tenant_id, status, created_at),
    CONSTRAINT fk_evaluation_run_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_evaluation_run_workflow_version FOREIGN KEY (workflow_version_id) REFERENCES workflow_versions(id),
    CONSTRAINT fk_evaluation_run_dataset_version FOREIGN KEY (dataset_version_id) REFERENCES dataset_versions(id),
    CONSTRAINT fk_evaluation_run_profile_version FOREIGN KEY (evaluation_profile_version_id) REFERENCES evaluation_profile_versions(id),
    CONSTRAINT fk_evaluation_run_creator FOREIGN KEY (created_by) REFERENCES users(id),
    CONSTRAINT fk_evaluation_run_department FOREIGN KEY (owner_department_id) REFERENCES departments(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE evaluation_case_results (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    evaluation_run_id BINARY(16) NOT NULL,
    source_case_id BINARY(16) NOT NULL,
    execution_id BINARY(16) NOT NULL,
    status ENUM('passed', 'failed', 'error', 'cancelled') NOT NULL,
    score DECIMAL(12,6) NULL,
    detail_json JSON NOT NULL,
    duration_ms BIGINT UNSIGNED NULL,
    cost_micros BIGINT UNSIGNED NOT NULL DEFAULT 0,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    UNIQUE KEY uq_evaluation_case_result (tenant_id, evaluation_run_id, source_case_id),
    CONSTRAINT fk_evaluation_case_result_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_evaluation_case_result_run FOREIGN KEY (evaluation_run_id) REFERENCES evaluation_runs(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE evaluation_metrics (
    tenant_id BINARY(16) NOT NULL,
    evaluation_run_id BINARY(16) NOT NULL,
    metric_key VARCHAR(128) NOT NULL,
    metric_value DECIMAL(24,8) NOT NULL,
    detail_json JSON NULL,
    PRIMARY KEY (tenant_id, evaluation_run_id, metric_key),
    CONSTRAINT fk_evaluation_metric_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_evaluation_metric_run FOREIGN KEY (evaluation_run_id) REFERENCES evaluation_runs(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE evaluation_comparisons (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    name VARCHAR(160) NOT NULL,
    baseline_run_id BINARY(16) NOT NULL,
    candidate_run_id BINARY(16) NOT NULL,
    created_by BINARY(16) NOT NULL,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    CONSTRAINT fk_evaluation_comparison_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_evaluation_comparison_baseline FOREIGN KEY (baseline_run_id) REFERENCES evaluation_runs(id),
    CONSTRAINT fk_evaluation_comparison_candidate FOREIGN KEY (candidate_run_id) REFERENCES evaluation_runs(id),
    CONSTRAINT fk_evaluation_comparison_creator FOREIGN KEY (created_by) REFERENCES users(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

INSERT INTO permissions (id, permission_key, name) VALUES
    (UUID_TO_BIN(UUID()), 'dataset:view', 'View datasets'),
    (UUID_TO_BIN(UUID()), 'dataset:manage', 'Manage datasets'),
    (UUID_TO_BIN(UUID()), 'evaluation_profile:view', 'View evaluation profiles'),
    (UUID_TO_BIN(UUID()), 'evaluation_profile:manage', 'Manage evaluation profiles'),
    (UUID_TO_BIN(UUID()), 'evaluation:view', 'View evaluations'),
    (UUID_TO_BIN(UUID()), 'evaluation:manage', 'Manage evaluations')
ON DUPLICATE KEY UPDATE name = VALUES(name);

INSERT IGNORE INTO role_permissions (tenant_id, role_id, permission_id)
SELECT r.tenant_id, r.id, p.id FROM roles r CROSS JOIN permissions p WHERE r.code = 'company_admin';

INSERT IGNORE INTO role_permissions (tenant_id, role_id, permission_id)
SELECT r.tenant_id, r.id, p.id FROM roles r JOIN permissions p ON p.permission_key IN ('dataset:view', 'evaluation_profile:view', 'evaluation:view')
WHERE r.code = 'department_admin';
