-- P4-05 channel credential internalization: config lives on the channel row.
SET NAMES utf8mb4;
SET time_zone = '+00:00';

ALTER TABLE application_webhooks
    ADD COLUMN channel_mode VARCHAR(16) NOT NULL DEFAULT 'callback' AFTER provider_type,
    ADD COLUMN channel_config_json JSON NULL AFTER channel_mode,
    DROP COLUMN credential_id;
