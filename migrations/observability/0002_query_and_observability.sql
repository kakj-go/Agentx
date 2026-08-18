-- V2-05 destructive Trace v1 rewrite. The ClickHouse domain is recreated.
DROP TABLE IF EXISTS workflow_trace_events;

CREATE TABLE workflow_trace_events (
    event_id UUID,
    tenant_id UUID,
    execution_id UUID,
    execution_sequence UInt64,
    trace_id UUID,
    span_id UUID,
    parent_span_id Nullable(UUID),
    node_execution_id Nullable(UUID),
    attempt_id Nullable(UUID),
    runtime_call_id Nullable(UUID),
    workflow_id Nullable(UUID),
    application_id Nullable(UUID),
    resource_type LowCardinality(Nullable(String)),
    resource_id Nullable(UUID),
    resource_version Nullable(String),
    event_type LowCardinality(String),
    status LowCardinality(String),
    error_code Nullable(String),
    duration_ms Nullable(UInt64),
    input_tokens Nullable(UInt64),
    output_tokens Nullable(UInt64),
    cost_micros UInt64,
    attributes_json String,
    content_ref Nullable(UUID),
    content_hash String,
    occurred_at DateTime64(6,'UTC'),
    ingested_at DateTime64(6,'UTC') DEFAULT now64(6),
    row_version UInt64 DEFAULT toUnixTimestamp64Micro(ingested_at)
)
ENGINE = ReplacingMergeTree(row_version)
PARTITION BY (tenant_id,toYYYYMM(occurred_at))
ORDER BY (tenant_id,execution_id,execution_sequence,event_id,content_hash)
TTL toDateTime(occurred_at) + INTERVAL 180 DAY DELETE;

CREATE TABLE trace_ingest_conflicts (
    event_id UUID,
    tenant_id UUID,
    execution_id UUID,
    existing_hash String,
    conflicting_hash String,
    stream_id String,
    detected_at DateTime64(6,'UTC') DEFAULT now64(6)
)
ENGINE = MergeTree
PARTITION BY toYYYYMM(detected_at)
ORDER BY (tenant_id,event_id,detected_at);

CREATE TABLE observability_consumer_health (
    consumer_id String,
    state LowCardinality(String),
    last_stream_id String,
    pending_count UInt64,
    conflict_count UInt64,
    last_error String,
    observed_at DateTime64(6,'UTC') DEFAULT now64(6),
    row_version UInt64 DEFAULT toUnixTimestamp64Micro(observed_at)
)
ENGINE = ReplacingMergeTree(row_version)
ORDER BY consumer_id;
