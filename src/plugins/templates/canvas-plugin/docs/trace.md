# Trace

平台限制每次调用最多 64 个 Span、16 层嵌套、每 Span 32 份业务内容、128 个事件和 64 个属性。超限返回 `PLUGIN_TRACE_BUDGET_EXCEEDED`。大业务内容自动外置为执行授权 Artifact；renderer 失败时标准 JSON 与下载入口仍保留。

Agentx always records Execution, Node, and Attempt spans with resolved input, output, error, duration, and frozen package identity. Plugin code adds business spans with `context.trace.span(name, body)`.

Inside the callback:

- `span.setAttribute(name, json)` adds bounded diagnostic attributes.
- `span.event(name, attributes)` emits a timestamped event.
- `span.content({ type, version, label?, data })` emits business content.

Content `type` must be namespaced, for example `acme.crm/customer-matches`; `version` is a positive integer. Declare the same type/version in `manifest.json.traceRenderers` with a JSON Schema and renderer export. Invalid content is recorded as a diagnostic warning and cannot change the business result.

Spans stream as started, updated, and finished events while execution is active. Nested and concurrent Promise work keeps its parent through the Runner async context. Host HTTP and Model calls retain their Runtime Call identity and attach below the current plugin span.

Trace is diagnostic. Never use it as recovery state, never put secrets in it, and do not change the node result when Trace delivery is unavailable. Large standard content is externalized by Agentx; plugin content is bounded and redacted by the host.
