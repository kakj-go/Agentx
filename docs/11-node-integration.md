# Node 服务接入

## 1. 接入边界

Agentx M4 不发布 Rust、JavaScript、Python 或其他语言的公共 Node SDK。外部节点服务只依赖版本化 HTTP 契约，因此可以使用任意语言实现；平台内部的 Rust Runner 不是扩展接口。

接入时使用以下仓库产物：

- `openapi/node-api.json`：Action、动态 Provider 和 Lifecycle OpenAPI 3.1 契约。
- `schemas/node-manifest.schema.json`：不可变 Node Manifest Version。
- `schemas/node-action-request.schema.json` 与 `schemas/node-action-result.schema.json`：Action 请求和三类结果。
- `schemas/workflow-definition.schema.json`：Definition `5.0` 契约。
- `services/echo-node`：具备认证、协议校验和一致性测试的参考服务。

Node Protocol 当前版本为 `1.0`。Node Manifest 的 `protocolVersion`、Action/Provider/Lifecycle 请求版本必须完全匹配；平台不会把未知版本降级或猜测转换。Node Type Version 由 Workflow Version 固定，已发布版本不得原地修改。

### 1.1 Manifest UI 画布契约

`uiSchema.canvas.role` 是 Studio 执行节点的唯一视觉角色来源，允许值为 `default`、`trigger`、`branch`、`flow`、`merge`、`loop`、`suspend`、`approval`、`sub_workflow`、`agent`、`code` 和 `error_handler`。内置节点必须在 Rust Registry 中显式写入该字段；Catalog 反序列化时拒绝未知角色，缺少字段的 Manifest 仅由前端回退为 `default`，不得再按 `nodeType` 推断形状。Workflow 5.0 不发布 Trigger 节点，`trigger` role 只保留为 Node Protocol 枚举值，固定 Start 使用独立 Boundary 组件。

该字段只控制编辑器外观，不改变端口、执行能力或 Runtime 语义，并进入现有 Manifest Hash。Model、MCP Tool、Memory、RAG 和 Skill 等资源附件继续由 Editor Document 的 `editorKind=binding` 表示，不进入角色枚举，也不能伪装为可执行节点。已创建的 Execution Snapshot 保留固化 Manifest 和 Hash；Catalog Reconcile 后的新执行使用新的 Manifest Hash。

Manifest 可声明 `localizations.zh-CN/en-US`，覆盖 `displayName`、`description`、`keywords`、输入端口、输出端口和 Binding Slot 的展示名。本地化 Map 的 Key 必须引用真实协议端口或 Slot；Catalog 拒绝未知语言和悬空引用。本地化数据进入 Manifest Hash，端口和 Slot 的协议 ID 始终使用英文且不随界面语言变化。

Workflow 5.0 Manifest 还必须声明 `outputSchema`、`outputCardinality`、`expressionCapabilities`、`contextReadCapability`、`contextWriteCapability`、`outputProjectionSchema` 和 `artifactOutputSchema`。可绑定参数在 Parameter Schema 中使用 `x-agentx-dynamicValue` 声明允许模式、命名空间、基数、缺失策略及是否递归绑定；未声明的代码字段不会自动打开 Reference Picker。

编译器把 Manifest 输出 Schema 与节点实例配置、Projection 合成为按端口冻结的 Effective Output Contract 并写入 IR。Worker 不在提交结果时重新查询 Registry；内置、远程、MCP、Plugin 和 Sub-workflow 均按发布时同一契约验证。Model 与 Agent 标准输出固定为 `text`、`reasoningContent`、`structuredOutput`、`citations`、`files`、`usage`、`finishReason` 和 `partial`；Provider message、工具调用、Agent 迭代与原始响应仅进入受权限控制的 Trace/Artifact。

内置 `error_handler` 使用 Error 类型输入 `error` 和 Main 类型输出 `recovered`；`mode=recover` 保留原错误 Item 并继续，`mode=fail` 使用原错误 code/message 终止。连接任意 Error 输出时 Studio 同一事务把源节点 `onError` 设为 `continue_error_output`。

## 2. Endpoint

Node 服务实现三个版本化调用族：

| Endpoint | 用途 | M4 调用方 |
|---|---|---|
| `POST /agentx/node/v1/actions/execute` | 执行普通节点或挂起节点 | Workflow Worker |
| `POST /agentx/node/v1/providers/{provider}/invoke` | 动态选项、搜索和字段映射 | Platform API/Studio Adapter |
| `POST /agentx/node/v1/lifecycle/{operation}` | activate、deactivate、poll、webhook、suspend、resume | Coordinator/Trigger Adapter |

Provider 与 Lifecycle 在 M4 冻结契约；完整动态配置 UI 在 M6 接入，生产 Trigger/Poll/Webhook Lifecycle 在 M7 接通。节点服务不得调用内部 gRPC、直接写 MySQL、创建 Attempt 或推进 Execution。

## 3. 认证与身份

生产部署必须通过 TLS，并使用调用方绑定的 Bearer Token 或部署环境提供的等价工作负载身份。参考 `echo-node` 读取 `AGENTX_REMOTE_NODE_AUTH_TOKEN`；Worker 使用同名 Secret 发起调用。认证失败返回 HTTP `401` 和 `NodeProtocolError`。

Action 请求中的 `tenantId`、`workflowVersionId`、`executionId`、`nodeExecutionId` 和 `attemptId` 是调用上下文，不是节点服务自行授权其他 Agentx API 的凭据。节点服务必须在日志中避免输出完整 Item、Handle、认证 Header 和敏感参数。

## 4. Action 请求

Action 请求包含：

- 固定的 Protocol/Node Version 和 Execution/Attempt 身份。
- `runIndex`、`iterationIndex` 和 `mode`。
- 按输入端口分组的 Items；`branchIndex` 保留多输入顺序。
- 平台已解析的公共参数和逐 Item 参数。远程服务不执行 CEL 或 JavaScript。
- 短时 `artifactHandles` 与 `credentialHandles`。Handle 只对本次调用、节点和 Deadline 有效；长期 Secret 不进入 Action Body。
- 稳定 `idempotencyKey`、RFC3339 `deadline`、可选取消 URL和 Trace Context。

Item 的 `lineage` 可以包含零到多个来源。Merge、按位置合并或聚合不得只保留一个来源；返回 Item 应保留或补充其来源关系。Binary 只传 Artifact Handle、文件名、Content Type 和大小，不内联大 Payload。

外部副作用无法由平台提供通用 Exactly Once。节点服务应以 `idempotencyKey` 去重可重复请求，并将结果至少保留到 Deadline 和最大重投窗口结束。相同 Key、相同请求必须得到同一业务结果；Key 相同但请求不同应拒绝。

### 4.1 Broker Handle 与取消

每个 `InvocationHandle` 同时包含 opaque `handle`、`brokerUrl` 和 `expiresAt`。节点服务使用 `POST brokerUrl` 解析资源，请求体必须回传 `handle`、`tenantId`、`nodeExecutionId` 和 `attemptId`；成功结果按 `kind` 返回 Credential JSON 或带 Content Type、SHA-256 的 Artifact Base64 内容。节点服务不得推导 Broker 地址、持久化 Handle，或把解析出的 Secret、Artifact 内容写入日志和 Trace。

Credential/Artifact Handle 只允许消费一次，并同时绑定 Tenant、Execution、Node Execution、Attempt、Lease 和 Deadline。平台只保存 Token Hash；跨作用域、过期、重放、Execution 已取消或 Lease 已释放的请求都会被拒绝。Credential 只在 Broker 内解密，长期 Secret 不进入 Action 请求或数据库 Handle 记录。

`cancellationUrl` 是只读轮询端点，返回 `cancellationRequested`、`leaseValid` 和 `expiresAt`。远程节点在长任务中应定期查询，并在已请求取消或 Lease 无效时尽快停止；这不会授予节点修改 Execution 的权限。Broker 是 Worker 的调用期接口，不属于公共 Platform API，也不替代 Node Action API。

## 5. Action 结果

HTTP `200` 的 Body 只能是以下一种：

- `completed`：按 Manifest 输出端口顺序返回 Item 数组和可选 Artifact。
- `failed`：返回稳定错误码、可读消息、`retryable` 和非敏感详情。
- `suspended`：返回 Resume Kind、允许的输出端口、Payload Schema、可选到期时间和 Checkpoint Payload。

平台拥有 Retry、Timeout、Cancel、Lease 和恢复状态机。节点服务不得在返回 `failed` 后自行重试，也不得在返回 `suspended` 后占用 Worker；外部恢复只能经过 Trigger Gateway 的 opaque Resume URL。

协议、认证和请求格式错误使用 HTTP `4xx` 加 `NodeProtocolError`。瞬时服务故障使用 `5xx`。业务失败仍使用 HTTP `200` 的 `failed` 结果，以便平台保存结构化错误和按 Manifest/Workflow 策略决定是否重试。不可逆节点默认不自动重试，Fork 时必须确认执行、复用旧输出或 Dry Run。

## 6. Provider 与 Lifecycle

Provider 请求只接收当前参数和短时 Credential Handle，返回稳定的 label/value、非敏感 metadata 和可选分页 cursor。Provider 不得修改 Workflow 或运行状态。

Lifecycle Path 必须与请求中的 `operation` 相同。activate/deactivate 应幂等；poll/webhook 的游标或服务侧状态放在 `state` 中；suspend/resume 只处理节点服务自身状态，Execution 恢复仍由 Coordinator 决定。

所有公共 Provider Endpoint 必须是 HTTPS，并统一走 `agentx-egress-gateway`；只有集群内受管 Fixture 可以继续使用 HTTP。Model 连接测试、MCP 发现/调试、Memory/RAG 健康检查和真实 Workflow 执行共享同一 Endpoint 拼接、TLS 与 Gateway 路径。HTTP 30x 每一跳都会重新校验并签发目标绑定 CONNECT Token，跨 Host 不转发 Authorization/Cookie。Provider 服务不能要求访问私网、Kubernetes Service、Metadata 或非 Profile 允许端口；企业私网接入应使用后续专属 Connector/VPN/PrivateLink。

## 7. 本地验证

生成协议产物：

```powershell
cargo run -p echo-node -- openapi openapi/node-api.json
cargo run -p echo-node -- schemas schemas
```

运行参考服务一致性测试：

```powershell
cargo test -p echo-node
```

完整 `uv run --frozen agentx-check` 会校验 Node OpenAPI、JSON Schema、Rust、Web、Python、Helm/Kustomize 渲染和契约漂移，并在不一致时失败。`services/echo-node/fixtures` 中的请求是语言无关的最小正反样例；接入实现应先对这些 Fixture 做反序列化、认证、Deadline、幂等和结果 Tag 测试，再进入 Kubernetes E2E。

## 8. Manifest 参数与语义输出矩阵

Manifest 是字段名称、动态值能力和输出 Schema 的唯一来源。Adapter 不得读取 Manifest 未声明的别名，也不得把声明参数仅作为透传 metadata。当前内置契约如下：

| 节点类别 | Manifest 参数的执行消费者 | 稳定输出 |
|---|---|---|
| Model | Worker 将 `prompt` 组装为 system message、`userQuestion` 组装为 user message | `AiResponse` |
| Agent | Agent Loop 消费 `systemPrompt/userQuestion` 及全部模型、工具、Token、成本、时长和限额策略 | `AiResponse` |
| MCP Tool | MCP Adapter 使用解析后的 `arguments` 作为 `tools/call.params.arguments` | `text/structuredOutput/files` |
| Skill | Runtime Resource Adapter 固定并读取 Skill Object Closure | `text/structuredOutput/files` |
| RAG / Memory | Resource Adapter 消费 `operation/input` 并映射到固定版本 Endpoint | RAG 为 `text/documents/citations/recordIds`；Memory 为 `text/records/recordIds` |
| Declarative HTTP | Egress Adapter 使用 `method/url/headers/body` 构建实际请求 | `statusCode/headers/body/files` |
| Remote Action | Node Protocol Adapter 消费 `endpoint` 和 ResolvedParameters | `text/structuredOutput/files` |
| Code | Sandbox Manager 消费 `runner/source/arguments/networkPolicy`；当前未实现文件收集和 Credential 文件挂载，因此 Manifest 不声明 `outputPaths/credentialFiles` | `stdout/stderr/exitCode/structuredOutput/files/partial` |
| Approval | Suspension Adapter 消费 title、description、candidateUserId、timeoutMs、timeoutAt，并把节点输入保存为审批 input | `taskId/decision/reason/decidedBy/input` |
| Wait | Suspension Adapter 消费 kind、durationMs、resumeAt、timeoutAt、authenticationMode；恢复时验证并返回语义 Payload | `status/payload/resumedAt` |
| Set / Flow | Builtin Adapter 消费赋值、条件、分支、合并和错误策略 | 直接 Item 字段 |
| Data Builtins | Builtin Adapter 消费过滤、限制、排序、去重、拆分、聚合、重命名、JSON、生成、日期、Base64、Hash、比较和 Schema 校验参数 | 直接 Item 字段或 Manifest 声明的分支端口 |
| Sub-workflow | Bundle Builder 用固定 Workflow Version 的 Start/End Contract 生成版本地址 Manifest | 直接继承子 Workflow End Schema |

数据节点产生的动态 Item 字段若要进入非字符串目标，必须先通过 Output Projection 声明具名字段及具体 Schema；字符串目标可由编译器冻结确定性的文本转换。Projection 应使用新字段名，避免覆盖节点已有的 Item 字段。

Runtime Call Trace 保存递归脱敏后的解析参数、请求与 Provider 响应预览；Authorization、Cookie、API Key、Token、Secret 和 Credential 字段不得明文进入预览。原始 Provider 响应超过 16 KiB 时写入 `runtime_calls.response_artifact_id` 指向的执行级 Artifact，Trace span 只保留 Artifact 引用；Artifact 下载继续经过执行查询权限校验。普通输出与 Reference Picker 只读取上述稳定 Schema。

`files` 的数组元素固定为 `ArtifactRef`：`artifactId/fileName/contentType/sizeBytes/sha256` 全部必填，`artifactId` 为 UUID，`sizeBytes` 非负，`sha256` 为 64 位十六进制摘要。`citations` 的数组元素固定为 `Citation`：必填 `sourceId/text/metadata`，可选 `title/uri/recordId`。两种对象都拒绝额外字段，不能再用任意 Object 延迟契约错误。

Studio 对 Model/Agent 的 `text` 标记为推荐引用。开发期已删除的 `message/messages/toolCalls/iterations/artifacts/providerRawResponse` 不提供别名；Compiler 遇到这些字段时明确要求重新选择稳定的 `text` 字段。
