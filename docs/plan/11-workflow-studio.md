# 阶段 11：Agentx Workflow Studio（M6）

状态：`done`。最终验收证据见 [M6 Workflow Studio 验收证据](m6-acceptance-evidence.md)。

## 1. 目标与完成边界

M6 交付一个可实际开发 Workflow 的桌面端 Studio：用户能够搜索和拖入 Agentx 节点、连接端口、通过右侧检查器配置参数与资源、保存草稿、调试整个或部分流程、查看节点输入输出与 Trace，并创建 Version 和发布 Deployment。

M6 对齐 n8n 的交互模型，不兼容 n8n 的实现协议。以下能力明确不进入 M6 或 M7：

- n8n Workflow JSON 导入导出和字段兼容。
- n8n/community npm 节点、Credential Definition 和插件运行时。
- n8n JavaScript 表达式语法、`$node` 等上下文兼容。
- 插件市场、移动端画布、实时多人协作和像素级复刻。

M6 完成后，Studio 本身必须是完整闭环；M7 不再补画布、节点表单、草稿调试或发布 UI，只消费 M6 冻结的契约完成外围入口和生产发布。

### 1.1 n8n 风格画布重构（Agentx 原生）

画布采用 n8n 的紧凑节点语言和工作区节奏，但不复制其源码、许可证实现或 JSON 协议。Catalog Manifest 的 `uiSchema.canvas.role` 是唯一视觉角色来源；缺失角色的旧 Manifest 仅在前端回退到 `default`，未知角色由 Catalog 校验拒绝。普通/Flow 节点以图标为主并在下方显示最多两行标签，Trigger、Branch、Merge、Loop、Suspend、Approval、Sub-workflow、Agent、Code 和 Error Handler 使用受控内部形状，AI/Binding 端口使用菱形提示。Manifest `localizations` 统一驱动画布、节点栏、搜索、详情和 Tooltip 的中英文显示。

Workflow Definition 的 `primaryOutputNodeId` 固化 Application/Playground 主要输出。普通调试和版本化允许多个正常终点；Application Deployment 对零终点和未指定的多终点执行门禁。Runtime 按 Activation 元组稳定选取主要节点的最后一次成功输出，绝不依赖画布坐标或墙钟时间。

左侧 Node Creator 默认折叠为 56px 工具栏，搜索或从输出端口打开 320px 覆盖创建器，支持自动聚焦、Manifest 分类、键盘导航、拖拽和合法连接过滤。无节点画布只提供 Add Trigger/Search Nodes。右侧 Node Details 为 480px 全高视图，固定 Parameters、Input、Output、Trace 四个 Tab；单节点运行仍复用 `single_node` Debug Plan。底部 Runtime Panel 是 40px 折叠、默认 220px 展开的全局 Execution Rail，可调整到 160px–65vh，节点结果不在画布和轨道重复复制。

Sticky Note（默认 240×160，最小 150×80）和非嵌套 Group（`collapsed=false`）只写入 Editor Document。便签支持双击编辑、语义色和缩放；Group 可移动成员、折叠为 240×64 代理并临时聚合外部连线，删除仅解除分组。Undo/Redo Snapshot 同时包含 nodes、edges、viewport、annotations 和 groups；`canvasNodeMetrics` 是渲染、ELK、对齐、粘贴和 Minimap 的唯一尺寸来源。

## 2. 实施前基线与必须修正的问题

- 当前 `workflow-canvas.tsx` 是单文件最小画布，只硬编码 `manual_trigger`、`model`、`mcp_tool`、`skill`、`rag` 和 `memory`，运行按钮仍禁用。
- Rust `NodeRegistry` 已有 Agent、Code 和基础控制节点 Manifest，但 `node_definitions` 表没有可供 Studio 使用的完整查询 API，前端与编译器尚未共享同一 Catalog。
- Draft 已有 `expectedRevision` 乐观并发和不可变 Draft Revision，Version 执行 API 也已存在。
- 现有 `ExecutionRuntime` 和 gRPC 请求只接受 `workflow_version_id`；`/workflows/{id}/run` 仍是 `RUNTIME_UNAVAILABLE`，不能直接运行某一草稿修订。
- `WorkflowDefinition 2.0` 把节点位置放进运行定义，运行编译还从位置推导分支顺序，编辑布局和执行语义没有真正解耦。
- Pin/Mock、运行高亮和节点结果若继续放进 Draft Definition，会污染 Version、Content Hash 和发布校验。
- M5 复核发现 Windows `scripts/check.ps1` 仍按原始换行比较 OpenAPI，且 JUnit 可被无环境运行覆盖；开始 STU-001 前必须改为结构化/规范化比较，并按 `stage/run-id` 隔离证据目录。

因此 M6 不是纯前端任务。必须先完成一次受控的编辑态/执行态契约重构，再建设 Studio；不新增运行引擎、队列或后端服务。

## 3. M6 架构决策

### 3.1 三类数据严格分离

| 数据 | 权威位置 | 内容 | 是否进入生产 Version/IR |
|---|---|---|---|
| Workflow Definition | Draft Revision、Workflow Version | 节点类型/版本、参数、资源引用、端口连接、显式连接顺序、执行设置 | 是 |
| Editor Document | Draft Revision、Workflow Version 的独立字段 | 节点坐标、视口、注释、分组、折叠状态等纯编辑信息 | 否；仅用于再次查看和编辑 |
| Debug Overlay | 独立调试表和 Artifact | Pin Data、Mock、临时输入、选中的历史输出 | 否；只在手动调试快照中引用 |

引入 `WorkflowDefinition 3.0`，删除 Node 中的 `position`，Connection 增加稳定 `order`。默认 `deterministic` 执行顺序由端口和 Connection Order 决定，编译器不再读取画布坐标；`parallel` 仍是显式设置。项目尚未发布，不保留 2.0 双读逻辑，Migration/Fixture/Schema 一次性升级。

Draft 保存请求原子提交 `definition + editorDocument + expectedRevision`。Revision 的 Content Hash 分为 `definitionHash` 和 `editorHash`；只有 Definition 变化影响编译和发布 Diff。Version 保留 Editor Document 快照用于只读查看，但 Worker 永远不读取它。

### 3.2 Node Catalog 是单一事实来源

新增共享 Node Catalog 边界：

1. 内置 Manifest 从共享 Rust crate 注册，并通过幂等 Reconcile 写入 `node_definitions/node_definition_versions`。
2. Platform API 提供分页/搜索的 Catalog 和精确版本详情；Studio 不再维护 `NodeKind` 白名单或参数 Schema 副本。
3. Compiler 按 `nodeType + typeVersion` 从同一 Catalog 构建 Registry，并将实际 Manifest Hash 固化到编译/执行快照。
4. Manifest 补齐 `category`、`descriptionKey`、受控 `iconKey`、`keywords`、端口、Parameter Schema、UI Schema、Provider、Credential、Sandbox Requirement、Mock 和副作用能力。
5. 前端只实现 Agentx 约定的 UI Schema 控件集合；未知控件显示明确“不支持”，不能降级为错误表单并保存。

Provider 请求由 Platform API 代理并执行权限、Credential Handle、超时和脱敏；浏览器不直连节点服务。

### 3.3 Draft Revision 调试快照

运行入口统一为 `ExecutionSource`：

```text
ExecutionSource
  - Version(versionId)                  生产、Application、Evaluation
  - DraftRevision(workflowId, revision) Studio 手动调试
```

Coordinator 在创建 Execution 的同一事务中解析来源并写入不可变 `execution_snapshots`：Definition、Compiled IR、Manifest Hash、资源版本、授权结果、Debug Plan 和 Overlay Hash。创建后 Worker 只读取 Execution Snapshot，不读取可变 Draft。

为此需要同步调整 `ExecutionRuntime`、runtime gRPC、Repository 和运行上下文：

- `workflow_version_id` 对 Draft Debug 可空，Execution 必须保存 `source_kind/source_id/source_revision`。
- Node/Agent/Sandbox 上下文继续使用必填 `execution_id` 作为 Execution Snapshot 和凭证作用域，`workflow_version_id` 改为可选业务关联；不能伪造临时 Version ID。
- Version 执行保持现有语义；Application、Webhook、Schedule、Evaluation 只能提交 Version Source。
- Draft Debug 每次运行都校验当前用户权限和 Workflow Service Identity Grant，并固化当次资源版本与授权证据。
- Autosave 后发起调试必须携带 `expectedRevision`；服务端 Revision 不一致时拒绝运行，不能暗中运行旧草稿。

这是一条新的“执行来源”，不是新的执行引擎。Version 和 Draft Debug 继续共用 Coordinator、Worker、Node Runner、Checkpoint、Trace、取消和恢复状态机。

### 3.4 调试计划与部分执行语义

调试命令只允许四种模式：

| 模式 | 语义 | 缺失输入处理 |
|---|---|---|
| `full` | 从手动 Trigger 运行完整可达图 | 使用请求输入 |
| `to_node` | 运行目标节点所需祖先子图并在目标后停止 | 从 Trigger、Pin 或明确输入开始 |
| `single_node` | 只运行目标节点 | 必须选择 Pin、历史 Node Output、Checkpoint 或手工输入 |
| `from_node` | 从目标节点已有输入/输出继续运行后继子图 | 必须选择可追踪的数据来源 |

Debug Plan 必须记录包含/跳过节点、种子数据来源、目标节点、Side Effect Decision 和 Overlay Hash。缺失上游数据、来源 Schema 不匹配或不可逆节点未确认时返回结构化字段错误。部分执行使用现有 Execution Machine 和 Checkpoint/Fork 能力，不在浏览器拼装伪输出。

### 3.5 前端状态边界

Studio 使用固定工作区布局：左侧 Node Palette、中央无限画布、右侧 Node Inspector、底部可调整高度的 Input/Output/Trace 面板；顶栏承载保存状态、Undo/Redo、运行、版本和发布动作。

状态分层：

- TanStack Query：Draft、Catalog、资源、Execution、Trace 等服务端状态。
- Zustand Editor Store：规范化 nodes/edges、viewport、selection 和本地 dirty 状态。
- History Store：仅保存可逆编辑 Command/Patch，不保存 Query 结果和运行高亮。
- React Hook Form：当前节点参数草稿；提交后形成一个 History Command。
- Runtime Overlay Store：按 `executionId + nodeId + runIndex` 保存短期高亮和选中结果，切换 Execution 时整体替换。

前端目录按 `canvas/nodes/edges/panels/forms/store/model/api/utils` 拆分，任何文件不得超过 2000 行。共享 Button、Input、Select、Tabs、Dialog、Tooltip、Resize Panel 和主题 Token 进入统一组件层，不在 Studio 建第二套视觉系统。

## 4. API 与数据变更

### 4.1 Migration

- `workflow_drafts`、`workflow_draft_revisions`、`workflow_versions` 增加 `editor_json/editor_hash`，Definition 升级到 3.0。
- `workflow_debug_overlays` 保存 `workflow_id/node_id/kind/payload_artifact_id/schema_hash/updated_by`；节点删除时清理引用，Artifact 按保留策略回收。
- `workflow_executions` 增加 `source_kind/source_id/source_revision`，`workflow_version_id` 对 Draft Debug 可空。
- `execution_snapshots` 继续以 `execution_id` 为唯一身份，并增加 `manifest_snapshot_json`、`debug_plan_json` 和 `debug_overlay_snapshot_json`；执行创建后全部不可变。
- 不建立“隐藏 Workflow Version”或“临时发布版本”；开发库可直接清理旧 2.0 Fixture，不维护长期兼容层。

### 4.2 外部 API

- `GET /node-definitions`：分类、搜索、可用版本和摘要。
- `GET /node-definitions/{nodeType}/versions/{version}`：完整 Manifest。
- `GET/PUT /workflows/{id}/draft`：原子读写 Definition 和 Editor Document，继续使用 Expected Revision。
- `POST /workflows/{id}/draft/validate`：Schema、连接、资源、表达式和发布级问题，返回 nodeId/fieldPath。
- `GET/PUT/DELETE /workflows/{id}/debug-overlays/{nodeId}`：Pin/Mock 元数据和 Artifact。
- `POST /workflows/{id}/debug-executions`：提交 Revision、Debug Mode、Target、Input Source 和 Side Effect Decisions。
- `GET /executions/{id}/events` 或等价可恢复订阅：使用递增 Cursor；SSE 断线后以 Execution Query 校准。
- 现有 Version、Deployment、Execution、Node Result、Checkpoint、Fork 和 Trace API 继续复用。

所有新 API 进入 OpenAPI，前端只使用生成 Client。大结果返回 Artifact 元数据和按需下载地址，不把完整 Trace 或二进制放进 Zustand。

### 4.3 代码与服务影响

| 模块 | M6 改动 | 不承担内容 |
|---|---|---|
| `agentx-domain` | Definition 3.0、显式 Connection Order、Execution Source/Debug Plan 值对象 | React Flow、表单或运行高亮 |
| `agentx-node-protocol` | Manifest 展示/UI 元数据，运行请求中的 Version 关联可空 | n8n Node/Credential 协议 |
| `agentx-application` | `ExecutionRuntime` 接收 Version/Draft Revision Source；Node Catalog、Debug Overlay Port | SQL、Axum 和前端 DTO |
| `agentx-infrastructure` | Catalog/Draft Revision Repository、来源解析、编译与 Execution Snapshot 原子写入 | 页面状态和可变进程内快照 |
| `platform-api` | Draft/Validate/Catalog/Provider/Overlay/Debug Execution OpenAPI 与权限 | 直接执行节点 |
| `workflow-coordinator` | 验证 Source、固化 Debug Plan、为部分执行建立初始 Machine/Delivery | 读取 React Flow 或 Draft Head |
| `workflow-worker` / `sandbox-manager` | 从 Execution Snapshot 执行，接受 Draft Debug 下空 Version 关联 | 区分另一套 Debug Runner |
| `apps/web` | 分层 Studio、Manifest Form、Autosave、Debug/Trace Overlay 和生成 Client | 硬编码 Node Schema、直接调用节点服务 |
| Migration/Schema/E2E | 2.0 到 3.0 一次性升级、OpenAPI/gRPC/JSON Schema 和 Kubernetes UI 证据 | 长期双读和历史兼容层 |

## 5. 实施批次与任务

### M6-0：契约与迁移冻结

| 编号 | 状态 | 依赖 | 交付物 | 验收条件 |
|---|---|---|---|---|
| STU-001 | done | RUN-001–005、WCP-001–004 | Definition 3.0、Editor Document、Debug Overlay、Execution Source ADR/Schema/Migration | 运行图不含坐标/Pin/Mock；2.0 无双读；空库、升级库和 Schema Fixture 通过 |
| STU-002 | done | STU-001、RUN-002、AGT-013 | Node Catalog Reconcile、Repository、OpenAPI 和 Manifest 补全 | Studio API 与 Compiler 对同一 type/version 返回相同 Manifest Hash，Agent/Code/Approval 在 Catalog 可见 |
| STU-003 | done | STU-001–002、RUN-014 | Draft Revision Debug Execution Port、gRPC、Repository 和快照创建 | 指定 Revision 被原子固化；后续修改 Draft 不改变运行；Version 与 Draft 共用同一 Runtime |

### M6-1：画布与编辑器核心

| 编号 | 状态 | 依赖 | 交付物 | 验收条件 |
|---|---|---|---|---|
| STU-004 | done | STU-001–002 | 前端 Domain Model、Serializer/Deserializer 和 Query Adapter | Definition/Editor 往返不丢字段，纯 UI 字段不会进入 Definition/IR |
| STU-005 | done | STU-004 | 三栏 Studio Shell、Palette、通用 Node/Handle/Edge、搜索和拖放 | main/error/AI 端口视觉与 Manifest 一致，非法方向/类型/重复连接被拒绝 |
| STU-006 | done | STU-004–005 | 多选、复制粘贴、删除、框选、对齐、自动布局、Undo/Redo 和快捷键 | 每次用户动作形成单一可逆 Command；跨 Workflow 粘贴重新校验资源和节点版本 |
| STU-007 | done | STU-004、WCP-002 | 防抖自动保存、保存状态、Revision 冲突、离线恢复和离开保护 | 不显示虚假已保存；冲突可载入服务器、保留本地副本或显式覆盖 |

### M6-2：Manifest 配置与表达式

| 编号 | 状态 | 依赖 | 交付物 | 验收条件 |
|---|---|---|---|---|
| STU-008 | done | STU-002、STU-005 | JSON Schema + UI Schema 表单引擎和统一 Node Inspector | 必填、条件字段、Collection、Mapper、Credential、Provider 和字段错误前后端一致 |
| STU-009 | done | RES-007/011、STU-008 | Model/MCP Tool/Skill/RAG/Memory/Credential/Sandbox Profile 选择器 | 只展示用户可见且 Workflow Identity 可授权资源，递归依赖和缺失 Grant 可解释 |
| STU-010 | done | RUN-004、STU-008 | Agentx Expression/Prompt/JSON/Code 编辑器、补全、校验和脱敏预览 | 不执行任意 JavaScript；服务端 AST 校验为权威；Secret 不进入补全、预览和日志 |

### M6-3：真实调试与可观测

| 编号 | 状态 | 依赖 | 交付物 | 验收条件 |
|---|---|---|---|---|
| STU-011 | done | STU-003、STU-007、STU-010 | Full/Stop、Single/To/From Node 命令和可恢复事件订阅 | 运行使用当前已保存 Revision；断线重连不丢最终状态；停止落到真实 Execution |
| STU-012 | done | REC-001–003、STU-011 | Pin/Mock、输入来源选择、Side Effect 确认和 Input/Output 面板 | Overlay 独立存储；缺失来源拒绝运行；大输出按需加载 Artifact |
| STU-013 | done | OBS-006、REC-009、AGT-012、STU-011 | 运行路径高亮、Attempt、Lineage、Trace、Checkpoint、Fork 和错误定位 | 可定位 runIndex、来源分支、Agent Iteration、Tool Call、Sandbox、Artifact 和失败 Attempt |

### M6-4：版本、发布与阶段门禁

| 编号 | 状态 | 依赖 | 交付物 | 验收条件 |
|---|---|---|---|---|
| STU-014 | done | STU-001、STU-007–010 | Draft Diff、发布级校验、Version、Deployment、回滚和只读版本视图 | 发布只固化 Definition/资源/Manifest 和 Editor 快照，任何 Debug Overlay 均不进入运行 Version |
| STU-015 | done | STU-005–014 | 画布性能、键盘/焦点、中文/英文、浅色/深色和桌面视觉门禁 | 1280x800、1440x900、1920x1080 无遮挡；大型 Workflow 基线和降级策略有自动化证据 |
| STU-016 | done | STU-001–015 | 临时 Kubernetes Namespace 的完整 Studio Playwright E2E 和验收证据 | 仅通过 UI 创建、拖拽、连线、配置、调试和发布 Agent+MCP Tool+Code+Approval Workflow；`failures=0`、`skipped=0` |

## 6. 依赖与关键路径

```text
STU-001 Definition/Source
  +--> STU-002 Catalog -----> STU-008 Forms --> STU-009 Resources --> STU-010 Editor
  +--> STU-003 Debug Port -------------------------------------------> STU-011 Run
  +--> STU-004 Frontend Model --> STU-005 Canvas --> STU-006 Editing
                              +--> STU-007 Autosave ------------------> STU-011 Run
STU-011 --> STU-012 Debug Data --> STU-013 Trace
STU-007/008/009/010 --> STU-014 Version/Publish
STU-005..014 --> STU-015 Quality --> STU-016 E2E
```

不得在 STU-001～003 未冻结前并行大规模开发表单和调试 UI，否则会把现有 2.0/Version-only 假设固化进前端。

## 7. 失败、安全和并发边界

- Revision 冲突绝不自动覆盖；本地恢复数据不得包含 Credential 明文。
- Debug 请求只能运行调用者可编辑的 Workflow，且每次重新验证 Service Identity Grant。
- Pin/Mock 不继承到另一 Workflow；节点版本、输出 Schema 或资源版本变化后必须标记为 stale。
- 不可逆节点在 Single/From/Fork 前要求显式决策；重复 Idempotency Key 不创建第二次 Execution。
- SSE/事件断线只影响实时显示，不能改变 MySQL 中的 Execution 权威状态。
- Catalog 中被禁用的新版本不可新增；已有 Version 和 Execution 仍按固化 Manifest Hash 可解释。
- 前端不得直连 Model、MCP、OpenSandbox 或远程 Node Endpoint。

## 8. 测试策略

- Rust：Definition 3.0、Catalog Reconcile、Manifest Hash、Draft Debug Snapshot、资源快照和四类 Debug Plan 单元/集成测试。
- 前端：Serializer、Connection Validator、History Command、Schema Form、Autosave 状态机和 Runtime Overlay Store 测试。
- 契约：OpenAPI 生成、runtime gRPC、Node/Sandbox Context 中可选 Version 与必填 Snapshot ID 的兼容测试。
- E2E：按项目标准创建临时 Kubernetes Namespace；Fixture 只准备账号和外部依赖，不能通过 API/SQL 写入被测 Workflow。
- 故障：Revision 冲突、保存超时、事件断线、资源撤权、Worker 重启、Sandbox 超时、Artifact 延迟和重复命令。
- 视觉：三种桌面分辨率、中英文、浅深主题；Node Inspector、底部结果面板和 Dialog 不得遮挡顶栏或画布命令。

## 9. M6 验收门禁

- 用户只通过 Studio 即可创建、配置、调试、版本化和发布真实 Workflow。
- Agent、Code、Approval 和资源节点全部由 Manifest 驱动，画布不存在业务节点类型白名单。
- Draft Debug 运行精确 Revision 快照，修改草稿不会改变已创建 Execution。
- Worker/Compiler 不读取 Editor Document，Pin/Mock/运行高亮不进入 Version 或 IR。
- Full、Single、To、From、Stop、Trace、Checkpoint 和 Fork 使用真实 Runtime。
- 全部单元、集成、OpenAPI、构建、Kustomize 和临时 Kubernetes Playwright 门禁通过。

## 10. 对 M7 的稳定输出

- Definition 3.0、Editor Document、Node Catalog 和 Manifest UI 契约。
- Version Source 与 Draft Revision Source 共用的 ExecutionRuntime。
- 完整 Workflow Studio、调试快照、Trace 和发布闭环。
- Agentx 自有表达式和资源授权 UI；无 n8n 兼容债务。
- Studio E2E 与性能/视觉基线；M7 只做外围集成和生产加固。
