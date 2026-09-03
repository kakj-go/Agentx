-- V2-03 production Gateway, user admission, session and Trigger runtime state.
SET NAMES utf8mb4;
SET time_zone = '+00:00';

ALTER TABLE application_routes
    ADD COLUMN runtime_config_revision BIGINT UNSIGNED NOT NULL DEFAULT 1 AFTER admission_epoch,
    ADD COLUMN runtime_policy_json JSON NULL AFTER runtime_config_revision;

ALTER TABLE application_sessions
    ADD COLUMN bundle_id BINARY(16) NULL AFTER workflow_version_id,
    ADD COLUMN head_bundle_id BINARY(16) NULL AFTER bundle_id,
    ADD COLUMN next_message_sequence BIGINT UNSIGNED NOT NULL DEFAULT 1 AFTER version,
    ADD KEY idx_runtime_session_bundle (tenant_id, bundle_id, status),
    ADD KEY idx_runtime_session_cursor (tenant_id, application_id, updated_at, id);

ALTER TABLE application_session_version_history
    ADD COLUMN from_bundle_id BINARY(16) NULL AFTER from_workflow_version_id,
    ADD COLUMN to_bundle_id BINARY(16) NULL AFTER to_workflow_version_id,
    ADD COLUMN from_session_version BIGINT UNSIGNED NOT NULL DEFAULT 1 AFTER to_bundle_id,
    ADD COLUMN to_session_version BIGINT UNSIGNED NOT NULL DEFAULT 2 AFTER from_session_version,
    ADD COLUMN command_idempotency_key VARCHAR(192) NULL AFTER changed_by,
    ADD UNIQUE KEY uq_session_upgrade_command (tenant_id, session_id, command_idempotency_key);

ALTER TABLE application_messages
    ADD COLUMN request_hash CHAR(71) NULL AFTER invocation_id,
    ADD COLUMN idempotency_key VARCHAR(192) NULL AFTER request_hash,
    ADD UNIQUE KEY uq_runtime_message_idempotency
        (tenant_id, session_id, idempotency_key);

ALTER TABLE application_invocations
    MODIFY caller_type ENUM('user','api_key','webhook','schedule','poll','lifecycle') NOT NULL,
    ADD COLUMN caller_token_version BIGINT UNSIGNED NULL AFTER caller_id,
    ADD COLUMN chat_mapping_version BIGINT UNSIGNED NULL AFTER caller_token_version,
    ADD COLUMN chat_mapping_json JSON NULL AFTER chat_mapping_version,
    ADD KEY idx_runtime_invocation_query (tenant_id, application_id, created_at, id),
    ADD KEY idx_runtime_invocation_execution (tenant_id, execution_id);

ALTER TABLE invocation_events
    ADD COLUMN event_id BINARY(16) NOT NULL AFTER invocation_id,
    ADD UNIQUE KEY uq_invocation_event_id (tenant_id, event_id),
    ADD KEY idx_invocation_event_replay (tenant_id, invocation_id, sequence_number, created_at);

ALTER TABLE runtime_idempotency_keys
    ADD COLUMN http_status SMALLINT UNSIGNED NULL AFTER status,
    ADD COLUMN response_headers_json JSON NULL AFTER http_status,
    MODIFY expires_at TIMESTAMP(6) NOT NULL DEFAULT (CURRENT_TIMESTAMP(6) + INTERVAL 14 DAY);

ALTER TABLE artifacts
    ADD COLUMN idempotency_scope VARCHAR(96) NULL AFTER storage_key,
    ADD COLUMN idempotency_key VARCHAR(192) NULL AFTER idempotency_scope,
    ADD COLUMN request_hash CHAR(71) NULL AFTER idempotency_key,
    ADD UNIQUE KEY uq_runtime_artifact_idempotency
        (tenant_id, idempotency_scope, idempotency_key);

ALTER TABLE webhook_bindings
    DROP COLUMN secret_hash,
    ADD COLUMN configuration_revision BIGINT UNSIGNED NOT NULL DEFAULT 1 AFTER bundle_id,
    ADD COLUMN configuration_hash CHAR(71) NOT NULL AFTER configuration_revision,
    ADD COLUMN secret_ref_json JSON NOT NULL AFTER public_id,
    ADD COLUMN activated_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6) AFTER status;

ALTER TABLE schedule_bindings
    ADD COLUMN trigger_revision BIGINT UNSIGNED NOT NULL DEFAULT 1 AFTER bundle_id,
    ADD COLUMN configuration_hash CHAR(71) NOT NULL AFTER trigger_revision,
    ADD COLUMN last_cursor_at TIMESTAMP(6) NULL AFTER next_fire_at,
    ADD COLUMN heartbeat_at TIMESTAMP(6) NULL AFTER locked_until,
    ADD COLUMN last_error VARCHAR(1000) NULL AFTER fencing_token;

ALTER TABLE trigger_bindings
    ADD COLUMN bundle_id BINARY(16) NULL AFTER application_deployment_id,
    ADD COLUMN configuration_revision BIGINT UNSIGNED NOT NULL DEFAULT 1 AFTER node_id,
    ADD COLUMN configuration_hash CHAR(71) NULL AFTER configuration_revision,
    ADD COLUMN cursor_value VARCHAR(1024) NULL AFTER configuration_json,
    ADD COLUMN heartbeat_at TIMESTAMP(6) NULL AFTER locked_until,
    ADD COLUMN fencing_token BIGINT UNSIGNED NOT NULL DEFAULT 0 AFTER heartbeat_at,
    ADD COLUMN activated_at TIMESTAMP(6) NULL AFTER last_error,
    ADD KEY idx_trigger_bundle (tenant_id, bundle_id, status);


CREATE TABLE runtime_user_admission (
    tenant_id BINARY(16) NOT NULL,
    user_id BINARY(16) NOT NULL,
    user_name VARCHAR(255) NOT NULL,
    department_id BINARY(16) NOT NULL,
    department_name VARCHAR(255) NOT NULL,
    token_version BIGINT UNSIGNED NOT NULL,
    status ENUM('active','disabled') NOT NULL,
    admission_epoch BIGINT UNSIGNED NOT NULL,
    updated_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6),
    PRIMARY KEY (tenant_id, user_id),
    KEY idx_runtime_user_status (tenant_id, status, token_version)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE runtime_user_application_grants (
    tenant_id BINARY(16) NOT NULL,
    user_id BINARY(16) NOT NULL,
    application_id BINARY(16) NOT NULL,
    grant_version BIGINT UNSIGNED NOT NULL,
    status ENUM('active','revoked') NOT NULL,
    admission_epoch BIGINT UNSIGNED NOT NULL,
    updated_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6),
    PRIMARY KEY (tenant_id, user_id, application_id),
    KEY idx_runtime_user_grant_application (tenant_id, application_id, status)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE runtime_trigger_operations (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    binding_id BINARY(16) NOT NULL,
    configuration_revision BIGINT UNSIGNED NOT NULL,
    operation VARCHAR(64) NOT NULL,
    idempotency_key VARCHAR(192) NOT NULL,
    request_hash CHAR(71) NOT NULL,
    status ENUM('pending','processing','completed','failed') NOT NULL DEFAULT 'pending',
    response_json JSON NULL,
    last_error VARCHAR(1000) NULL,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    updated_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    UNIQUE KEY uq_runtime_trigger_operation
        (tenant_id, binding_id, configuration_revision, operation),
    UNIQUE KEY uq_runtime_trigger_operation_idempotency
        (tenant_id, idempotency_key)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE application_chat_mappings (
    tenant_id BINARY(16) NOT NULL,
    application_id BINARY(16) NOT NULL,
    deployment_id BINARY(16) NOT NULL,
    bundle_id BINARY(16) NOT NULL,
    version BIGINT UNSIGNED NOT NULL,
    mapping_json JSON NULL,
    content_hash CHAR(71) NOT NULL,
    updated_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6),
    PRIMARY KEY (tenant_id, deployment_id),
    UNIQUE KEY uq_runtime_chat_mapping_bundle (tenant_id, bundle_id),
    KEY idx_runtime_chat_mapping_application (tenant_id, application_id, version)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;
