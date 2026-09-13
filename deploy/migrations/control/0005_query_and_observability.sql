-- V2-05 Control pull projection generations and query boundary state.
SET NAMES utf8mb4;
SET time_zone = '+00:00';

CREATE TABLE runtime_projection_status (
    projection_name VARCHAR(96) NOT NULL,
    partition_key VARCHAR(96) NOT NULL,
    state ENUM('ready','rebuilding','degraded','error') NOT NULL DEFAULT 'rebuilding',
    current_cursor BIGINT UNSIGNED NOT NULL DEFAULT 0,
    retention_floor_cursor BIGINT UNSIGNED NOT NULL DEFAULT 0,
    active_generation BIGINT UNSIGNED NOT NULL DEFAULT 0,
    building_generation BIGINT UNSIGNED NULL,
    snapshot_upper_cursor BIGINT UNSIGNED NULL,
    last_error_code VARCHAR(128) NULL,
    last_error_message VARCHAR(1000) NULL,
    last_success_at TIMESTAMP(6) NULL,
    updated_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6),
    PRIMARY KEY (projection_name,partition_key),
    KEY idx_projection_status_state (state,updated_at)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE projection_rebuild_items (
    projection_name VARCHAR(96) NOT NULL,
    partition_key VARCHAR(96) NOT NULL,
    generation BIGINT UNSIGNED NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    object_type VARCHAR(32) NOT NULL,
    object_id BINARY(16) NOT NULL,
    object_version BIGINT UNSIGNED NOT NULL,
    source_event_cursor BIGINT UNSIGNED NOT NULL,
    content_hash CHAR(71) NOT NULL,
    payload_json JSON NOT NULL,
    projection_deleted BOOLEAN NOT NULL DEFAULT FALSE,
    applied_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    PRIMARY KEY (projection_name,partition_key,generation,tenant_id,object_type,object_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

ALTER TABLE projection_receipts
    ADD COLUMN content_hash CHAR(71) NOT NULL AFTER tenant_id,
    ADD COLUMN object_version BIGINT UNSIGNED NOT NULL AFTER content_hash,
    ADD COLUMN event_cursor BIGINT UNSIGNED NOT NULL AFTER object_version,
    ADD COLUMN outcome ENUM('applied','ignored_old_version','replayed') NOT NULL AFTER event_cursor,
    ADD UNIQUE KEY uq_projection_cursor (projector_name,event_cursor);

ALTER TABLE evaluation_runs
    ADD COLUMN projection_generation BIGINT UNSIGNED NOT NULL DEFAULT 1 AFTER intent_version,
    ADD COLUMN source_event_id BINARY(16) NULL AFTER projection_generation,
    ADD COLUMN source_event_cursor BIGINT UNSIGNED NOT NULL DEFAULT 0 AFTER source_event_id,
    ADD COLUMN projection_deleted BOOLEAN NOT NULL DEFAULT FALSE AFTER source_event_cursor,
    ADD COLUMN completed_cases BIGINT UNSIGNED NOT NULL DEFAULT 0 AFTER projection_deleted,
    ADD COLUMN total_cases BIGINT UNSIGNED NOT NULL DEFAULT 0 AFTER completed_cases,
    ADD COLUMN runtime_report_json JSON NULL AFTER total_cases,
    ADD KEY idx_control_evaluation_generation (tenant_id,projection_generation,status,created_at);

ALTER TABLE evaluation_case_projection
    ADD COLUMN projection_generation BIGINT UNSIGNED NOT NULL DEFAULT 1 AFTER status,
    ADD COLUMN source_event_cursor BIGINT UNSIGNED NOT NULL DEFAULT 0 AFTER projection_generation,
    ADD KEY idx_control_evaluation_case_generation (tenant_id,projection_generation,evaluation_run_id,status);

ALTER TABLE evaluation_rule_results
    ADD COLUMN projection_generation BIGINT UNSIGNED NOT NULL DEFAULT 1 AFTER status,
    ADD COLUMN source_event_cursor BIGINT UNSIGNED NOT NULL DEFAULT 0 AFTER projection_generation,
    ADD KEY idx_control_evaluation_rule_generation (tenant_id,projection_generation,evaluation_run_case_id,status);

ALTER TABLE workflow_debug_runs
    ADD COLUMN projection_generation BIGINT UNSIGNED NOT NULL DEFAULT 1 AFTER status,
    ADD COLUMN source_event_id BINARY(16) NULL AFTER projection_generation,
    ADD COLUMN source_event_cursor BIGINT UNSIGNED NOT NULL DEFAULT 0 AFTER source_event_id,
    ADD COLUMN projection_deleted BOOLEAN NOT NULL DEFAULT FALSE AFTER source_event_cursor,
    ADD KEY idx_control_debug_generation (tenant_id,projection_generation,status,created_at);

ALTER TABLE retention_runs
    ADD COLUMN projection_generation BIGINT UNSIGNED NOT NULL DEFAULT 1 AFTER policy_version,
    ADD COLUMN source_event_id BINARY(16) NULL AFTER projection_generation,
    ADD COLUMN source_event_cursor BIGINT UNSIGNED NOT NULL DEFAULT 0 AFTER source_event_id,
    ADD COLUMN projection_deleted BOOLEAN NOT NULL DEFAULT FALSE AFTER source_event_cursor,
    ADD COLUMN failed_count BIGINT UNSIGNED NOT NULL DEFAULT 0 AFTER projection_deleted,
    ADD KEY idx_control_retention_generation (tenant_id,projection_generation,status,created_at);

ALTER TABLE retention_items
    MODIFY status ENUM('candidate','marked','deleting','blocked','deleted','failed') NOT NULL,
    ADD COLUMN projection_generation BIGINT UNSIGNED NOT NULL DEFAULT 1 AFTER status,
    ADD COLUMN source_event_cursor BIGINT UNSIGNED NOT NULL DEFAULT 0 AFTER projection_generation,
    ADD COLUMN projection_deleted BOOLEAN NOT NULL DEFAULT FALSE AFTER source_event_cursor,
    ADD KEY idx_control_retention_item_generation (tenant_id,projection_generation,retention_run_id,status);

ALTER TABLE notifications
    ADD COLUMN source_plane ENUM('control','runtime') NOT NULL DEFAULT 'control' AFTER source_event_id,
    ADD COLUMN runtime_object_version BIGINT UNSIGNED NOT NULL DEFAULT 0 AFTER source_plane,
    ADD COLUMN projection_generation BIGINT UNSIGNED NOT NULL DEFAULT 1 AFTER tone,
    ADD COLUMN source_event_cursor BIGINT UNSIGNED NOT NULL DEFAULT 0 AFTER projection_generation,
    ADD COLUMN projection_deleted BOOLEAN NOT NULL DEFAULT FALSE AFTER source_event_cursor,
    ADD KEY idx_control_notification_generation (tenant_id,projection_generation,created_at);

DROP TABLE trace_delivery_status;
