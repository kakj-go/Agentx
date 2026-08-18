-- V2-02 Control-plane Bundle publishing state.
-- Runtime objects are copied through the Runtime Internal API; this schema
-- never stores Runtime OSS credentials or creates cross-plane foreign keys.
SET NAMES utf8mb4;
SET time_zone = '+00:00';

ALTER TABLE application_deployments
    MODIFY status ENUM(
        'building', 'prepared', 'activating', 'active', 'rejected', 'superseded'
    ) NOT NULL DEFAULT 'building';

CREATE TABLE execution_spec_bundles (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    application_id BINARY(16) NOT NULL,
    deployment_id BINARY(16) NOT NULL,
    workflow_id BINARY(16) NOT NULL,
    workflow_version_id BINARY(16) NOT NULL,
    sequence_number BIGINT UNSIGNED NOT NULL,
    schema_version SMALLINT UNSIGNED NOT NULL,
    compiler_version VARCHAR(64) NOT NULL,
    content_hash CHAR(71) NOT NULL,
    signature_key_id VARCHAR(128) NOT NULL,
    signature VARBINARY(128) NOT NULL,
    payload_json JSON NOT NULL,
    object_manifest_json JSON NOT NULL,
    status ENUM('building', 'built', 'published', 'rejected') NOT NULL DEFAULT 'building',
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    built_at TIMESTAMP(6) NULL,
    published_at TIMESTAMP(6) NULL,
    PRIMARY KEY (id),
    UNIQUE KEY uq_control_bundle_deployment (tenant_id, deployment_id),
    UNIQUE KEY uq_control_bundle_sequence (tenant_id, application_id, sequence_number),
    UNIQUE KEY uq_control_bundle_hash (tenant_id, application_id, content_hash),
    KEY idx_control_bundle_version (tenant_id, workflow_version_id),
    CONSTRAINT chk_control_bundle_schema_v1 CHECK (schema_version = 1)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE bundle_object_copies (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    bundle_id BINARY(16) NOT NULL,
    object_id BINARY(16) NOT NULL,
    source_domain ENUM('control') NOT NULL DEFAULT 'control',
    source_key VARCHAR(1024) NOT NULL,
    content_hash CHAR(71) NOT NULL,
    size_bytes BIGINT UNSIGNED NOT NULL,
    media_type VARCHAR(255) NOT NULL,
    idempotency_key VARCHAR(192) NOT NULL,
    status ENUM('pending', 'copying', 'copied', 'failed') NOT NULL DEFAULT 'pending',
    runtime_object_key VARCHAR(1024) NULL,
    receipt_json JSON NULL,
    attempt_count INT UNSIGNED NOT NULL DEFAULT 0,
    last_error_code VARCHAR(128) NULL,
    last_error_message VARCHAR(1000) NULL,
    copied_at TIMESTAMP(6) NULL,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    updated_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    UNIQUE KEY uq_bundle_object_copy (tenant_id, bundle_id, object_id, content_hash),
    UNIQUE KEY uq_bundle_object_idempotency (tenant_id, idempotency_key),
    KEY idx_bundle_object_copy_pending (status, updated_at)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE publish_attempts (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    application_id BINARY(16) NOT NULL,
    deployment_id BINARY(16) NOT NULL,
    bundle_id BINARY(16) NULL,
    previous_attempt_id BINARY(16) NULL,
    requested_action ENUM('publish', 'retry', 'rollback', 'disable') NOT NULL DEFAULT 'publish',
    state ENUM(
        'building', 'copying', 'preparing', 'prepared', 'activating', 'active', 'rejected'
    ) NOT NULL DEFAULT 'building',
    next_action ENUM(
        'build', 'copy_objects', 'apply_admission', 'prepare', 'activate', 'update_head', 'none'
    ) NOT NULL DEFAULT 'build',
    idempotency_key VARCHAR(192) NOT NULL,
    expected_head_version BIGINT UNSIGNED NULL,
    activation_sequence BIGINT UNSIGNED NOT NULL,
    minimum_admission_epoch BIGINT UNSIGNED NOT NULL,
    locked_by BINARY(16) NULL,
    locked_until TIMESTAMP(6) NULL,
    fencing_token BIGINT UNSIGNED NOT NULL DEFAULT 0,
    attempt_count INT UNSIGNED NOT NULL DEFAULT 0,
    available_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    last_error_code VARCHAR(128) NULL,
    last_error_message VARCHAR(1000) NULL,
    last_error_json JSON NULL,
    created_by BINARY(16) NOT NULL,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    updated_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6),
    completed_at TIMESTAMP(6) NULL,
    PRIMARY KEY (id),
    UNIQUE KEY uq_publish_attempt_idempotency (tenant_id, idempotency_key),
    UNIQUE KEY uq_publish_attempt_sequence (tenant_id, application_id, activation_sequence, requested_action),
    KEY idx_publish_attempt_claim (state, available_at, locked_until, created_at),
    KEY idx_publish_attempt_deployment (tenant_id, application_id, deployment_id, created_at),
    CONSTRAINT chk_publish_attempt_fencing CHECK (fencing_token >= attempt_count)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE admission_epochs (
    tenant_id BINARY(16) NOT NULL,
    application_id BINARY(16) NOT NULL,
    current_epoch BIGINT UNSIGNED NOT NULL DEFAULT 0,
    updated_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6),
    PRIMARY KEY (tenant_id, application_id),
    CONSTRAINT chk_admission_epoch_monotonic CHECK (current_epoch >= 0)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

ALTER TABLE outbox
    ADD COLUMN status ENUM('pending', 'processing', 'published', 'failed') NOT NULL DEFAULT 'pending' AFTER payload_json,
    ADD COLUMN locked_by BINARY(16) NULL AFTER available_at,
    ADD COLUMN locked_until TIMESTAMP(6) NULL AFTER locked_by,
    ADD COLUMN fencing_token BIGINT UNSIGNED NOT NULL DEFAULT 0 AFTER locked_until,
    ADD COLUMN request_hash CHAR(71) NULL AFTER fencing_token,
    ADD COLUMN idempotency_key VARCHAR(192) NULL AFTER request_hash,
    ADD UNIQUE KEY uq_control_outbox_idempotency (tenant_id, idempotency_key),
    ADD KEY idx_control_outbox_claim (status, available_at, locked_until, occurred_at);
