ALTER TABLE workflow_drafts
    ADD COLUMN editor_json JSON NULL AFTER definition_json,
    ADD COLUMN editor_hash VARCHAR(80) NULL AFTER content_hash;

ALTER TABLE workflow_draft_revisions
    ADD COLUMN editor_json JSON NULL AFTER definition_json,
    ADD COLUMN editor_hash VARCHAR(80) NULL AFTER content_hash;

ALTER TABLE workflow_versions
    ADD COLUMN editor_json JSON NULL AFTER definition_json,
    ADD COLUMN editor_hash VARCHAR(80) NULL AFTER content_hash;

ALTER TABLE workflow_executions
    DROP FOREIGN KEY fk_execution_version,
    ADD COLUMN source_kind VARCHAR(32) NOT NULL DEFAULT 'version' AFTER workflow_version_id,
    ADD COLUMN source_id BINARY(16) NULL AFTER source_kind,
    ADD COLUMN source_revision BIGINT UNSIGNED NULL AFTER source_id,
    MODIFY COLUMN workflow_version_id BINARY(16) NULL,
    ADD CONSTRAINT fk_execution_version_m6 FOREIGN KEY (workflow_version_id) REFERENCES workflow_versions(id);

UPDATE workflow_executions SET source_id = workflow_version_id WHERE source_id IS NULL;

ALTER TABLE execution_snapshots
    DROP FOREIGN KEY fk_execution_snapshot_version,
    ADD COLUMN manifest_snapshot_json JSON NULL AFTER compiler_version,
    ADD COLUMN debug_plan_json JSON NULL AFTER manifest_snapshot_json,
    ADD COLUMN debug_overlay_snapshot_json JSON NULL AFTER debug_plan_json,
    MODIFY COLUMN workflow_version_id BINARY(16) NULL,
    MODIFY COLUMN compiled_ir_hash VARCHAR(255) NOT NULL,
    ADD CONSTRAINT fk_execution_snapshot_version_m6 FOREIGN KEY (workflow_version_id) REFERENCES workflow_versions(id);

CREATE TABLE workflow_debug_overlays (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    workflow_id BINARY(16) NOT NULL,
    node_id VARCHAR(128) NOT NULL,
    kind ENUM('pin_data', 'mock_output', 'temporary_input', 'history_output', 'artifact') NOT NULL,
    payload_json JSON NOT NULL,
    artifact_id BINARY(16) NULL,
    schema_hash VARCHAR(96) NULL,
    stale BOOLEAN NOT NULL DEFAULT FALSE,
    updated_by BINARY(16) NOT NULL,
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    updated_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    UNIQUE KEY uq_workflow_debug_overlay_node (tenant_id, workflow_id, node_id),
    KEY idx_workflow_debug_overlay_workflow (tenant_id, workflow_id, updated_at),
    CONSTRAINT fk_debug_overlay_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_debug_overlay_workflow FOREIGN KEY (workflow_id) REFERENCES workflows(id),
    CONSTRAINT fk_debug_overlay_artifact FOREIGN KEY (artifact_id) REFERENCES artifacts(id),
    CONSTRAINT fk_debug_overlay_user FOREIGN KEY (updated_by) REFERENCES users(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

ALTER TABLE workflow_version_resources
    ADD COLUMN binding_id VARCHAR(128) NULL AFTER node_id,
    ADD COLUMN binding_role VARCHAR(64) NULL AFTER binding_id;
