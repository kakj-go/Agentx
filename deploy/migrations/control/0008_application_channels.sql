-- P4 provider webhook channel configuration.
SET NAMES utf8mb4;
SET time_zone = '+00:00';

ALTER TABLE application_webhooks
    ADD COLUMN provider_type VARCHAR(32) NOT NULL DEFAULT 'agentx' AFTER public_id,
    ADD COLUMN credential_id BINARY(16) NULL AFTER provider_type,
    ADD COLUMN input_mapping_json JSON NULL AFTER credential_id,
    ADD COLUMN fixed_inputs_json JSON NULL AFTER input_mapping_json;
