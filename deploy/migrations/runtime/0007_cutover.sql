-- V2-08A local functional cutover fixes.
-- Composite deadlines are Runtime-authoritative and are evaluated with database time.

ALTER TABLE execution_children
    ADD COLUMN deadline_at TIMESTAMP(6) NULL AFTER context_overlay_hash,
    ADD KEY idx_execution_children_deadline (
        relationship, merge_status, deadline_at, tenant_id, child_execution_id
    );
