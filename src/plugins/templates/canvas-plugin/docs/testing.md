# Testing

Run these commands from the template root:

```bash
pnpm dev
pnpm check
pnpm test
pnpm build
pnpm pack:plugin
```

`check` validates TypeScript and package paths. `test` builds and executes the compiled module through the production Agentx Runner protocol. `build` emits self-contained ESM/CSS. `pack:plugin` creates `dist/*.agentx-plugin` with deterministic entry ordering.

Before delivery, import the ZIP through **Resources → Canvas Plugins**, add the node by real canvas interaction, configure it, save and reopen, run it, reference its output downstream, and inspect both standard and custom Trace views. Local package tests do not replace Agentx Kubernetes E2E.

Test at least empty and multiple Item inputs, every declared port, invalid parameters, deadline cancellation, host-call errors, idempotent replay, renderer failure, and output Schema violations. A package version must produce the same behavior and files on every build.
