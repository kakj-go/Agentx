# Runtime migrations

`0001_initial.sql` is generated from the frozen V2 table disposition and is the
only initial schema for the Runtime MySQL target. It contains Runtime-owned
tables plus local projections needed to execute an admitted Work Package.
Cross-plane identifiers are logical references; no Control foreign keys are
created here.
