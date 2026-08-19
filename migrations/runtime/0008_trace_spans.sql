ALTER TABLE agent_runs
    ADD COLUMN attempt_id BINARY(16) NULL AFTER node_execution_id,
    ADD KEY idx_agent_run_attempt (tenant_id, attempt_id);
