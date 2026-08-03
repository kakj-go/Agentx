# Node 服务接入

## 1. 接入边界

Agentx M4 不发布 Rust、JavaScript、Python 或其他语言的公共 Node SDK。外部节点服务只依赖版本化 HTTP 契约，因此可以使用任意语言实现；平台内部的 Rust Runner 不是扩展接口。

接入时使用以下仓库产物：

- `openapi/node-api.json`：Action、动态 Provider 和 Lifecycle OpenAPI 3.1 契约。
- `schemas/node-manifest.schema.json`：不可变 Node Manifest Version。
- `schemas/node-action-request.schema.json` 与 `schemas/node-action-result.schema.json`：Action 请求和三类结果。
- `schemas/workflow-definition.schema.json`：Definition `2.0` 契约。
- `services/echo-node`：具备认证、协议校验和一致性测试的参考服务。

Node Protocol 当前版本为 `1.0`。Node Manifest 的 `protocolVersion`、Action/Provider/Lifecycle 请求版本必须完全匹配；平台不会把未知版本降级或猜测转换。Node Type Version 由 Workflow Version 固定，已发布版本不得原地修改。

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

完整 `scripts/check.ps1` 会重新生成 Node OpenAPI 和四份 JSON Schema，并在任何字节漂移时失败。`services/echo-node/fixtures` 中的请求是语言无关的最小正反样例；接入实现应先对这些 Fixture 做反序列化、认证、Deadline、幂等和结果 Tag 测试，再进入 Kubernetes E2E。
