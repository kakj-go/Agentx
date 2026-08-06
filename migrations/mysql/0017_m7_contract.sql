ALTER TABLE credential_secret_versions
    ADD CONSTRAINT chk_credential_secret_storage CHECK (
        (provider='local_encrypted' AND algorithm IS NOT NULL AND key_id IS NOT NULL AND nonce IS NOT NULL AND ciphertext IS NOT NULL)
        OR
        (provider<>'local_encrypted' AND secret_ref IS NOT NULL AND provider_version IS NOT NULL)
    );

ALTER TABLE application_webhooks
    ADD CONSTRAINT chk_webhook_secret_storage CHECK (
        (secret_provider='local_encrypted' AND secret_algorithm IS NOT NULL AND secret_key_id IS NOT NULL AND secret_nonce IS NOT NULL AND secret_ciphertext IS NOT NULL)
        OR
        (secret_provider<>'local_encrypted' AND secret_ref IS NOT NULL AND secret_provider_version IS NOT NULL)
    );

CREATE TABLE release_schema_contract (
    contract_name VARCHAR(64) NOT NULL,
    schema_version VARCHAR(32) NOT NULL,
    minimum_application_version VARCHAR(32) NOT NULL,
    applied_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    PRIMARY KEY (contract_name)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

INSERT INTO release_schema_contract(contract_name,schema_version,minimum_application_version)
VALUES('m7-runtime-integration','17','0.1.0');
