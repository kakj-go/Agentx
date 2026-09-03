-- V2-06A covering indexes for multi-replica Claim/Lease loops. No authority
-- table or historical migration is changed by this migration.

ALTER TABLE runtime_commands
    ADD KEY idx_v206_runtime_command_claim
        (status, available_at, locked_until, created_at, id);

ALTER TABLE execution_outbox
    ADD KEY idx_v206_execution_outbox_claim
        (status, message_type, available_at, locked_until, created_at, id);

ALTER TABLE node_attempts
    ADD KEY idx_v206_node_attempt_claim
        (status, capability, locked_until, created_at, id);


ALTER TABLE trigger_bindings
    ADD KEY idx_v206_trigger_claim
        (status, trigger_kind, next_poll_at, locked_until, id);

ALTER TABLE sandbox_leases
    ADD KEY idx_v206_sandbox_claim
        (status, expires_at, locked_until, id);

ALTER TABLE retention_runs
    ADD KEY idx_v206_runtime_retention_claim
        (status, available_at, locked_until, created_at, id);

ALTER TABLE retention_items
    ADD KEY idx_v206_runtime_retention_item_claim
        (status, locked_until, created_at, id);

ALTER TABLE bundle_gc_runs
    ADD KEY idx_v206_bundle_gc_run_claim
        (status, locked_until, created_at, id);

ALTER TABLE bundle_gc_items
    ADD KEY idx_v206_bundle_gc_item_claim
        (status, locked_until, created_at, id);

ALTER TABLE trace_outbox
    ADD KEY idx_v206_trace_outbox_claim
        (status, available_at, locked_until, created_at, event_id);

