# Node 服务接入

本文描述Workflow Definition 8.0与plan5实施后的节点接入边界。Rust Registry是内置节点的权威来源；生成Schema、Studio Catalog Fixture与运行契约由漂移测试约束。

## 1. 接入边界

Agentx M4 不发布 Rust、JavaScript、Python 或其他语言的公共 Node SDK。外部节点服务只依赖版本化 HTTP 契约，因此可以使用任意语言实现；平台内部的 Rust Runner 不是扩展接口。

接入时使用以下仓库产物：

- `schemas/runtime-v1/NodeManifestVersion.schema.json`：不可变 Node Manifest Version。
- `schemas/runtime-v1/WorkflowDefinition.schema.json`：Definition `8.0` 契约。
- `services/echo-node`：具备认证、协议校验和一致性测试的 Provider 参考服务。

Node Protocol 当前版本为 `2.0`（Provider 调用族）。plan5 已整条废弃远程节点执行协议：`remote_action` 节点、`POST /agentx/node/v1/actions/execute`、`POST /agentx/node/v1/lifecycle/{operation}`、`node-action-request/result` Schema 与 `openapi/node-api.json` 均已删除，触发内部 Workflow 由 `sub_workflow` 承载，第三方节点生态位未来归 MCP（`mcp_tool`）。Provider 请求版本必须与 Node Manifest 的 `protocolVersion` 完全匹配；平台不会把未知版本降级或猜测转换。

### 1.1 Manifest UI 画布契约

`uiSchema.canvas.role` 是 Studio 执行节点的唯一视觉角色来源，允许值为 `default`、`trigger`、`branch`、`flow`、`merge`、`loop`、`suspend`、`approval`、`sub_workflow`、`agent` 和 `code`（`error_handler` 角色已随节点删除）。内置节点必须在 Rust Registry 中显式写入该字段；Catalog 反序列化时拒绝未知角色，缺少字段的 Manifest 仅由前端回退为 `default`，不得再按 `nodeType` 推断形状。Workflow Definition 8.0 不发布 Trigger 节点，`trigger` role 只保留为 Node Protocol 枚举值，固定 Start 使用独立 Boundary 组件。

该字段只控制编辑器外观，不改变端口、执行能力或 Runtime 语义，并进入现有 Manifest Hash。Model、MCP Tool、Memory、RAG 和 Skill 等资源附件是 Agent/资源节点上的内嵌槽位（`resourceReferences[].bindingRole`），不再是画布节点，也不进入角色枚举。已创建的 Execution Snapshot 保留固化 Manifest 和 Hash；Catalog Reconcile 后的新执行使用新的 Manifest Hash。

Manifest 可声明 `localizations.zh-CN/en-US`，覆盖 `displayName`、`description`、`keywords`、输入端口、输出端口和 Binding Slot 的展示名。本地化 Map 的 Key 必须引用真实协议端口或 Slot；Catalog 拒绝未知语言和悬空引用。本地化数据进入 Manifest Hash，端口和 Slot 的协议 ID 始终使用英文且不随界面语言变化。

Workflow Definition 8.0 Manifest 还必须声明 `outputSchema`、`outputCardinality`、`selectorCapabilities`、`contextReadCapability`、`contextWriteCapability` 和 `artifactOutputSchema`。可绑定参数在Parameter Schema中使用`x-agentx-binding.acceptedKinds`声明允许的`literal/reference/template/array/object`、命名空间和基数；未声明的字段不会自动打开Reference Picker。公共输出投影已删除，字段重命名、筛选和结果构造由显式Set或Code节点完成。

编译器把 Manifest 输出 Schema 与节点实例配置、Projection 合成为按端口冻结的 Effective Output Contract 并写入 IR。Worker 不在提交结果时重新查询 Registry；内置节点与固定版本Sub-workflow均按发布时同一契约验证。MCP、Skill、Knowledge和Memory只作为Agent资源能力进入冻结附件Registry，不是独立Workflow节点。Model 与 Agent 标准输出固定为 `text`、`reasoningContent`、`structuredOutput`、`citations`、`files`、`usage`、`finishReason` 和 `partial`；Provider message、工具调用、Agent 迭代与原始响应仅进入受权限控制的 Trace/Artifact。

错误处理不再是节点：每个节点都保留 Error 输出端口，**接线即失败分支**——节点存在 `error` 出边时，失败产生 Error Item 走该分支；未接线则按完成模式终止执行。Definition 8.0 已删除 `settings.onError`、`end.error.strategy` 与 `collectWindowMs`。

## 2. Endpoint

Node 服务实现一个版本化调用族：

| Endpoint | 用途 | 调用方 |
|---|---|---|
| `POST /agentx/node/v1/providers/{provider}/invoke` | 动态选项、搜索和字段映射 | Platform API/Studio Adapter |

Provider契约继续用于动态选项、搜索、资源映射和凭据测试；对应专用UI随plan5节点切片接通。生产Trigger/Poll/Webhook属于应用触发与Trigger Gateway链路，不恢复已经删除的Node Lifecycle协议。节点服务不得调用内部gRPC、直接写MySQL、创建Attempt或推进Execution。

## 3. 认证与身份

生产部署必须通过 TLS，并使用调用方绑定的 Bearer Token 或部署环境提供的等价工作负载身份。参考 `echo-node` 读取 `AGENTX_NODE_PROVIDER_AUTH_TOKEN`。认证失败返回 HTTP `401` 和 `NodeProtocolError`。

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

平台拥有 Retry、Timeout、Cancel、Lease 和恢复状态机。

协议、认证和请求格式错误使用 HTTP `4xx` 加 `NodeProtocolError`。瞬时服务故障使用 `5xx`。

## 6. Provider

Provider 请求只接收当前参数和短时 Credential Handle，返回稳定的 label/value、非敏感 metadata 和可选分页 cursor。Provider 不得修改 Workflow 或运行状态。

所有公共 Provider Endpoint 必须是 HTTPS，并统一走 `agentx-egress-gateway`；只有集群内受管 Fixture 可以继续使用 HTTP。Model 连接测试、MCP 发现/调试、Memory/RAG 健康检查和真实 Workflow 执行共享同一 Endpoint 拼接、TLS 与 Gateway 路径。HTTP 30x 每一跳都会重新校验并签发目标绑定 CONNECT Token，跨 Host 不转发 Authorization/Cookie。Provider 服务不能要求访问私网、Kubernetes Service、Metadata 或非 Profile 允许端口；企业私网接入应使用后续专属 Connector/VPN/PrivateLink。

## 7. 本地验证

生成契约产物（Manifest 与 Definition Schema 已迁入 generate-contracts）：

```powershell
cargo run -p agentx-runtime-contracts --bin generate-contracts -- schemas/runtime-v1 openapi/runtime-internal-v1.json openapi/observability-internal-v1.json
cargo run -p agentx-runtime --bin generate-studio-catalog -- apps/web/src/features/workflow-designer/testing/studio-catalog.fixture.json
```

运行参考服务一致性测试：

```powershell
cargo test -p echo-node
```

完整 `cargo xtask check` 会校验JSON Schema、Rust、Web、Python验收、Helm/Kustomize渲染、文件边界和契约漂移，并在不一致时失败。`echo-node`与`echo-mcp`的Rust测试覆盖Provider协议、认证和结构化响应；系统级行为继续由`pytest tests/e2e`编排Kubernetes与Playwright验收。

## 8. Manifest 参数与语义输出矩阵

Manifest 是字段名称、动态值能力和输出 Schema 的唯一来源。Adapter 不得读取 Manifest 未声明的别名，也不得把声明参数仅作为透传 metadata。当前内置契约如下：

| 节点类别 | Manifest 参数的执行消费者 | 稳定输出 |
|---|---|---|
| Model | Worker将`prompt/userQuestion`组装为有序消息；`responseMode=json_schema`把`structuredSchema`作为Provider原生response_format并校验结果，不解析普通文本兜底 | `AiResponse` |
| Agent | Agent Loop消费`systemPrompt/userQuestion`、预算及六槽Model/Workspace Sandbox/Tool/Skill/Knowledge/Long-term Memory冻结资源 | `AiResponse` |
| Declarative HTTP | Egress Adapter 使用 `method/url/query/headers/body`，timeout只读取NodeSettings；Bearer/Basic/API Key/custom_json凭据在调用前注入并从结果、Trace和日志中脱敏 | `statusCode/headers/body/files` |
| Code | Sandbox Manager 消费 `runner/inputs/source/outputExample/networkPolicy`；Compiler从JSON5对象示例推导冻结Schema；Python调用`main(**inputs)`，JavaScript调用`main(inputs)`，Shell只通过`AGENTX_INPUT_PATH/AGENTX_OUTPUT_PATH`交换JSON；根结果必须为对象 | `stdout/stderr/exitCode/structuredOutput/files/partial`，Picker将对象字段直接展示为业务输出 |
| Approval | Suspension Adapter 消费`title/description/candidateUserId/buttons/timeoutMs`；buttons随任务快照持久化，业务decisionId独立保存，统一Decide恢复`decision:{id}` | `taskId/decision/reason/decidedBy/input`，另有固定`timed_out` |
| If | Builtin Adapter 按序求值 `cases[]`（每分支条件组 and/or），首中路由到 `case:{id}`，否则 `else` | 分支端口透传 Item |
| List | 先求值必填数组`input`，再在元素`item`上下文执行filter→结构化sort→takeN | ExactlyOne Item：`{"items":[...]}` |
| Loop Over Items | 状态机消费`input/outputSelector/errorMode/parallelism`；仅激活并行上限内的轮次，检查点保存队列、活跃generation、结果和失败 | ExactlyOne Item：`{"items":[...]}` |
| Set / Merge | Builtin Adapter 消费赋值与合并模式（append / combine_by_position / combine_by_key） | 直接 Item 字段 |
| Sub-workflow | `workflowVersionId/inputs`选择固定版本；Bundle Builder从该版本Start/End/Context生成不可变Manifest，Runtime在创建子执行前求值并校验inputs | 继承子Workflow End Schema；`all_complete`字段为含可选null槽位的数组 |

数据节点产生的动态Item字段若要重命名、筛选或组合，必须显式增加Set或Code。`InputBinding`解析后按目标JSON Schema执行唯一一套严格转换，并将转换诊断写入Trace；不存在公共输出投影或隐式字段声明入口。

Runtime Call Trace 保存递归脱敏后的解析参数、请求与 Provider 响应预览；Authorization、Cookie、API Key、Token、Secret 和 Credential 字段不得明文进入预览。原始 Provider 响应超过 16 KiB 时写入 `runtime_calls.response_artifact_id` 指向的执行级 Artifact，Trace span 只保留 Artifact 引用；Artifact 下载继续经过执行查询权限校验。普通输出与 Reference Picker 只读取上述稳定 Schema。

`files` 的数组元素固定为 `ArtifactRef`：`artifactId/fileName/contentType/sizeBytes/sha256` 全部必填，`artifactId` 为 UUID，`sizeBytes` 非负，`sha256` 为 64 位十六进制摘要。`citations` 的数组元素固定为 `Citation`：必填 `sourceId/text/metadata`，可选 `title/uri/recordId`。两种对象都拒绝额外字段，不能再用任意 Object 延迟契约错误。

Studio 对 Model/Agent 的 `text` 标记为推荐引用。开发期已删除的 `message/messages/toolCalls/iterations/artifacts/providerRawResponse` 不提供别名；Compiler 遇到这些字段时明确要求重新选择稳定的 `text` 字段。
