# Observability migrations

This directory is executed only against the Observability ClickHouse target.
It has an independent migration history/lock and must not be run by a Control
or Runtime MySQL migration Job. The `workflow_trace_events` table is
deduplicated by `event_id`/`row_version`; Runtime MySQL remains authoritative
for execution status.

`0002_query_and_observability.sql` destructively creates the current semantic
Trace schema with explicit event/span kinds, Runtime entity IDs, ordered
`content_kind` events, and redacted inline previews. Historical flat Trace data
is not interpreted or migrated; deployment recreates the Observability domain.
