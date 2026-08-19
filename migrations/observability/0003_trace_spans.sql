ALTER TABLE workflow_trace_events
    ADD COLUMN event_kind LowCardinality(String) AFTER parent_span_id,
    ADD COLUMN span_kind LowCardinality(String) AFTER event_kind,
    ADD COLUMN span_name String AFTER span_kind,
    ADD COLUMN agent_run_id Nullable(UUID) AFTER attempt_id,
    ADD COLUMN agent_iteration_id Nullable(UUID) AFTER agent_run_id,
    ADD COLUMN sandbox_lease_id Nullable(UUID) AFTER runtime_call_id,
    ADD COLUMN wait_id Nullable(UUID) AFTER sandbox_lease_id,
    ADD COLUMN error_message Nullable(String) AFTER error_code,
    ADD COLUMN content_role LowCardinality(Nullable(String)) AFTER content_ref,
    ADD COLUMN content_preview_json Nullable(String) AFTER content_role;
