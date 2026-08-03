ALTER TABLE workflow_versions
    ADD COLUMN compiled_ir_json JSON NULL AFTER content_hash,
    ADD COLUMN compiled_ir_hash VARCHAR(96) NULL AFTER compiled_ir_json,
    ADD COLUMN compiler_version VARCHAR(64) NULL AFTER compiled_ir_hash,
    ADD COLUMN compiled_at TIMESTAMP(6) NULL AFTER compiler_version,
    ADD KEY idx_workflow_version_compiler (tenant_id, compiler_version, compiled_at);

CREATE TABLE node_definitions (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NULL,
    node_type VARCHAR(128) NOT NULL,
    display_name VARCHAR(160) NOT NULL,
    source_type ENUM('platform', 'remote') NOT NULL,
    status ENUM('active', 'disabled') NOT NULL DEFAULT 'active',
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    updated_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    UNIQUE KEY uq_node_definition (tenant_id, node_type),
    KEY idx_node_definition_lookup (node_type, status),
    CONSTRAINT fk_node_definition_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE node_definition_versions (
    id BINARY(16) NOT NULL,
    node_definition_id BINARY(16) NOT NULL,
    version_number INT UNSIGNED NOT NULL,
    protocol_version VARCHAR(32) NOT NULL,
    manifest_json JSON NOT NULL,
    manifest_hash VARCHAR(96) NOT NULL,
    capability ENUM('builtin', 'declarative_http', 'remote_action') NOT NULL,
    execution_style ENUM('action', 'trigger', 'suspend', 'sub_workflow') NOT NULL,
    side_effect_level ENUM('none', 'idempotent', 'reversible', 'irreversible') NOT NULL DEFAULT 'none',
    resume_policy JSON NULL,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    UNIQUE KEY uq_node_definition_version (node_definition_id, version_number),
    CONSTRAINT fk_node_definition_version_definition FOREIGN KEY (node_definition_id) REFERENCES node_definitions(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

ALTER TABLE workflow_executions
    ADD COLUMN requested_by BINARY(16) NULL AFTER trigger_type,
    ADD COLUMN input_json JSON NULL AFTER requested_by,
    ADD COLUMN cancellation_requested_at TIMESTAMP(6) NULL AFTER error_message,
    ADD COLUMN state_version BIGINT UNSIGNED NOT NULL DEFAULT 1 AFTER cancellation_requested_at,
    ADD CONSTRAINT fk_execution_requested_by FOREIGN KEY (requested_by) REFERENCES users(id);

CREATE TABLE execution_snapshots (
    execution_id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    workflow_version_id BINARY(16) NOT NULL,
    definition_json JSON NOT NULL,
    compiled_ir_json JSON NOT NULL,
    compiled_ir_hash VARCHAR(96) NOT NULL,
    compiler_version VARCHAR(64) NOT NULL,
    resource_snapshot_json JSON NOT NULL,
    runtime_settings_json JSON NOT NULL,
    state_hash VARCHAR(96) NOT NULL,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    PRIMARY KEY (execution_id),
    CONSTRAINT fk_execution_snapshot_execution FOREIGN KEY (execution_id) REFERENCES workflow_executions(id),
    CONSTRAINT fk_execution_snapshot_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_execution_snapshot_version FOREIGN KEY (workflow_version_id) REFERENCES workflow_versions(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE node_executions (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    execution_id BINARY(16) NOT NULL,
    node_id VARCHAR(128) NOT NULL,
    node_name VARCHAR(160) NOT NULL,
    node_type VARCHAR(128) NOT NULL,
    node_version INT UNSIGNED NOT NULL,
    generation INT UNSIGNED NOT NULL,
    activation_slot INT UNSIGNED NOT NULL,
    run_index INT UNSIGNED NOT NULL,
    iteration_index INT UNSIGNED NOT NULL DEFAULT 0,
    status ENUM('ready', 'queued', 'running', 'waiting', 'succeeded', 'failed', 'skipped', 'cancelled', 'timed_out') NOT NULL,
    capability ENUM('builtin', 'declarative_http', 'remote_action') NOT NULL,
    side_effect_level ENUM('none', 'idempotent', 'reversible', 'irreversible') NOT NULL DEFAULT 'none',
    input_json JSON NULL,
    output_json JSON NULL,
    error_code VARCHAR(128) NULL,
    error_message VARCHAR(1000) NULL,
    started_at TIMESTAMP(6) NULL,
    ended_at TIMESTAMP(6) NULL,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    updated_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    UNIQUE KEY uq_node_execution_activation (execution_id, node_id, generation, activation_slot),
    KEY idx_node_execution_ready (tenant_id, status, capability, created_at),
    KEY idx_node_execution_list (tenant_id, execution_id, run_index, created_at),
    CONSTRAINT fk_node_execution_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_node_execution_execution FOREIGN KEY (execution_id) REFERENCES workflow_executions(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE node_attempts (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    execution_id BINARY(16) NOT NULL,
    node_execution_id BINARY(16) NOT NULL,
    attempt_number INT UNSIGNED NOT NULL,
    status ENUM('queued', 'running', 'suspended', 'succeeded', 'failed', 'cancelled', 'timed_out', 'lease_expired') NOT NULL,
    idempotency_key VARCHAR(192) NOT NULL,
    lease_token BINARY(16) NULL,
    worker_instance_id VARCHAR(160) NULL,
    deadline_at TIMESTAMP(6) NULL,
    input_json JSON NULL,
    output_json JSON NULL,
    log_artifact_id BINARY(16) NULL,
    error_code VARCHAR(128) NULL,
    error_message VARCHAR(1000) NULL,
    started_at TIMESTAMP(6) NULL,
    ended_at TIMESTAMP(6) NULL,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    UNIQUE KEY uq_node_attempt_number (node_execution_id, attempt_number),
    UNIQUE KEY uq_node_attempt_idempotency (tenant_id, idempotency_key),
    KEY idx_node_attempt_execution (tenant_id, execution_id, created_at),
    CONSTRAINT fk_node_attempt_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_node_attempt_execution FOREIGN KEY (execution_id) REFERENCES workflow_executions(id),
    CONSTRAINT fk_node_attempt_node_execution FOREIGN KEY (node_execution_id) REFERENCES node_executions(id),
    CONSTRAINT fk_node_attempt_log FOREIGN KEY (log_artifact_id) REFERENCES artifacts(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE execution_edge_deliveries (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    execution_id BINARY(16) NOT NULL,
    sequence_number BIGINT UNSIGNED NOT NULL,
    connection_id VARCHAR(128) NOT NULL,
    source_node_execution_id BINARY(16) NOT NULL,
    source_port VARCHAR(128) NOT NULL,
    target_node_id VARCHAR(128) NOT NULL,
    target_port VARCHAR(128) NOT NULL,
    target_generation INT UNSIGNED NOT NULL,
    delivery_kind ENUM('data', 'closed_without_data') NOT NULL,
    item_count INT UNSIGNED NOT NULL DEFAULT 0,
    payload_json JSON NULL,
    payload_artifact_id BINARY(16) NULL,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    UNIQUE KEY uq_edge_delivery_sequence (execution_id, sequence_number),
    KEY idx_edge_delivery_frontier (tenant_id, execution_id, target_node_id, target_generation),
    CONSTRAINT fk_edge_delivery_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_edge_delivery_execution FOREIGN KEY (execution_id) REFERENCES workflow_executions(id),
    CONSTRAINT fk_edge_delivery_source FOREIGN KEY (source_node_execution_id) REFERENCES node_executions(id),
    CONSTRAINT fk_edge_delivery_artifact FOREIGN KEY (payload_artifact_id) REFERENCES artifacts(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE item_lineage (
    tenant_id BINARY(16) NOT NULL,
    execution_id BINARY(16) NOT NULL,
    delivery_id BINARY(16) NOT NULL,
    target_item_index INT UNSIGNED NOT NULL,
    source_node_execution_id BINARY(16) NOT NULL,
    source_run_index INT UNSIGNED NOT NULL,
    source_output_index INT UNSIGNED NOT NULL,
    source_item_index INT UNSIGNED NOT NULL,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    PRIMARY KEY (delivery_id, target_item_index, source_node_execution_id, source_output_index, source_item_index),
    KEY idx_item_lineage_execution (tenant_id, execution_id, source_node_execution_id),
    CONSTRAINT fk_item_lineage_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_item_lineage_execution FOREIGN KEY (execution_id) REFERENCES workflow_executions(id),
    CONSTRAINT fk_item_lineage_delivery FOREIGN KEY (delivery_id) REFERENCES execution_edge_deliveries(id),
    CONSTRAINT fk_item_lineage_source FOREIGN KEY (source_node_execution_id) REFERENCES node_executions(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE execution_outbox (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    execution_id BINARY(16) NOT NULL,
    node_execution_id BINARY(16) NULL,
    attempt_id BINARY(16) NULL,
    message_type ENUM('dispatch_node', 'runtime_event', 'resume', 'cancel') NOT NULL,
    capability ENUM('builtin', 'declarative_http', 'remote_action') NULL,
    payload_json JSON NOT NULL,
    status ENUM('pending', 'published', 'failed') NOT NULL DEFAULT 'pending',
    available_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    locked_by BINARY(16) NULL,
    locked_until TIMESTAMP(6) NULL,
    attempt_count INT UNSIGNED NOT NULL DEFAULT 0,
    published_at TIMESTAMP(6) NULL,
    last_error VARCHAR(1000) NULL,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    KEY idx_execution_outbox_pending (status, available_at, locked_until, created_at),
    CONSTRAINT fk_execution_outbox_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_execution_outbox_execution FOREIGN KEY (execution_id) REFERENCES workflow_executions(id),
    CONSTRAINT fk_execution_outbox_node FOREIGN KEY (node_execution_id) REFERENCES node_executions(id),
    CONSTRAINT fk_execution_outbox_attempt FOREIGN KEY (attempt_id) REFERENCES node_attempts(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE worker_leases (
    node_attempt_id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    node_execution_id BINARY(16) NOT NULL,
    lease_token BINARY(16) NOT NULL,
    worker_instance_id VARCHAR(160) NOT NULL,
    capability ENUM('builtin', 'declarative_http', 'remote_action') NOT NULL,
    acquired_at TIMESTAMP(6) NOT NULL,
    heartbeat_at TIMESTAMP(6) NOT NULL,
    expires_at TIMESTAMP(6) NOT NULL,
    released_at TIMESTAMP(6) NULL,
    PRIMARY KEY (node_attempt_id),
    UNIQUE KEY uq_worker_lease_token (lease_token),
    KEY idx_worker_lease_reaper (expires_at, released_at),
    CONSTRAINT fk_worker_lease_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_worker_lease_attempt FOREIGN KEY (node_attempt_id) REFERENCES node_attempts(id),
    CONSTRAINT fk_worker_lease_node FOREIGN KEY (node_execution_id) REFERENCES node_executions(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE runtime_idempotency_keys (
    tenant_id BINARY(16) NOT NULL,
    scope VARCHAR(96) NOT NULL,
    idempotency_key VARCHAR(192) NOT NULL,
    request_hash VARCHAR(96) NOT NULL,
    status ENUM('processing', 'completed', 'failed') NOT NULL,
    response_json JSON NULL,
    expires_at TIMESTAMP(6) NOT NULL,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    updated_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6),
    PRIMARY KEY (tenant_id, scope, idempotency_key),
    KEY idx_runtime_idempotency_expiry (expires_at),
    CONSTRAINT fk_runtime_idempotency_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE checkpoints (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    execution_id BINARY(16) NOT NULL,
    node_execution_id BINARY(16) NULL,
    sequence_number BIGINT UNSIGNED NOT NULL,
    checkpoint_type ENUM('execution_start', 'node_completed', 'node_suspended', 'manual') NOT NULL,
    state_hash VARCHAR(96) NOT NULL,
    payload_json JSON NULL,
    payload_artifact_id BINARY(16) NULL,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    UNIQUE KEY uq_checkpoint_sequence (execution_id, sequence_number),
    KEY idx_checkpoint_timeline (tenant_id, execution_id, created_at),
    CONSTRAINT fk_checkpoint_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_checkpoint_execution FOREIGN KEY (execution_id) REFERENCES workflow_executions(id),
    CONSTRAINT fk_checkpoint_node FOREIGN KEY (node_execution_id) REFERENCES node_executions(id),
    CONSTRAINT fk_checkpoint_artifact FOREIGN KEY (payload_artifact_id) REFERENCES artifacts(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;
