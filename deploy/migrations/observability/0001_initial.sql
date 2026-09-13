-- V2-01 Observability schema. Runtime remains the source of execution truth;
-- ClickHouse only stores immutable trace facts and derived observability data.
CREATE TABLE IF NOT EXISTS workflow_trace_events (
    event_id UUID,
    tenant_id UUID,
    trace_id UUID,
    span_id UUID,
    parent_span_id Nullable(UUID),
    execution_id UUID,
    workflow_id UUID,
    workflow_version_id Nullable(UUID),
    node_execution_id Nullable(UUID),
    attempt_id Nullable(UUID),
    agent_run_id Nullable(UUID),
    runtime_call_id Nullable(UUID),
    sandbox_id Nullable(String),
    resource_type Nullable(String),
    resource_id Nullable(UUID),
    resource_version_id Nullable(UUID),
    node_id Nullable(String),
    event_type LowCardinality(String),
    status LowCardinality(String),
    event_time DateTime64(6, 'UTC'),
    duration_ms Nullable(UInt64),
    run_index UInt32,
    iteration_index UInt32,
    model_name Nullable(String),
    provider_name Nullable(String),
    mcp_tool_name Nullable(String),
    input_tokens Nullable(UInt64),
    output_tokens Nullable(UInt64),
    cost_micros UInt64,
    error_code Nullable(String),
    error_message Nullable(String),
    stop_reason Nullable(String),
    partial UInt8,
    attributes_json String,
    content_ref Nullable(UUID),
    ingested_at DateTime64(6, 'UTC') DEFAULT now64(6),
    row_version UInt64 DEFAULT toUnixTimestamp64Micro(ingested_at)
)
ENGINE = ReplacingMergeTree(row_version)
PARTITION BY toYYYYMM(event_time)
ORDER BY (tenant_id, execution_id, event_time, event_id)
TTL toDateTime(event_time) + INTERVAL 180 DAY DELETE;
