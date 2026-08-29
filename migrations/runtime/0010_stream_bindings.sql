-- P4-05/P4-06 stream channel bindings: mode, lease and connection status.
SET NAMES utf8mb4;
SET time_zone = '+00:00';

ALTER TABLE webhook_bindings
    ADD COLUMN channel_mode VARCHAR(16) NOT NULL DEFAULT 'callback' AFTER provider_type,
    DROP COLUMN credential_id,
    ADD COLUMN locked_by BINARY(16) NULL AFTER activated_at,
    ADD COLUMN locked_until TIMESTAMP(6) NULL AFTER locked_by,
    ADD COLUMN heartbeat_at TIMESTAMP(6) NULL AFTER locked_until,
    ADD COLUMN fencing_token BIGINT UNSIGNED NOT NULL DEFAULT 0 AFTER heartbeat_at,
    ADD COLUMN connection_status VARCHAR(16) NULL AFTER fencing_token,
    ADD COLUMN connection_error VARCHAR(1000) NULL AFTER connection_status,
    ADD COLUMN last_connected_at TIMESTAMP(6) NULL AFTER connection_error;

CREATE INDEX idx_webhook_binding_stream ON webhook_bindings (status, channel_mode, locked_until);
