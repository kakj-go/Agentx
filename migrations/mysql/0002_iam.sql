CREATE TABLE tenants (
    id BINARY(16) NOT NULL,
    name VARCHAR(100) NOT NULL,
    normalized_name VARCHAR(100) NOT NULL,
    status ENUM('active', 'disabled') NOT NULL DEFAULT 'active',
    version BIGINT UNSIGNED NOT NULL DEFAULT 1,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    updated_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE tenant_settings (
    tenant_id BINARY(16) NOT NULL,
    locale VARCHAR(16) NOT NULL,
    timezone VARCHAR(64) NOT NULL,
    version BIGINT UNSIGNED NOT NULL DEFAULT 1,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    updated_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6),
    PRIMARY KEY (tenant_id),
    CONSTRAINT fk_tenant_settings_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE bootstrap_state (
    singleton_id TINYINT UNSIGNED NOT NULL,
    state ENUM('required', 'completed') NOT NULL,
    tenant_id BINARY(16) NULL,
    completed_at TIMESTAMP(6) NULL,
    PRIMARY KEY (singleton_id),
    CONSTRAINT chk_bootstrap_singleton CHECK (singleton_id = 1),
    CONSTRAINT fk_bootstrap_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

INSERT INTO bootstrap_state (singleton_id, state) VALUES (1, 'required');

CREATE TABLE departments (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    parent_id BINARY(16) NULL,
    name VARCHAR(100) NOT NULL,
    normalized_name VARCHAR(100) NOT NULL,
    is_root BOOLEAN NOT NULL DEFAULT FALSE,
    status ENUM('active', 'disabled') NOT NULL DEFAULT 'active',
    version BIGINT UNSIGNED NOT NULL DEFAULT 1,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    updated_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    UNIQUE KEY uq_departments_sibling_name (tenant_id, parent_id, normalized_name),
    KEY idx_departments_tenant_parent (tenant_id, parent_id),
    CONSTRAINT fk_departments_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_departments_parent FOREIGN KEY (parent_id) REFERENCES departments(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE department_closure (
    tenant_id BINARY(16) NOT NULL,
    ancestor_id BINARY(16) NOT NULL,
    descendant_id BINARY(16) NOT NULL,
    depth INT UNSIGNED NOT NULL,
    PRIMARY KEY (tenant_id, ancestor_id, descendant_id),
    KEY idx_department_closure_descendant (tenant_id, descendant_id, ancestor_id),
    CONSTRAINT fk_department_closure_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_department_closure_ancestor FOREIGN KEY (ancestor_id) REFERENCES departments(id),
    CONSTRAINT fk_department_closure_descendant FOREIGN KEY (descendant_id) REFERENCES departments(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE users (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    username VARCHAR(64) NOT NULL,
    username_normalized VARCHAR(64) NOT NULL,
    display_name VARCHAR(100) NOT NULL,
    status ENUM('active', 'disabled') NOT NULL DEFAULT 'active',
    password_change_required BOOLEAN NOT NULL DEFAULT FALSE,
    token_version BIGINT UNSIGNED NOT NULL DEFAULT 1,
    version BIGINT UNSIGNED NOT NULL DEFAULT 1,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    updated_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    UNIQUE KEY uq_users_tenant_username (tenant_id, username_normalized),
    KEY idx_users_tenant_status (tenant_id, status, created_at),
    CONSTRAINT fk_users_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE user_credentials (
    user_id BINARY(16) NOT NULL,
    password_hash VARCHAR(255) NOT NULL,
    password_changed_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    PRIMARY KEY (user_id),
    CONSTRAINT fk_user_credentials_user FOREIGN KEY (user_id) REFERENCES users(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE user_departments (
    tenant_id BINARY(16) NOT NULL,
    user_id BINARY(16) NOT NULL,
    department_id BINARY(16) NOT NULL,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    PRIMARY KEY (user_id),
    KEY idx_user_departments_department (tenant_id, department_id, user_id),
    CONSTRAINT fk_user_departments_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_user_departments_user FOREIGN KEY (user_id) REFERENCES users(id),
    CONSTRAINT fk_user_departments_department FOREIGN KEY (department_id) REFERENCES departments(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE roles (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    code VARCHAR(64) NOT NULL,
    name VARCHAR(100) NOT NULL,
    description VARCHAR(500) NULL,
    data_scope ENUM('company', 'department_tree', 'own') NOT NULL,
    is_builtin BOOLEAN NOT NULL DEFAULT FALSE,
    status ENUM('active', 'disabled') NOT NULL DEFAULT 'active',
    version BIGINT UNSIGNED NOT NULL DEFAULT 1,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    updated_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    UNIQUE KEY uq_roles_tenant_code (tenant_id, code),
    CONSTRAINT fk_roles_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE permissions (
    id BINARY(16) NOT NULL,
    permission_key VARCHAR(96) NOT NULL,
    name VARCHAR(100) NOT NULL,
    description VARCHAR(500) NULL,
    PRIMARY KEY (id),
    UNIQUE KEY uq_permissions_key (permission_key)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE role_permissions (
    tenant_id BINARY(16) NOT NULL,
    role_id BINARY(16) NOT NULL,
    permission_id BINARY(16) NOT NULL,
    PRIMARY KEY (role_id, permission_id),
    KEY idx_role_permissions_tenant (tenant_id, role_id),
    CONSTRAINT fk_role_permissions_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_role_permissions_role FOREIGN KEY (role_id) REFERENCES roles(id),
    CONSTRAINT fk_role_permissions_permission FOREIGN KEY (permission_id) REFERENCES permissions(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE user_roles (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    user_id BINARY(16) NOT NULL,
    role_id BINARY(16) NOT NULL,
    scope_department_id BINARY(16) NULL,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    KEY idx_user_roles_user (tenant_id, user_id),
    KEY idx_user_roles_role (tenant_id, role_id),
    CONSTRAINT fk_user_roles_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_user_roles_user FOREIGN KEY (user_id) REFERENCES users(id),
    CONSTRAINT fk_user_roles_role FOREIGN KEY (role_id) REFERENCES roles(id),
    CONSTRAINT fk_user_roles_scope_department FOREIGN KEY (scope_department_id) REFERENCES departments(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE refresh_sessions (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    user_id BINARY(16) NOT NULL,
    token_family_id BINARY(16) NOT NULL,
    jti_hash CHAR(64) NOT NULL,
    token_version BIGINT UNSIGNED NOT NULL,
    expires_at TIMESTAMP(6) NOT NULL,
    rotated_at TIMESTAMP(6) NULL,
    revoked_at TIMESTAMP(6) NULL,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    UNIQUE KEY uq_refresh_sessions_jti_hash (jti_hash),
    KEY idx_refresh_sessions_user (tenant_id, user_id, revoked_at),
    KEY idx_refresh_sessions_family (token_family_id, revoked_at),
    CONSTRAINT fk_refresh_sessions_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_refresh_sessions_user FOREIGN KEY (user_id) REFERENCES users(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE audit_events (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    actor_user_id BINARY(16) NULL,
    action VARCHAR(128) NOT NULL,
    target_type VARCHAR(128) NOT NULL,
    target_id VARCHAR(128) NOT NULL,
    request_id BINARY(16) NOT NULL,
    detail_json JSON NOT NULL,
    occurred_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    KEY idx_audit_tenant_time (tenant_id, occurred_at),
    KEY idx_audit_target (tenant_id, target_type, target_id),
    CONSTRAINT fk_audit_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_audit_actor FOREIGN KEY (actor_user_id) REFERENCES users(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

ALTER TABLE artifacts
    ADD CONSTRAINT fk_artifacts_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id);

ALTER TABLE outbox_events
    ADD CONSTRAINT fk_outbox_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id);

