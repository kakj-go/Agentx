# Plan3 破坏性契约草案

本文件定义 P3-00 必须冻结的目标语义。字段名可以在 Schema 评审中调整，但不得改变 [固定架构决策](00-baseline-and-decisions.md)。所有旧契约直接删除，不设计双读或 Migration。

## 1. 契约版本

| 契约 | 目标版本 | 破坏性变化 |
|---|---|---|
| Workflow Definition | 6.0 | Agent 内置 Model、可选 Workspace Sandbox、四类 Attachment、Session/Core Policy 和旧参数删除 |
| Node Manifest | 2.0 | Resource Slot 增加 Inspector/Canvas 展示语义 |
| Execution Spec Bundle | 2.0 | 固定 Agent Core Profile、Model、工具、Context 和 Session Policy |
| Worker Task | 2.0 | Agent Attempt 携带 Session/Operation/Fencing 元数据 |
| Agent Core Contract | 1.1 | Run Input、External Context、Attachment Registry、Entry、Operation State、Effect、Tool 和 Event 的 Rust/Schema 契约 |
| Trace/Event | 2.0 | Agent Message/Turn/Tool/Compaction/Recovery 强类型事件 |

不再存在 `Pi Agent Runtime Protocol`、Rust↔TypeScript 双向流或远程 `PiAgentRunSpec`。Agent Core Contract 是 Worker 与内嵌 Rust crate 的编译期边界；需要持久化的类型仍必须有 JSON Schema 和序列化漂移测试。

## 2. Workflow Definition 6.0

Agent Node 示例：

```json
{
  "id": "agent-1",
  "key": "support_agent",
  "type": "agent",
  "typeVersion": 2,
  "name": "智能体",
  "parameters": {
    "systemPrompt": {
      "kind": "literal",
      "value": "You are a helpful assistant."
    },
    "userQuestion": {
      "kind": "reference",
      "selector": {
        "namespace": "inputs",
        "path": ["question"],
        "run": { "kind": "current" },
        "item": { "kind": "current" }
      },
      "missingPolicy": { "kind": "error" }
    },
    "thinkingLevel": "medium",
    "compaction": {
      "thresholdTokens": 64000,
      "retainedMessages": 32
    },
    "sessionPolicy": {
      "mode": "application_session",
      "retentionPolicyId": "..."
    },
    "budget": {
      "maxTurns": 12,
      "maxToolCalls": 32,
      "maxTotalTokens": 64000,
      "maxOutputTokens": 4096,
      "maxCostMicros": 1000000,
      "maxDurationMs": 300000,
      "limitAction": "error_output"
    }
  },
  "resourceReferences": [
    {
      "bindingRole": "model",
      "resourceType": "model",
      "resourceId": "...",
      "operation": "use"
    },
    {
      "bindingRole": "workspace_sandbox",
      "resourceType": "sandbox_profile",
      "resourceId": "...",
      "operation": "use"
    },
    {
      "bindingId": "mcp-weather",
      "bindingRole": "mcp",
      "resourceType": "mcp_tool",
      "resourceId": "...",
      "operation": "use"
    }
  ],
  "outputProjection": {},
  "contextWrites": [],
  "settings": {}
}
```

不变量：

- `model` 恰好一个、`bindingId` 为空、`operation=use`。
- `workspace_sandbox` 最多一个、`bindingId` 为空、`operation=use`；不配置隐藏默认值。
- `workspace_sandbox` 缺失时 Bundle 的 Core Tool Registry 必须排除 `read/write/edit/bash`；其存在是启用四工具的唯一 Agent 级条件。
- `sessionPolicy.mode` 恰好为 `application_session` 或 `invocation`；Runtime 不允许 `auto`、布尔兼容字段或隐式退化。
- `application_session` 缺少可信 Application Session ID 时返回 `AGENT_SESSION_REQUIRED`；`invocation` 不读取旧 Agent Session。
- 单次 Run 的 Agent Context 和自动 Compaction 是 Agent Core 内置能力，不以 Attachment 或 Workflow Node 表达。
- `mcp` 允许多个 `mcp_tool/use`；`skill` 允许多个 `skill/use`；`knowledge` 允许多个 `rag/read`。
- `memory` 最多一个，读写操作必须显式，不能让 `write` 隐式包含 `read`。
- 内置 Reference 不出现在 Editor Binding Layout；外挂 Reference 必须有稳定 `bindingId` 和 Binding Edge。
- 删除旧 `ai_model`、`ai_tool`、`ai_memory`、`ai_retriever` 和 `ai_skill` Role。

## 3. Node Manifest 2.0

Resource Slot 增加展示与运行语义：

```json
{
  "name": "model",
  "resourceType": "model",
  "required": true,
  "multiple": false,
  "placement": "inspector",
  "operation": "use"
}
```

固定 Slot：

| Slot | Resource | Cardinality | Placement |
|---|---|---:|---|
| model | model | 1 | inspector |
| workspace_sandbox | sandbox_profile | 0..1 | inspector |
| mcp | mcp_tool | 0..N | canvas |
| skill | skill | 0..N | canvas |
| memory | memory | 0..1 | canvas |
| knowledge | rag | 0..N | canvas |

Studio、Compiler 和 Publisher 必须读取同一 Manifest。前端不能按 `nodeType === agent` 私自隐藏 Model Port；`placement` 是统一行为来源。

## 4. Editor Document

- Inspector Slot 不创建 `editorKind=binding` Node、Binding Edge 或布局记录。
- Canvas Slot 继续使用 Attachment Node 和无箭头虚线 Binding Edge。
- Serializer 从 Action Node 的全部 `resourceReferences` 生成 Definition，只为有 `bindingId` 的引用生成 Editor Binding Layout。
- Deserialize 不得为 Model/Workspace Sandbox 引用伪造 Attachment。
- Agent Node 卡片的 Model/Sandbox/Core 摘要来自 Definition 与资源查询，不形成第二份持久状态。

## 5. Execution Spec Bundle 2.0

每个 Compiled Agent 固化：

```text
node id / stable node key / agent definition hash
agent core contract version / reference behavior commit / adapter version
resolved system prompt and DynamicValue IR
context projection / compaction / retry / budget policy
exact model binding + price + credential reference
optional exact Agent Workspace Sandbox Profile
resolved core tool registry: no workspace sandbox = empty; selected = read/write/edit/bash
ordered MCP bindings and schemas
for stdio MCP: exact server version + runtime sandbox profile version + process contract
ordered Skill versions, objects and recursive dependencies
ordered Knowledge bindings
optional Memory binding and allowed operations
session and retention policy
grant/policy epoch evidence
```

Bundle 不保存用户会话消息、长期记忆结果、Credential 明文、本地路径或参考 Pi 的 npm/JSONL 配置。

### 5.1 Agent Attachment Registry V1

P3-04 起每个 Agent Bundle Entry 必须包含规范化的 `attachmentRegistry`：

```text
contexts[]
  contextId / origin / exact resource + version / signed object + content hash
tools[]
  name / description / input schema / origin / replay policy / exact resource + version / operation
authorizationEvidence[]
  resource type / exact resource + version / operation / policy epoch / grant ids
```

Registry 名称在发布阶段全局判重，不在运行时重命名。MCP 保留发现时冻结的名称；Skill 资产统一为 `skill_resource`；Knowledge 使用 `knowledge_search_<resource_uuid_without_hyphens>`；Memory 按授权暴露 `memory_recall` 和/或 `memory_write`。未绑定、缺少传递依赖、版本不一致或未授权能力不会进入 Registry。

Skill 指令按 Binding、Resource ID、Version ID 确定性排序，在首轮 Model Effect 前作为带来源的 External Context 注入且不重复累积。Knowledge 与 Memory 返回值始终是不可信 Tool Result；Knowledge Citation 固定包含 Resource、Document、Chunk、Title、URI 和 Score。P3-04 Memory Scope 仅为不可伪造的 Agent Run Scope，不跨 Execution 自动召回。

## 6. MCP Server Version 与 stdio Sandbox

MCP transport 使用封闭枚举：

```json
{
  "transport": {
    "kind": "stdio",
    "command": "/opt/mcp/bin/server",
    "args": ["--stdio"],
    "environmentCredentialRefs": []
  },
  "runtimeSandbox": {
    "resourceType": "sandbox_profile",
    "resourceId": "uuid",
    "resourceVersionId": "uuid",
    "operation": "use"
  }
}
```

不变量：

- `stdio` 创建/更新时 `runtimeSandbox` 恰好一个；缺失返回 `MCP_STDIO_SANDBOX_REQUIRED`。
- Streamable HTTP/SSE 不使用 `runtimeSandbox`，继续固化 Endpoint/Egress/Credential。
- command 与 args 分离，禁止 Shell 字符串、动态包安装和宿主可执行文件；镜像由 Sandbox Profile Version 固定。
- `initialize`、`tools/list`、Health、Debug 和 Workflow Runtime 都使用相同的 Sandbox Process Session Contract。
- Tool Version 由 Server 自动发现并继承 `serverVersionId + runtimeSandboxProfileVersionId`，不允许逐 Tool 覆盖。
- Grant/Bundle 闭包为 `MCP Tool → MCP Server Version → Sandbox Profile Version → Credential`；任何 Grant 不隐式传递。
- 同一 Agent Run + MCP Server Version 最多一个活跃 Process Session；多个 Tool Call 复用该进程，跨 Server 不复用。

## 7. AgentRunInputV1

Worker 在进程内构造并传给 Agent Core：

```json
{
  "apiVersion": 1,
  "runId": "uuid",
  "executionId": "uuid",
  "nodeExecutionId": "uuid",
  "attemptId": "uuid",
  "fencingToken": 3,
  "deadlineAt": "RFC3339",
  "agentDefinitionHash": "sha256:...",
  "coreContractVersion": "1.1",
  "model": {
    "resourceId": "uuid",
    "resourceVersionId": "uuid",
    "modelName": "snapshot-name",
    "contextWindow": 200000
  },
  "session": {
    "mode": "application_session",
    "sessionId": "uuid",
    "stateVersion": 7,
    "lane": "main",
    "leafEntryId": "uuid-or-null",
    "openOperationId": "uuid-or-null"
  },
  "prompt": {
    "system": "resolved text",
    "user": "resolved text"
  },
  "workspace": {
    "sandboxBinding": "uuid-or-null",
    "coreTools": []
  },
  "tools": [],
  "externalContexts": [],
  "compaction": {},
  "budget": {}
}
```

限制：

- 不包含 Provider API Key、Vault Token、数据库 DSN、OSS Credential、Sandbox Endpoint 或宿主路径。
- Model/Tool/State 依赖通过 Worker 提供的 trait object/adapter 注入，不作为可序列化 Secret 塞入 Core State。
- Input Hash 进入 Agent Run Ledger 和 Trace；恢复时必须验证 Definition、Model、Tool Registry 与开放 Operation 捕获的版本一致。
- `sandboxBinding=null` 时 `coreTools` 必须为空；有 Agent Workspace Sandbox 时必须解析为冻结的四工具集合。
- stdio MCP 的 Sandbox Binding 只存在于对应 Tool Adapter/Bundle Dependency，不写入 `workspace.sandboxBinding`。

## 8. Agent Session State V1

### 8.1 Entry

`AgentSessionEntryV1` 是 append-only，至少支持：

- `message.user`
- `message.assistant`
- `message.tool_result`
- `compaction`
- `custom.external_context`

每个 Entry 至少包含 `entryId + sessionId + lane + parentEntryId + kind + payloadRef/inlinePayload + createdAt`。Assistant Tool Call 与对应 Tool Result 使用稳定 ID 关联。

### 8.2 Register

可变 Register 至少包含：

- `lane.state/main`：leaf、开放 Operation、最近终态。
- `operation.meta/<operationId>`：Run 意图、起点、Attempt/Fencing、捕获的 Definition/Adapter 版本。
- `operation.state/<operationId>`：完整当前 Operation State。
- `operation.effect/<effectId>`：大参数或结果的 Object Reference。
- `pending.entry/<entryId>`：steer/follow-up 尚未放入 Entry Tree 的内容。

Terminal Transaction 必须删除开放 Operation 的 Register，并原子更新 Lane Last Result 和 Current Operation。

### 8.3 Usage

Usage 是 append-only，至少包含：`usageId + operationId + entry/effectId + kind + provider/model + input/output/cache tokens + cost + createdAt`。Compaction 的模型调用独立入账。

## 9. Complete Operation State V1

Operation State 必须是带版本的封闭枚举，能仅凭当前值决定下一动作。最小状态图：

```text
accepted
  → checkpoint(needs_assistant)
  → model_ready
  → model_effect_pending(intent + reserved response/usage ids)
  → model_settled(response + usage)
       ├─ tool_batch_planned
       │    → tool_effect_pending
       │    → tool_settled
       │    → checkpoint(needs_assistant)
       ├─ compaction_planned
       │    → compaction_effect_pending
       │    → compaction_settled
       │    → checkpoint(needs_assistant)
       └─ terminal
```

每个状态都包含 owner operation、control status、continuation、预算快照和该阶段恢复所需的完整数据。禁止用“某行不存在”“Trace 最后一条事件”或进程内 Future 推断状态。

## 10. Agent Core Effect Contract

Agent Core 只请求以下逻辑 Effect：

| Kind | Worker Adapter | Ledger Kind | 默认 Replay |
|---|---|---|---|
| `model.stream` | Model Runtime Adapter | model | ledger-dependent |
| `tool.read` | OpenSandbox Adapter | sandbox | safe |
| `tool.write/edit/bash` | OpenSandbox Adapter | sandbox | never |
| `mcp.call` | MCP Runtime Adapter | mcp_tool | binding policy |
| `knowledge.search` | RAG Runtime Adapter | rag | safe |
| `memory.search` | Memory Runtime Adapter | memory | safe |
| `memory.add/update/delete` | Memory Runtime Adapter | memory | never/idempotency-required |

幂等键：

```text
attempt_id + agent_run_id + operation_id + effect_id + effect_kind
```

执行规则：

1. Worker 先原子提交 Effect Intent、稳定 ID 和下一 Operation State。
2. 验证 Lease/Fencing/Deadline/Grant 后执行外部 Effect。
3. 原子提交结果 Entry/Object Ref、Usage 和下一完整 Operation State。
4. 恢复发现 `effect_pending` 时按 Replay Policy 和 Runtime Call Ledger 决定查询、重放或生成 Unknown/Interrupted 结果。

stdio MCP Adapter 还必须用稳定的 Server Session ID 管理 `process.start → initialize → call → process.stop`。`ProcessSessionWriteRequestV1` 必须携带冻结的 `safe | idempotency_required | never` Replay Policy；Ledger 按该策略记录副作用等级，但任何状态为 Sent 的 Frame 都必须先通过 Process `read` 按 JSON-RPC ID 对账，禁止通用 HTTP 恢复路径直接重发。Process stdout 只接受有界 MCP JSON-RPC Frame，stderr 不得混入协议；Worker/Sandbox 强退时通过 Lease 对账并将飞行中的不可判定 Tool Call 标为 Outcome Unknown。

Attachment Authorization Evidence 不能复制 Execution 的全部 Grant ID。`RuntimeAuthorizationSnapshotV1.grantBindings` 必须逐项固化 `grant_id + resource_type + resource_id + optional exact version + operation`；Bundle Builder 为直接和传递资源只选择精确匹配的 Grant，缺少任一 Tool、Server、Sandbox 或 Credential Grant 时发布失败。运行时同时校验 Evidence、Snapshot Binding 和当前 Runtime Projection。

## 11. Core Adapter Traits

P3-00 必须冻结以下逻辑接口，具体 Rust 命名可调整：

```text
AgentModelEffect      stream(request, effect_context) -> event stream + settled response
AgentToolEffect       execute(tool, args, effect_context) -> progress + result
AgentStateStore       load / commit(transaction, expected_version, fencing_token)
AgentEventSink        publish(core event)
AgentClock            now / deadline
AgentBudget           admit / charge / terminal decision
```

Core crate 的 Cargo 边界测试必须证明这些 trait 是唯一基础设施出口。

## 12. 并发、CAS 与恢复

- 一个持久 Agent Session + stable Node Key 同时最多一个开放 Operation。
- 接受 Run 时 CAS `state_version`，写 User Entry、Operation Meta、初始完整 State 和 Session Busy。
- 每次 State Commit 都带当前 Attempt Fencing Token 和 Expected State Version。
- 新输入按 Policy 进入 durable queue 或返回 `AGENT_SESSION_BUSY`，不得并发驱动两个写 Session 的 Core。
- `invocation` 仍创建可审计的临时 Session State，但后续 Execution 不加载。
- 进程重启后只从 Entry/Register/Usage 与 Runtime Call Ledger 恢复，不依赖 Worker 本地内存。

## 13. 错误码

至少冻结：

- `AGENT_MODEL_REQUIRED`
- `AGENT_CORE_TOOLS_REQUIRE_WORKSPACE_SANDBOX`
- `AGENT_RESOURCE_SLOT_INVALID`
- `AGENT_CORE_STATE_INVALID`
- `AGENT_CORE_VERSION_MISMATCH`
- `AGENT_SESSION_BUSY`
- `AGENT_SESSION_REQUIRED`
- `AGENT_CONTEXT_COMPACTION_FAILED`
- `AGENT_CONTEXT_OVERFLOW`
- `AGENT_TOOL_NOT_AUTHORIZED`
- `AGENT_EFFECT_OUTCOME_UNKNOWN`
- `AGENT_WORKSPACE_PATH_DENIED`
- `AGENT_LONG_TERM_MEMORY_SUBJECT_REQUIRED`
- `MCP_STDIO_SANDBOX_REQUIRED`
- `MCP_STDIO_PROCESS_UNAVAILABLE`
- `MCP_STDIO_PROTOCOL_INVALID`

错误必须声明 retryable、稳定公开 message、受权 details 和 Trace 映射；不得把 Core/Provider/Sandbox 原始异常直接返回用户。

## 14. Trace Contract

固定层级：

```text
Execution
  → Node
    → Attempt
      → Agent Run
        → Operation / Turn
          → Model Runtime Call
          → Tool Runtime Call
        → Compaction Model Call
        → State Transition / Recovery Event
```

流式 delta 不为每个 Token 创建 ClickHouse Span。Message End、Turn End、Tool End、Compaction、Effect Unknown、Recovery 和 Terminal 使用稳定 Span/Event。Prompt、Memory、Knowledge 和文件内容继续按现有脱敏和 Artifact 预算处理。
