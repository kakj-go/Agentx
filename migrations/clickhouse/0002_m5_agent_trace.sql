ALTER TABLE workflow_trace_events
    ADD COLUMN IF NOT EXISTS attempt_id Nullable(UUID) AFTER node_execution_id,
    ADD COLUMN IF NOT EXISTS agent_run_id Nullable(UUID) AFTER attempt_id,
    ADD COLUMN IF NOT EXISTS runtime_call_id Nullable(UUID) AFTER agent_run_id,
    ADD COLUMN IF NOT EXISTS sandbox_id Nullable(String) AFTER runtime_call_id,
    ADD COLUMN IF NOT EXISTS resource_type Nullable(String) AFTER sandbox_id,
    ADD COLUMN IF NOT EXISTS resource_id Nullable(UUID) AFTER resource_type,
    ADD COLUMN IF NOT EXISTS resource_version_id Nullable(UUID) AFTER resource_id,
    ADD COLUMN IF NOT EXISTS stop_reason Nullable(String) AFTER error_message,
    ADD COLUMN IF NOT EXISTS partial UInt8 DEFAULT 0 AFTER stop_reason;
