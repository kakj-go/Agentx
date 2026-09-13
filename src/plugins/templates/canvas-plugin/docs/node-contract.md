# Node contract

`resolveDefinition(configuration, upstreamContracts)` 返回 `complete/incomplete/invalid`、有效输入输出端口、主输出 Schema 和可选 `outputPortSchemas`。Draft 可保存 `incomplete`；`invalid` 不推进修订，创建 Version、Debug、发布、Fork 和评测只接受 `complete`。同一调用输入和包摘要必须产生稳定结果。

`manifest.json` identifies one immutable ZIP package. The import digest is computed from the ZIP bytes; never edit a package after assigning `packageVersion`.

| Package field | Required value |
|---|---|
| `protocolVersion` | `1` |
| `sdkApiVersion` | `1` |
| `packageId` | lowercase `publisher/name`; `agentx/*` is reserved |
| `packageVersion` | semantic version such as `1.2.0` |
| `nodes` | unique paths to Node Manifest JSON files |
| `runtimeEntry` | self-contained compiled ESM; no bare imports |
| `uiEntry` / `uiStylesEntry` | optional self-contained React ESM and CSS |
| `traceRenderers` | content type, positive content version, UI export name, and JSON Schema |

Every file under `nodes/` is a Node Manifest 3.0. `nodeType` plus integer `version` is the persistent node identity. Increase the integer when parameter, port, output, binding-slot, or execution semantics change; never reuse it for different behavior.

`inputPorts` and `outputPorts` declare execution edges. Each port has `name`, `kind` (`main` or `error`), `required`, and `variadic`. `parameterSchema` validates the resolved configuration. `outputSchema` describes `item.json`; `outputPortSchemas` overrides it per port. `outputCardinality` is `many`, `exactly_one`, or `at_most_one`.

`bindingSlots` request immutable Agentx resources. A slot declares `name`, `resourceType`, inspector placement, whether it is required, and whether multiple bindings are allowed. The Workflow owns authorization and stores the resource reference; plugin code sees only Runtime host operations.

Set `capability` to `plugin_nodejs`. Use `sideEffectLevel` to describe execution behavior. Plugins cannot claim native Loop, Approval, Sub-workflow, Agent, Model, Code, or boundary semantics.

A Workflow locks the exact package version and digest through its node manifests. One Workflow cannot mix two versions of the same package. Updating a package default never changes an existing node.
