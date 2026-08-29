-- Workflow Definition 7.0 multi-exit: remember which exit node received each
-- terminal delivery so result materialization can pick that exit's mappings.
ALTER TABLE execution_end_deliveries
    ADD COLUMN target_exit_id VARCHAR(128) NOT NULL DEFAULT '' AFTER target_port;
