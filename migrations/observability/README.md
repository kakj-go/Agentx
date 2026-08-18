# Observability migrations

This directory is executed only against the Observability ClickHouse target.
It has an independent migration history/lock and must not be run by a Control
or Runtime MySQL migration Job. The `workflow_trace_events` table is
deduplicated by `event_id`/`row_version`; Runtime MySQL remains authoritative
for execution status.
