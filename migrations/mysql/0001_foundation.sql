CREATE TABLE artifacts (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    content_type VARCHAR(255) NOT NULL,
    size_bytes BIGINT UNSIGNED NOT NULL,
    sha256 CHAR(64) NOT NULL,
    storage_key VARCHAR(512) NOT NULL,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    deleted_at TIMESTAMP(6) NULL,
    PRIMARY KEY (id),
    UNIQUE KEY uq_artifacts_storage_key (storage_key),
    KEY idx_artifacts_tenant_created (tenant_id, created_at),
    KEY idx_artifacts_tenant_hash (tenant_id, sha256)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE outbox_events (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    event_type VARCHAR(128) NOT NULL,
    aggregate_type VARCHAR(128) NOT NULL,
    aggregate_id VARCHAR(128) NOT NULL,
    payload_json JSON NOT NULL,
    occurred_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    available_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    published_at TIMESTAMP(6) NULL,
    attempt_count INT UNSIGNED NOT NULL DEFAULT 0,
    last_error VARCHAR(1024) NULL,
    PRIMARY KEY (id),
    KEY idx_outbox_pending (published_at, available_at, occurred_at),
    KEY idx_outbox_tenant_aggregate (tenant_id, aggregate_type, aggregate_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;
