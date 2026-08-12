ALTER TABLE workflow_executions
    ADD COLUMN application_deployment_id BINARY(16) NULL AFTER session_id,
    ADD COLUMN caller_node_execution_id BINARY(16) NULL AFTER caller_execution_id,
    ADD COLUMN context_json JSON NULL AFTER input_json,
    ADD COLUMN context_base_json JSON NULL AFTER context_json,
    ADD COLUMN context_version BIGINT UNSIGNED NOT NULL DEFAULT 0 AFTER context_base_json,
    ADD COLUMN session_context_version BIGINT UNSIGNED NOT NULL DEFAULT 0 AFTER context_version;

UPDATE workflow_executions SET context_json=JSON_OBJECT(),context_base_json=JSON_OBJECT() WHERE context_json IS NULL OR context_base_json IS NULL;

ALTER TABLE workflow_executions
    MODIFY COLUMN context_json JSON NOT NULL,
    MODIFY COLUMN context_base_json JSON NOT NULL,
    DROP INDEX uq_workflow_execution_trace,
    ADD KEY idx_workflow_execution_trace (tenant_id, trace_id),
    ADD KEY idx_workflow_execution_session_context (tenant_id, application_deployment_id, session_id),
    ADD KEY idx_workflow_execution_parent_node (tenant_id, caller_execution_id, caller_node_execution_id),
    ADD CONSTRAINT fk_workflow_execution_application_deployment FOREIGN KEY (application_deployment_id) REFERENCES application_deployments(id);

ALTER TABLE node_executions
    ADD COLUMN node_key VARCHAR(128) NULL AFTER node_id;

UPDATE node_executions SET node_key=node_id WHERE node_key IS NULL;

ALTER TABLE node_executions
    MODIFY COLUMN node_key VARCHAR(128) NOT NULL,
    ADD KEY idx_node_execution_key (tenant_id, execution_id, node_key, run_index);

CREATE TABLE application_session_contexts (
    tenant_id BINARY(16) NOT NULL,
    application_deployment_id BINARY(16) NOT NULL,
    session_id BINARY(16) NOT NULL,
    context_json JSON NOT NULL,
    context_version BIGINT UNSIGNED NOT NULL DEFAULT 0,
    updated_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6),
    PRIMARY KEY (tenant_id, application_deployment_id, session_id),
    CONSTRAINT fk_session_context_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_session_context_deployment FOREIGN KEY (application_deployment_id) REFERENCES application_deployments(id),
    CONSTRAINT fk_session_context_session FOREIGN KEY (session_id) REFERENCES application_sessions(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE workflow_context_patches (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    execution_id BINARY(16) NOT NULL,
    node_execution_id BINARY(16) NOT NULL,
    attempt_id BINARY(16) NOT NULL,
    patch_index INT UNSIGNED NOT NULL,
    operation_key VARCHAR(32) NOT NULL,
    context_path VARCHAR(512) NOT NULL,
    value_json JSON NOT NULL,
    context_version_before BIGINT UNSIGNED NOT NULL,
    context_version_after BIGINT UNSIGNED NOT NULL,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    UNIQUE KEY uq_workflow_context_patch (tenant_id, node_execution_id, patch_index),
    KEY idx_workflow_context_patch_execution (tenant_id, execution_id, created_at),
    CONSTRAINT fk_workflow_context_patch_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_workflow_context_patch_execution FOREIGN KEY (execution_id) REFERENCES workflow_executions(id),
    CONSTRAINT fk_workflow_context_patch_node_execution FOREIGN KEY (node_execution_id) REFERENCES node_executions(id),
    CONSTRAINT fk_workflow_context_patch_attempt FOREIGN KEY (attempt_id) REFERENCES node_attempts(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE execution_end_deliveries (
    tenant_id BINARY(16) NOT NULL,
    execution_id BINARY(16) NOT NULL,
    sequence_number BIGINT UNSIGNED NOT NULL,
    source_node_execution_id BINARY(16) NOT NULL,
    source_node_id VARCHAR(128) NOT NULL,
    source_port VARCHAR(128) NOT NULL,
    target_port ENUM('main', 'error') NOT NULL,
    payload_json JSON NOT NULL,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    PRIMARY KEY (execution_id, sequence_number),
    KEY idx_end_delivery_terminal (tenant_id, execution_id, target_port, sequence_number),
    CONSTRAINT fk_end_delivery_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_end_delivery_execution FOREIGN KEY (execution_id) REFERENCES workflow_executions(id),
    CONSTRAINT fk_end_delivery_source FOREIGN KEY (source_node_execution_id) REFERENCES node_executions(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

ALTER TABLE node_definitions
    MODIFY COLUMN source_type ENUM('platform', 'remote', 'composite') NOT NULL;

UPDATE node_definitions
SET status = 'disabled'
WHERE tenant_id IS NULL AND node_type = 'sub_workflow';

ALTER TABLE application_deployments
    DROP COLUMN input_schema_json,
    DROP COLUMN output_schema_json,
    DROP COLUMN output_expression;
