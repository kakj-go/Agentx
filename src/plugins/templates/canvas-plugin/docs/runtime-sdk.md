# Runtime SDK

Export `execute(context)`. Parameters are resolved values, inputs are grouped by port, and `execution.idempotencyKey`, deadline and AbortSignal describe the Attempt. Return `completed` or `failed`; plugins cannot suspend or schedule work.

`context.perItemParameters` contains schema-resolved values for every input item. Preserve an input `Item` with object spread when producing a transformed item so lineage, metadata, and binary references remain attached.

Use the host bridges for effects and protected resources:

- `context.http(request)` routes through Agentx egress, credential injection, runtime-call idempotency, Artifact handling, and Trace. Mutating requests require a stable `idempotencyKey`.
- `context.model(request)` invokes a frozen Model resource and returns its normalized JSON result.
- `context.credentials.list()` returns non-secret descriptors. Plugins never receive raw credential bytes; pass `credentialIndex` to `context.http`.
- `context.artifacts.put({ path, fileName, contentType })` stores an execution-scoped Artifact and returns an `ArtifactRef`.

See `src/examples/http-trace.ts` for an HTTP call nested under a plugin Trace span.

Option providers receive `(input, context)`. Their `context` exposes the same `signal`, `http`, `model`, `credentials.list` and `artifacts.put` bridges through the Runtime design pool. Only resource references already selected on the node are available. Design-time HTTP is read-only, uses frozen resource snapshots, and has a ten-second deadline; missing or inaccessible resources fail the Provider request instead of returning guessed options.

## Invocation files (SDK API 2)

Every execute/provider call owns a fresh Node process and working directory. Module state, files and child processes are discarded after the call. Runner RPC is 2; package format remains 1. Old SDK APIs are rejected on import.

`context.artifacts.read(reference)` accepts an ArtifactRef from this invocation's inputs or host results. It streams the authorized object into the working directory and returns `{ path, fileName, contentType, sizeBytes, sha256 }`. Paths are relative to `process.cwd()`. Arbitrary Artifact IDs, absolute paths, parent traversal and symbolic links are rejected. Files are limited to 64 MiB and total transfers to 128 MiB per invocation.

```ts
const { createReadStream, createWriteStream } = await import('node:fs')
const { pipeline } = await import('node:stream/promises')
const local = await context.artifacts.read(inputArtifact)
await pipeline(createReadStream(local.path), createWriteStream('output.bin'))
const output = await context.artifacts.put({ path: 'output.bin', fileName: 'output.bin', contentType: 'application/octet-stream' })
```

For small JSON use `writeFile('result.json', JSON.stringify(value))` and the same put API. See `src/examples/files.ts` for a complete typed example. File content never crosses JSON-RPC. Provider files exist only in the operation directory and are deleted when it ends; providers may read only files created by their own operation.

Trace budgets never throw into plugin business logic. Excess, invalid or backpressured diagnostics are dropped and the Attempt receives `traceIncomplete` with a dropped-diagnostics count when Trace storage is available.
