-- V2-03 Runtime Gateway control-plane publication metadata.
-- Runtime owns production state; these columns only track the next immutable revision to publish.
SET NAMES utf8mb4;
SET time_zone = '+00:00';

ALTER TABLE applications
    ADD COLUMN runtime_config_revision BIGINT UNSIGNED NOT NULL DEFAULT 1 AFTER status,
    ADD COLUMN published_runtime_config_revision BIGINT UNSIGNED NOT NULL DEFAULT 0 AFTER runtime_config_revision,
    ADD COLUMN runtime_activation_sequence BIGINT UNSIGNED NOT NULL DEFAULT 0 AFTER published_runtime_config_revision,
    ADD CONSTRAINT chk_application_runtime_revisions
        CHECK (published_runtime_config_revision <= runtime_config_revision);

ALTER TABLE application_deployments
    ADD COLUMN trigger_revision BIGINT UNSIGNED NOT NULL DEFAULT 1 AFTER session_version_policy,
    ADD COLUMN trigger_manifest_hash CHAR(71) NULL AFTER trigger_revision,
    ADD KEY idx_application_deployment_trigger_revision
        (tenant_id, application_id, trigger_revision);

ALTER TABLE application_webhooks
    DROP COLUMN secret_algorithm,
    DROP COLUMN secret_key_id,
    DROP COLUMN secret_nonce,
    DROP COLUMN secret_ciphertext,
    ADD COLUMN secret_ref_json JSON NOT NULL AFTER public_id,
    ADD COLUMN configuration_revision BIGINT UNSIGNED NOT NULL DEFAULT 1 AFTER status,
    ADD COLUMN configuration_hash CHAR(71) NULL AFTER configuration_revision;

ALTER TABLE application_schedules
    ADD COLUMN misfire_policy ENUM('skip','fire_once') NOT NULL DEFAULT 'skip' AFTER timezone,
    ADD COLUMN configuration_revision BIGINT UNSIGNED NOT NULL DEFAULT 1 AFTER status,
    ADD COLUMN configuration_hash CHAR(71) NULL AFTER configuration_revision;

CREATE TABLE application_runtime_trigger_revisions (
    tenant_id BINARY(16) NOT NULL,
    application_id BINARY(16) NOT NULL,
    revision BIGINT UNSIGNED NOT NULL,
    manifest_hash CHAR(71) NOT NULL,
    manifest_json JSON NOT NULL,
    created_by BINARY(16) NOT NULL,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    PRIMARY KEY (tenant_id, application_id, revision),
    UNIQUE KEY uq_control_trigger_manifest_hash (tenant_id, application_id, manifest_hash)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;
