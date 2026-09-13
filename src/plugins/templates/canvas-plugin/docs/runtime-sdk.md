# Runtime SDK

Export `execute(context)`. Parameters are resolved values, inputs are grouped by port, and `execution.idempotencyKey`, deadline and AbortSignal describe the Attempt. Return `completed` or `failed`; plugins cannot suspend or schedule work.

`context.perItemParameters` contains schema-resolved values for every input item. Preserve an input `Item` with object spread when producing a transformed item so lineage, metadata, and binary references remain attached.

Use the host bridges for effects and protected resources:

- `context.http(request)` routes through Agentx egress, credential injection, runtime-call idempotency, Artifact handling, and Trace. Mutating requests require a stable `idempotencyKey`.
- `context.model(request)` invokes a frozen Model resource and returns its normalized JSON result.
- `context.credentials.list()` returns non-secret descriptors. Plugins never receive raw credential bytes; pass `credentialIndex` to `context.http`.
- `context.artifacts.put({ bytesBase64, fileName, contentType })` stores an execution-scoped Artifact and returns an `ArtifactRef`.

See `src/examples/http-trace.ts` for an HTTP call nested under a plugin Trace span.

Option providers receive `(input, context)`. Their `context` exposes the same `signal`, `http`, `model`, `credentials.list` and `artifacts.put` bridges through the Runtime design pool. Only resource references already selected on the node are available. Design-time HTTP is read-only, uses frozen resource snapshots, and has a ten-second deadline; missing or inaccessible resources fail the Provider request instead of returning guessed options.
