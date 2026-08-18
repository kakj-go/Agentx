# Control migrations

`0001_initial.sql` is generated from the frozen V2 table disposition and is the
only initial schema for the Control MySQL target. It contains Control-owned
tables plus the Control replacement of split contracts. Runtime replacement
tables are intentionally absent.

Incremental migrations are append-only. `0007_cutover.sql` adds the separate
Workflow Studio editor document required by the V2-only Platform Control API;
it does not introduce a Runtime-owned table or change the frozen initial
schema.
