-- P4 provider webhook runtime projection and source metadata.
SET NAMES utf8mb4;
SET time_zone = '+00:00';

ALTER TABLE webhook_bindings
    ADD COLUMN provider_type VARCHAR(32) NOT NULL DEFAULT 'agentx' AFTER public_id,
    ADD COLUMN credential_id BINARY(16) NULL AFTER provider_type,
    ADD COLUMN input_mapping_json JSON NULL AFTER credential_id,
    ADD COLUMN fixed_inputs_json JSON NULL AFTER input_mapping_json;

ALTER TABLE application_invocations
    ADD COLUMN provider_event_id VARCHAR(255) NULL AFTER idempotency_key,
    ADD COLUMN conversation_id VARCHAR(255) NULL AFTER provider_event_id,
    ADD COLUMN trigger_context_json JSON NULL AFTER conversation_id;
