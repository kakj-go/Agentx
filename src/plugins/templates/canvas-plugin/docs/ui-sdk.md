# UI SDK

`createUi(host)` 接收宿主唯一 React 实例、公共组件、当前 `locale`、`theme`、`portalRoot`、Manifest 中声明资产对应的 `assets` Data URL，以及设计时 `resolveDefinition/invokeProvider`。不得自行打包 React。

Provider 搜索必须把当前参数传入，为每次请求创建 AbortController，并在新搜索、卸载或只读切换时取消旧请求；晚到响应不得覆盖新结果。

Export `createUi(host)` from the compiled `uiEntry`. Do not bundle React or import Agentx source files. The host supplies the single React runtime and themed components: `Field`, `Input`, `Button`, `Select`, and `SmartInput`.

`createUi` returns:

| Export | Props | Purpose |
|---|---|---|
| `Panel` | `parameters`, `readOnly`, `fieldErrors`, `providerOptions`, `referenceCatalog`, `resources`, `updateParameters` | required configuration panel |
| `Canvas` | `parameters` | optional compact body inside the canvas node |
| `Result` | `value` | optional read-only execution result |
| `traceRenderers[key]` | `value` | optional read-only business Trace view |

Call `updateParameters(patch)` for one user edit. The host records the transaction for autosave and Undo/Redo. Respect `readOnly`; display the matching `fieldErrors` entry next to the editable field. Use `SmartInput` when a field accepts literals, variables, or templates so the host keeps reference filtering and typed validation.

Provider options are keyed by the provider name declared in the Node Manifest. Treat all Result and Trace props as immutable. A thrown render error is isolated by the host and the standard JSON view remains available.

CSS is scoped by the plugin author and loaded once per bundle digest. Avoid page-wide element selectors and fixed z-index values; use the host components for focus, theme, portal, and accessibility behavior.
