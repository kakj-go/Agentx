# plan6 冻结契约

本文件记录实现后的当前协议，不描述兼容窗口。未知版本直接拒绝。

| 契约 | 当前版本 | 权威位置 |
|---|---:|---|
| Workflow Definition | 8.0 | `agentx-domain::WORKFLOW_SCHEMA_VERSION` |
| Node Manifest | 3.0 | `agentx-node-protocol::NODE_PROTOCOL_VERSION` |
| Canvas Plugin Package | 1 | `manifest.json.protocolVersion` |
| TypeScript Plugin SDK API | 1 | `PLUGIN_SDK_API_VERSION` |
| Node Runner RPC | 1 | `runner.initialize` |
| Worker Protocol | 1 | `agentx-runtime-contracts::WORKER_PROTOCOL_VERSION` |
| Runtime Internal API | 1 | `agentx-runtime-contracts::INTERNAL_API_VERSION` |
| Trace Envelope | 1 | `TraceEventEnvelopeV1` |

## 包身份和锁

包ID格式为小写`publisher/name`，`agentx/*`保留给内置包。版本使用SemVer。上传ZIP的SHA-256是内容身份；同一租户内同摘要唯一，同一包版本不同摘要冲突。Workflow节点以`nodeType/typeVersion`选择版本，Compiler在IR中冻结：

```json
{
  "packageId": "acme/json-mapper",
  "packageVersion": "1.0.0",
  "bundleDigest": "sha256:...",
  "runtimeEntry": "runtime/entry.js",
  "runtimeArtifact": { "objectId": "uuid", "contentHash": "sha256:...", "sizeBytes": 1234, "mediaType": "text/javascript" },
  "uiEntry": "ui/entry.js",
  "uiSource": "...",
  "uiStyles": "...",
  "traceRenderers": []
}
```

同一Workflow中相同`packageId`只能出现同一`packageVersion + bundleDigest`。更新默认版本不修改既有节点。Runtime只使用Bundle/Work Package中的冻结绑定。

## Runner消息

传输是stdin/stdout UTF-8 JSON Lines，每条消息使用JSON-RPC 2.0对象。Runner先接收：

```json
{"jsonrpc":"2.0","id":"...","method":"runner.initialize","params":{"protocolVersion":1,"sdkApiVersion":1}}
```

业务请求为`node.execute`，设计时请求为`node.resolveDefinition`或`node.invokeProvider`，取消为`invocation.cancel`。Runner通知为`trace.event`。反向请求固定为`host.http`、`host.model`、`host.credentials.list`和`host.artifacts.put`。RPC错误`-32021`表示外部结果未知，Rust映射为`OutcomeUnknown`。

## Trace扩展

Span Kind只增加`plugin_operation`，Content Kind只增加`plugin_content`。具体业务类型由`packageId/packageVersion/bundleDigest/nodeType/typeVersion/contentType/contentVersion`确定。Renderer声明包含`contentType/contentVersion/exportName/schema`；Runtime先验证Schema，UI再按精确摘要加载，不能以当前默认版本替代历史制品。

## 构建运行时

- Node.js：`24.20.0`，运行镜像固定`node:24.20.0-bookworm-slim@sha256:ba849c60be29959425b8734d57b8b4b7d56f98edd9504c9af091d5281095a71e`。
- pnpm：仓库`packageManager`声明`pnpm@11.9.0`。
- TypeScript：`~6.0.2`。
- 插件产物：自包含ESM，不允许未解析静态import、第二份React、`node_modules`或npm锁文件进入ZIP。
