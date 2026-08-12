ALTER TABLE model_deployments
    DROP COLUMN name,
    ADD COLUMN max_input_tokens BIGINT UNSIGNED NOT NULL DEFAULT 1050000 AFTER model_name,
    ADD COLUMN max_output_tokens BIGINT UNSIGNED NOT NULL DEFAULT 128000 AFTER max_input_tokens;
