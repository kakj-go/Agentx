-- V2-06A claim scans use stable ordering and a left-most prefix matching the
-- complete eligibility predicate. Earlier migrations remain immutable.

ALTER TABLE outbox
    ADD KEY idx_v206_control_outbox_claim
        (aggregate_type, status, available_at, locked_until, occurred_at, id);

ALTER TABLE publish_attempts
    ADD KEY idx_v206_publish_attempt_claim
        (state, available_at, locked_until, created_at, id);

ALTER TABLE runtime_projection_cursors
    ADD KEY idx_v206_projection_lease
        (projection_name, partition_key, locked_until, fencing_token);

ALTER TABLE retention_runs
    ADD KEY idx_v206_control_retention_claim
        (status, available_at, locked_until, created_at, id);

