ALTER TABLE workflow_executions
    ADD COLUMN result_json JSON NULL AFTER input_json,
    ADD COLUMN result_artifact_id BINARY(16) NULL AFTER result_json,
    ADD COLUMN result_hash VARCHAR(96) NULL AFTER result_artifact_id,
    ADD COLUMN terminal_event_emitted BOOLEAN NOT NULL DEFAULT FALSE AFTER result_hash,
    ADD CONSTRAINT fk_execution_result_artifact FOREIGN KEY (result_artifact_id) REFERENCES artifacts(id);

ALTER TABLE application_invocations
    MODIFY COLUMN caller_type ENUM('user','api_key','webhook','schedule','poll') NOT NULL;

ALTER TABLE execution_events
    ADD COLUMN event_id BINARY(16) NULL AFTER execution_id,
    ADD COLUMN schema_version VARCHAR(16) NOT NULL DEFAULT '1.0' AFTER event_type,
    ADD UNIQUE KEY uq_execution_event_id (event_id);

UPDATE execution_events SET event_id=UUID_TO_BIN(UUID()) WHERE event_id IS NULL;

ALTER TABLE execution_events
    MODIFY COLUMN event_id BINARY(16) NOT NULL;

ALTER TABLE outbox_events
    ADD COLUMN schema_version VARCHAR(16) NOT NULL DEFAULT '1.0' AFTER event_type,
    ADD COLUMN execution_id BINARY(16) NULL AFTER aggregate_id,
    ADD COLUMN sequence_number BIGINT UNSIGNED NULL AFTER execution_id,
    ADD KEY idx_outbox_execution (tenant_id, execution_id, sequence_number),
    ADD CONSTRAINT fk_outbox_execution FOREIGN KEY (execution_id) REFERENCES workflow_executions(id);

CREATE TABLE runtime_commands (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    command_type VARCHAR(64) NOT NULL,
    aggregate_type VARCHAR(64) NOT NULL,
    aggregate_id VARCHAR(128) NOT NULL,
    idempotency_key VARCHAR(192) NOT NULL,
    payload_json JSON NOT NULL,
    status ENUM('pending','processing','completed','failed') NOT NULL DEFAULT 'pending',
    available_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    locked_by BINARY(16) NULL,
    locked_until TIMESTAMP(6) NULL,
    attempt_count INT UNSIGNED NOT NULL DEFAULT 0,
    result_json JSON NULL,
    error_code VARCHAR(128) NULL,
    error_message VARCHAR(1000) NULL,
    completed_at TIMESTAMP(6) NULL,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    updated_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    UNIQUE KEY uq_runtime_command_idempotency (tenant_id, command_type, idempotency_key),
    KEY idx_runtime_commands_pending (status, available_at, locked_until, created_at),
    KEY idx_runtime_commands_aggregate (tenant_id, aggregate_type, aggregate_id, created_at),
    CONSTRAINT fk_runtime_command_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE projection_receipts (
    projector_name VARCHAR(128) NOT NULL,
    event_id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    processed_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    PRIMARY KEY (projector_name, event_id),
    KEY idx_projection_receipts_tenant (tenant_id, processed_at),
    CONSTRAINT fk_projection_receipt_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE worker_capabilities (
    instance_id VARCHAR(160) NOT NULL,
    capability VARCHAR(64) NOT NULL,
    node_protocol_version VARCHAR(32) NOT NULL,
    ir_schema_versions_json JSON NOT NULL,
    compiler_version_min VARCHAR(64) NOT NULL,
    compiler_version_max VARCHAR(64) NOT NULL,
    manifest_hashes_json JSON NOT NULL,
    status ENUM('ready','draining','unavailable') NOT NULL DEFAULT 'ready',
    heartbeat_at TIMESTAMP(6) NOT NULL,
    PRIMARY KEY (instance_id, capability),
    KEY idx_worker_capability_ready (capability, status, heartbeat_at)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

ALTER TABLE application_deployments
    ADD COLUMN output_expression VARCHAR(4000) NULL AFTER output_schema_json;

ALTER TABLE application_invocations
    ADD COLUMN application_deployment_id BINARY(16) NULL AFTER application_id,
    ADD COLUMN runtime_command_id BINARY(16) NULL AFTER execution_id,
    ADD COLUMN last_event_sequence BIGINT UNSIGNED NOT NULL DEFAULT 0 AFTER status,
    ADD UNIQUE KEY uq_invocation_runtime_command (runtime_command_id),
    ADD KEY idx_invocation_deployment (tenant_id, application_deployment_id),
    ADD CONSTRAINT fk_invocation_application_deployment FOREIGN KEY (application_deployment_id) REFERENCES application_deployments(id),
    ADD CONSTRAINT fk_invocation_runtime_command FOREIGN KEY (runtime_command_id) REFERENCES runtime_commands(id);

ALTER TABLE application_schedules
    ADD COLUMN misfire_policy ENUM('skip','fire_once') NOT NULL DEFAULT 'fire_once' AFTER input_json,
    ADD COLUMN next_fire_at TIMESTAMP(6) NULL AFTER misfire_policy,
    ADD COLUMN last_fire_at TIMESTAMP(6) NULL AFTER next_fire_at,
    ADD COLUMN locked_by BINARY(16) NULL AFTER last_fire_at,
    ADD COLUMN locked_until TIMESTAMP(6) NULL AFTER locked_by,
    ADD KEY idx_application_schedule_scan (status, next_fire_at, locked_until);

ALTER TABLE approval_tasks
    ADD COLUMN decision_command_id BINARY(16) NULL AFTER resume_status,
    ADD UNIQUE KEY uq_approval_decision_command (decision_command_id),
    ADD CONSTRAINT fk_approval_decision_command FOREIGN KEY (decision_command_id) REFERENCES runtime_commands(id);

CREATE TABLE evaluation_run_cases (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    evaluation_run_id BINARY(16) NOT NULL,
    source_case_id BINARY(16) NOT NULL,
    target_command_id BINARY(16) NOT NULL,
    target_execution_id BINARY(16) NULL,
    status ENUM('queued','running','scoring','completed','failed','cancelled') NOT NULL DEFAULT 'queued',
    actual_output_json JSON NULL,
    duration_ms BIGINT UNSIGNED NULL,
    cost_micros BIGINT UNSIGNED NOT NULL DEFAULT 0,
    error_code VARCHAR(128) NULL,
    error_message VARCHAR(1000) NULL,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    completed_at TIMESTAMP(6) NULL,
    PRIMARY KEY (id),
    UNIQUE KEY uq_evaluation_run_case (tenant_id, evaluation_run_id, source_case_id),
    UNIQUE KEY uq_evaluation_case_command (target_command_id),
    KEY idx_evaluation_case_status (tenant_id, evaluation_run_id, status),
    CONSTRAINT fk_evaluation_case_run_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_evaluation_case_run_run FOREIGN KEY (evaluation_run_id) REFERENCES evaluation_runs(id),
    CONSTRAINT fk_evaluation_case_run_command FOREIGN KEY (target_command_id) REFERENCES runtime_commands(id),
    CONSTRAINT fk_evaluation_case_run_execution FOREIGN KEY (target_execution_id) REFERENCES workflow_executions(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE evaluation_rule_results (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    evaluation_run_case_id BINARY(16) NOT NULL,
    profile_rule_id BINARY(16) NOT NULL,
    evaluator_command_id BINARY(16) NULL,
    evaluator_execution_id BINARY(16) NULL,
    status ENUM('queued','running','passed','failed','error','cancelled') NOT NULL,
    passed BOOLEAN NULL,
    score DECIMAL(12,6) NULL,
    detail_json JSON NOT NULL,
    duration_ms BIGINT UNSIGNED NULL,
    cost_micros BIGINT UNSIGNED NOT NULL DEFAULT 0,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    completed_at TIMESTAMP(6) NULL,
    PRIMARY KEY (id),
    UNIQUE KEY uq_evaluation_case_rule (tenant_id, evaluation_run_case_id, profile_rule_id),
    KEY idx_evaluation_rule_status (tenant_id, status, created_at),
    CONSTRAINT fk_evaluation_rule_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_evaluation_rule_case FOREIGN KEY (evaluation_run_case_id) REFERENCES evaluation_run_cases(id),
    CONSTRAINT fk_evaluation_rule_profile FOREIGN KEY (profile_rule_id) REFERENCES evaluation_profile_rules(id),
    CONSTRAINT fk_evaluation_rule_command FOREIGN KEY (evaluator_command_id) REFERENCES runtime_commands(id),
    CONSTRAINT fk_evaluation_rule_execution FOREIGN KEY (evaluator_execution_id) REFERENCES workflow_executions(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE trigger_bindings (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    application_id BINARY(16) NOT NULL,
    application_deployment_id BINARY(16) NOT NULL,
    workflow_version_id BINARY(16) NOT NULL,
    node_id VARCHAR(128) NOT NULL,
    trigger_kind ENUM('webhook','schedule','poll','lifecycle') NOT NULL,
    configuration_json JSON NOT NULL,
    status ENUM('activating','active','deactivating','disabled','failed') NOT NULL,
    next_poll_at TIMESTAMP(6) NULL,
    last_poll_at TIMESTAMP(6) NULL,
    locked_by BINARY(16) NULL,
    locked_until TIMESTAMP(6) NULL,
    last_error VARCHAR(1000) NULL,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    updated_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    UNIQUE KEY uq_trigger_binding_node (tenant_id, application_deployment_id, node_id, trigger_kind),
    KEY idx_trigger_binding_scan (status, trigger_kind, next_poll_at, locked_until),
    CONSTRAINT fk_trigger_binding_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_trigger_binding_application FOREIGN KEY (application_id) REFERENCES applications(id),
    CONSTRAINT fk_trigger_binding_deployment FOREIGN KEY (application_deployment_id) REFERENCES application_deployments(id),
    CONSTRAINT fk_trigger_binding_version FOREIGN KEY (workflow_version_id) REFERENCES workflow_versions(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE quota_policies (
    tenant_id BINARY(16) NOT NULL,
    dimension_key VARCHAR(64) NOT NULL,
    hard_limit DECIMAL(24,6) NOT NULL,
    period_seconds BIGINT UNSIGNED NULL,
    version BIGINT UNSIGNED NOT NULL DEFAULT 1,
    updated_by BINARY(16) NOT NULL,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    updated_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6),
    PRIMARY KEY (tenant_id, dimension_key),
    CONSTRAINT fk_quota_policy_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_quota_policy_user FOREIGN KEY (updated_by) REFERENCES users(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE quota_reservations (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    dimension_key VARCHAR(64) NOT NULL,
    scope_type VARCHAR(64) NOT NULL,
    scope_id VARCHAR(128) NOT NULL,
    amount DECIMAL(24,6) NOT NULL,
    status ENUM('active','settled','released','expired') NOT NULL DEFAULT 'active',
    expires_at TIMESTAMP(6) NOT NULL,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    settled_at TIMESTAMP(6) NULL,
    PRIMARY KEY (id),
    UNIQUE KEY uq_quota_reservation_scope (tenant_id, dimension_key, scope_type, scope_id),
    KEY idx_quota_reservation_reaper (status, expires_at),
    CONSTRAINT fk_quota_reservation_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE quota_usage_ledger (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    dimension_key VARCHAR(64) NOT NULL,
    scope_type VARCHAR(64) NOT NULL,
    scope_id VARCHAR(128) NOT NULL,
    amount DECIMAL(24,6) NOT NULL,
    idempotency_key VARCHAR(192) NOT NULL,
    occurred_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    UNIQUE KEY uq_quota_usage_idempotency (tenant_id, idempotency_key),
    KEY idx_quota_usage_window (tenant_id, dimension_key, occurred_at),
    CONSTRAINT fk_quota_usage_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE artifact_references (
    tenant_id BINARY(16) NOT NULL,
    artifact_id BINARY(16) NOT NULL,
    owner_type VARCHAR(64) NOT NULL,
    owner_id VARCHAR(128) NOT NULL,
    reference_role VARCHAR(64) NOT NULL,
    retention_until TIMESTAMP(6) NULL,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    PRIMARY KEY (tenant_id, artifact_id, owner_type, owner_id, reference_role),
    KEY idx_artifact_reference_owner (tenant_id, owner_type, owner_id),
    KEY idx_artifact_reference_retention (tenant_id, artifact_id, retention_until),
    CONSTRAINT fk_artifact_reference_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_artifact_reference_artifact FOREIGN KEY (artifact_id) REFERENCES artifacts(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE retention_policies (
    tenant_id BINARY(16) NOT NULL,
    data_type VARCHAR(64) NOT NULL,
    retention_days INT UNSIGNED NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    version BIGINT UNSIGNED NOT NULL DEFAULT 1,
    updated_by BINARY(16) NOT NULL,
    updated_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6),
    PRIMARY KEY (tenant_id, data_type),
    CONSTRAINT fk_retention_policy_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_retention_policy_user FOREIGN KEY (updated_by) REFERENCES users(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE retention_runs (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    dry_run BOOLEAN NOT NULL,
    status ENUM('queued','running','completed','failed','cancelled') NOT NULL DEFAULT 'queued',
    requested_by BINARY(16) NOT NULL,
    candidate_count BIGINT UNSIGNED NOT NULL DEFAULT 0,
    deleted_count BIGINT UNSIGNED NOT NULL DEFAULT 0,
    error_message VARCHAR(1000) NULL,
    attempt_count INT UNSIGNED NOT NULL DEFAULT 0,
    available_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    locked_by BINARY(16) NULL,
    locked_until TIMESTAMP(6) NULL,
    started_at TIMESTAMP(6) NULL,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    completed_at TIMESTAMP(6) NULL,
    PRIMARY KEY (id),
    KEY idx_retention_run_status (status, available_at, locked_until, created_at),
    CONSTRAINT fk_retention_run_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_retention_run_user FOREIGN KEY (requested_by) REFERENCES users(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE retention_items (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    retention_run_id BINARY(16) NOT NULL,
    data_type VARCHAR(64) NOT NULL,
    target_id VARCHAR(128) NOT NULL,
    status ENUM('candidate','blocked','deleted','failed') NOT NULL,
    reason VARCHAR(255) NULL,
    attempt_count INT UNSIGNED NOT NULL DEFAULT 0,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    updated_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    UNIQUE KEY uq_retention_item_target (retention_run_id, data_type, target_id),
    KEY idx_retention_item_status (tenant_id, retention_run_id, status),
    CONSTRAINT fk_retention_item_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_retention_item_run FOREIGN KEY (retention_run_id) REFERENCES retention_runs(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

ALTER TABLE credential_secret_versions
    ADD COLUMN provider VARCHAR(32) NOT NULL DEFAULT 'local_encrypted' AFTER version_number,
    ADD COLUMN secret_ref VARCHAR(512) NULL AFTER provider,
    ADD COLUMN provider_version VARCHAR(128) NULL AFTER secret_ref,
    MODIFY COLUMN algorithm VARCHAR(32) NULL,
    MODIFY COLUMN key_id VARCHAR(64) NULL,
    MODIFY COLUMN nonce VARBINARY(32) NULL,
    MODIFY COLUMN ciphertext MEDIUMBLOB NULL;

ALTER TABLE application_webhooks
    ADD COLUMN secret_provider VARCHAR(32) NOT NULL DEFAULT 'local_encrypted' AFTER public_id,
    ADD COLUMN secret_ref VARCHAR(512) NULL AFTER secret_provider,
    ADD COLUMN secret_provider_version VARCHAR(128) NULL AFTER secret_ref,
    MODIFY COLUMN secret_algorithm VARCHAR(32) NULL,
    MODIFY COLUMN secret_key_id VARCHAR(64) NULL,
    MODIFY COLUMN secret_nonce VARBINARY(32) NULL,
    MODIFY COLUMN secret_ciphertext BLOB NULL;
